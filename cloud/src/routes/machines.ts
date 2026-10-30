import { and, eq } from "drizzle-orm";
import { Hono } from "hono";
import { z } from "zod";
import type { AppEnv } from "../app.js";
import { requireUser } from "../auth/sessions.js";
import type { Db } from "../db/client.js";
import { machineMembers, machines, users, type Machine, type MachineRole } from "../db/schema.js";
import { fail, parseBody } from "../lib/http.js";
import type { AgentHub } from "../relay/hub.js";
import { relayHandler } from "./relay.js";

const uuid = z.uuid();
const renameBody = z.object({ name: z.string().trim().min(1).max(64) });
const role = z.enum(["admin", "member"]);
const addMemberBody = z.object({ username: z.string().max(64), role });
const roleBody = z.object({ role });

/** The machine and the caller's role on it, or 404 (also when not a member, so ids don't leak). */
export async function membership(db: Db, machineId: string, userId: string) {
  if (!uuid.safeParse(machineId).success) fail(404, "machine_not_found");
  const [row] = await db
    .select({ machine: machines, role: machineMembers.role })
    .from(machineMembers)
    .innerJoin(machines, eq(machines.id, machineMembers.machineId))
    .where(and(eq(machineMembers.machineId, machineId), eq(machineMembers.userId, userId)))
    .limit(1);
  if (!row) fail(404, "machine_not_found");
  return row;
}

function machineView(m: Machine, role: MachineRole, userId: string, hub: AgentHub) {
  return {
    id: m.id,
    name: m.name,
    kind: m.kind,
    role,
    is_owner: m.ownerId === userId,
    paired: m.credentialHash !== null,
    version: m.version,
    capabilities: m.capabilities,
    last_seen: m.lastSeen?.toISOString() ?? null,
    created_at: m.createdAt.toISOString(),
    ...hub.status(m.id),
  };
}

export function machineRoutes() {
  const app = new Hono<AppEnv>();
  app.use("*", requireUser);

  app.get("/", async (c) => {
    const { db, hub } = c.var.deps;
    const rows = await db
      .select({ machine: machines, role: machineMembers.role })
      .from(machineMembers)
      .innerJoin(machines, eq(machines.id, machineMembers.machineId))
      .where(eq(machineMembers.userId, c.var.user.id))
      .orderBy(machines.createdAt);
    return c.json({ machines: rows.map((r) => machineView(r.machine, r.role, c.var.user.id, hub)) });
  });

  app.get("/:id", async (c) => {
    const { db, hub } = c.var.deps;
    const { machine, role } = await membership(db, c.req.param("id"), c.var.user.id);
    return c.json({ machine: machineView(machine, role, c.var.user.id, hub) });
  });

  app.patch("/:id", async (c) => {
    const { db, hub } = c.var.deps;
    const { machine, role } = await membership(db, c.req.param("id"), c.var.user.id);
    if (role !== "admin") fail(403, "forbidden");
    const { name } = await parseBody(c, renameBody);
    const [updated] = await db.update(machines).set({ name }).where(eq(machines.id, machine.id)).returning();
    return c.json({ machine: machineView(updated!, role, c.var.user.id, hub) });
  });

  /** Owner: remove the machine and revoke its credential. Others: leave the machine. */
  app.delete("/:id", async (c) => {
    const { db, hub } = c.var.deps;
    const { machine } = await membership(db, c.req.param("id"), c.var.user.id);
    if (machine.ownerId === c.var.user.id) {
      await db.delete(machines).where(eq(machines.id, machine.id));
      hub.disconnect(machine.id, "machine removed from account");
      return c.json({ ok: true, removed: true });
    }
    await db
      .delete(machineMembers)
      .where(and(eq(machineMembers.machineId, machine.id), eq(machineMembers.userId, c.var.user.id)));
    return c.json({ ok: true, removed: false });
  });

  // ── Members ──

  app.get("/:id/members", async (c) => {
    const { db } = c.var.deps;
    const { machine } = await membership(db, c.req.param("id"), c.var.user.id);
    const rows = await db
      .select({ id: users.id, username: users.username, role: machineMembers.role })
      .from(machineMembers)
      .innerJoin(users, eq(users.id, machineMembers.userId))
      .where(eq(machineMembers.machineId, machine.id))
      .orderBy(users.username);
    return c.json({ members: rows.map((r) => ({ ...r, is_owner: r.id === machine.ownerId })) });
  });

  app.post("/:id/members", async (c) => {
    const { db } = c.var.deps;
    const { machine, role } = await membership(db, c.req.param("id"), c.var.user.id);
    if (role !== "admin") fail(403, "forbidden");
    const body = await parseBody(c, addMemberBody);
    const [user] = await db.select().from(users).where(eq(users.username, body.username)).limit(1);
    if (!user) fail(404, "user_not_found", "No Tessera account with that username.");
    const inserted = await db
      .insert(machineMembers)
      .values({ machineId: machine.id, userId: user.id, role: body.role })
      .onConflictDoNothing()
      .returning();
    if (!inserted.length) fail(409, "already_member");
    return c.json({ member: { id: user.id, username: user.username, role: body.role, is_owner: false } }, 201);
  });

  app.patch("/:id/members/:userId", async (c) => {
    const { db } = c.var.deps;
    const { machine, role } = await membership(db, c.req.param("id"), c.var.user.id);
    if (role !== "admin") fail(403, "forbidden");
    const target = c.req.param("userId");
    if (target === machine.ownerId) fail(400, "owner_role_fixed", "The owner is always an admin.");
    const body = await parseBody(c, roleBody);
    const updated = await db
      .update(machineMembers)
      .set({ role: body.role })
      .where(and(eq(machineMembers.machineId, machine.id), eq(machineMembers.userId, target)))
      .returning();
    if (!updated.length) fail(404, "member_not_found");
    return c.json({ ok: true });
  });

  app.delete("/:id/members/:userId", async (c) => {
    const { db } = c.var.deps;
    const { machine, role } = await membership(db, c.req.param("id"), c.var.user.id);
    const target = c.req.param("userId");
    if (role !== "admin" && target !== c.var.user.id) fail(403, "forbidden");
    if (target === machine.ownerId) fail(400, "owner_cannot_leave", "Remove the machine instead.");
    const removed = await db
      .delete(machineMembers)
      .where(and(eq(machineMembers.machineId, machine.id), eq(machineMembers.userId, target)))
      .returning();
    if (!removed.length) fail(404, "member_not_found");
    return c.json({ ok: true });
  });

  // Everything else under a machine is CSCM's own API, relayed to the agent.
  app.all("/:id/*", relayHandler);

  return app;
}
