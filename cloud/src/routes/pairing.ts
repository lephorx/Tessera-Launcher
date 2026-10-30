import { and, eq, gt, isNull, lt } from "drizzle-orm";
import { Hono } from "hono";
import { z } from "zod";
import type { AppEnv } from "../app.js";
import { requireUser } from "../auth/sessions.js";
import { machineMembers, machines, pairingCodes, users } from "../db/schema.js";
import { fail, parseBody, RateLimiter } from "../lib/http.js";
import { formatPairingCode, hashToken, normalisePairingCode, pairingCode, randomToken, tokenMatches } from "../lib/tokens.js";
import { clientIp } from "./auth.js";

const PAIRING_TTL_S = 600;

const startBody = z.object({
  machine_hint: z.string().trim().max(64).optional(),
  version: z.string().max(64).optional(),
});
const exchangeBody = z.object({ code: z.string().max(16), secret: z.string().max(128) });
const claimBody = z.object({ code: z.string().max(16), name: z.string().trim().min(1).max(64).optional() });

/**
 * Pairing, see docs/02-architecture.md:
 *   agent  POST /api/pairing           → code + secret
 *   user   POST /api/pairing/claim     → binds the code to a new machine on the account
 *   agent  POST /api/pairing/exchange  → (polls) once claimed: the machine credential, exactly once
 */
export function pairingRoutes() {
  const app = new Hono<AppEnv>();
  const startLimiter = new RateLimiter(10, 60_000);
  const claimLimiter = new RateLimiter(10, 60_000);

  app.post("/", async (c) => {
    if (!startLimiter.take(clientIp(c))) fail(429, "rate_limited");
    const body = await parseBody(c, startBody);
    const { db } = c.var.deps;
    await db.delete(pairingCodes).where(lt(pairingCodes.expiresAt, new Date()));

    const secret = randomToken();
    const expiresAt = new Date(Date.now() + PAIRING_TTL_S * 1000);
    for (let attempt = 0; attempt < 5; attempt++) {
      const code = pairingCode();
      const inserted = await db
        .insert(pairingCodes)
        .values({ code, secretHash: hashToken(secret), machineHint: body.machine_hint, version: body.version, expiresAt })
        .onConflictDoNothing()
        .returning({ code: pairingCodes.code });
      if (inserted.length) return c.json({ code: formatPairingCode(code), secret, expires_in: PAIRING_TTL_S }, 201);
    }
    fail(503, "no_code_available");
  });

  app.post("/exchange", async (c) => {
    const body = await parseBody(c, exchangeBody);
    const { db, config } = c.var.deps;
    const [row] = await db
      .select()
      .from(pairingCodes)
      .where(eq(pairingCodes.code, normalisePairingCode(body.code)))
      .limit(1);
    if (!row || !tokenMatches(body.secret, row.secretHash)) fail(404, "unknown_code");
    if (row.exchangedAt) fail(410, "already_exchanged");
    if (!row.machineId) {
      if (row.expiresAt < new Date()) fail(410, "expired");
      return c.json({ status: "pending" }, 202);
    }

    const credential = randomToken();
    // Mark the code used and set the credential atomically, so it is handed out exactly once.
    const done = await db.transaction(async (tx) => {
      const used = await tx
        .update(pairingCodes)
        .set({ exchangedAt: new Date() })
        .where(and(eq(pairingCodes.code, row.code), isNull(pairingCodes.exchangedAt)))
        .returning({ code: pairingCodes.code });
      if (!used.length) return false;
      await tx.update(machines).set({ credentialHash: hashToken(credential) }).where(eq(machines.id, row.machineId!));
      return true;
    });
    if (!done) fail(410, "already_exchanged");

    const [owner] = await db
      .select({ id: users.id, username: users.username })
      .from(machines)
      .innerJoin(users, eq(users.id, machines.ownerId))
      .where(eq(machines.id, row.machineId))
      .limit(1);
    const agentUrl = new URL("/agent", config.PUBLIC_URL);
    agentUrl.protocol = agentUrl.protocol === "https:" ? "wss:" : "ws:";
    // `owner` lets the agent link the account that claimed the code to the local admin who started pairing.
    return c.json({ status: "paired", machine_id: row.machineId, credential, agent_url: agentUrl.toString(), owner });
  });

  app.post("/claim", requireUser, async (c) => {
    if (!claimLimiter.take(c.var.user.id)) fail(429, "rate_limited");
    const body = await parseBody(c, claimBody);
    const { db } = c.var.deps;
    const code = normalisePairingCode(body.code);

    const machine = await db.transaction(async (tx) => {
      const [row] = await tx
        .select()
        .from(pairingCodes)
        .where(and(eq(pairingCodes.code, code), gt(pairingCodes.expiresAt, new Date()), isNull(pairingCodes.machineId)))
        .limit(1);
      if (!row) return null;
      const [m] = await tx
        .insert(machines)
        .values({
          ownerId: c.var.user.id,
          name: body.name || row.machineHint || "My machine",
          version: row.version,
        })
        .returning();
      await tx.insert(machineMembers).values({ machineId: m!.id, userId: c.var.user.id, role: "admin" });
      const claimed = await tx
        .update(pairingCodes)
        .set({ machineId: m!.id })
        .where(and(eq(pairingCodes.code, code), isNull(pairingCodes.machineId)))
        .returning({ code: pairingCodes.code });
      if (!claimed.length) throw new Error("pairing code claimed concurrently");
      return m!;
    });
    if (!machine) fail(404, "invalid_code", "That code is wrong or has expired. Get a new one in CSCM.");
    return c.json({ machine: { id: machine.id, name: machine.name } }, 201);
  });

  return app;
}
