import { eq, or } from "drizzle-orm";
import { Hono } from "hono";
import { deleteCookie, setCookie } from "hono/cookie";
import { getConnInfo } from "@hono/node-server/conninfo";
import { z } from "zod";
import type { AppEnv } from "../app.js";
import { checkTotp, dummyPasswordHash, hashPassword, newTotpSecret, totpUri, verifyPassword } from "../auth/passwords.js";
import { createSession, deleteSession, publicUser, requireUser, SESSION_COOKIE, sessionToken } from "../auth/sessions.js";
import { sessions, users } from "../db/schema.js";
import { fail, parseBody, RateLimiter } from "../lib/http.js";

const email = z.email().max(254).transform((e) => e.toLowerCase());
const password = z.string().min(10, "at least 10 characters").max(256);
const username = z
  .string()
  .regex(/^[a-zA-Z0-9_]{3,24}$/, "3–24 letters, digits or underscores");

const registerBody = z.object({ email, username, password });
const loginBody = z.object({ email, password: z.string().max(256), totp: z.string().max(16).optional() });
const codeBody = z.object({ code: z.string().max(16) });
const disableTotpBody = z.object({ password: z.string().max(256), code: z.string().max(16) });

export function clientIp(c: Parameters<typeof getConnInfo>[0]): string {
  try {
    return getConnInfo(c).remote.address ?? "unknown";
  } catch {
    return "unknown"; // app.request() in tests has no socket
  }
}

export function authRoutes() {
  const app = new Hono<AppEnv>();
  const loginLimiter = new RateLimiter(10, 60_000);

  function startSession(c: Parameters<typeof setCookie>[0], token: string, expiresAt: Date) {
    setCookie(c, SESSION_COOKIE, token, {
      httpOnly: true,
      secure: c.var.deps.config.PUBLIC_URL.startsWith("https://"),
      sameSite: "Lax",
      path: "/",
      expires: expiresAt,
    });
  }

  app.post("/register", async (c) => {
    const body = await parseBody(c, registerBody);
    const { db } = c.var.deps;
    const taken = await db
      .select({ email: users.email, username: users.username })
      .from(users)
      .where(or(eq(users.email, body.email), eq(users.username, body.username)))
      .limit(1);
    if (taken[0]) fail(409, taken[0].email === body.email ? "email_taken" : "username_taken");

    const [user] = await db
      .insert(users)
      .values({ email: body.email, username: body.username, passwordHash: await hashPassword(body.password) })
      .returning();
    const session = await createSession(db, user!.id, c.req.header("user-agent"));
    startSession(c, session.token, session.expiresAt);
    return c.json({ token: session.token, user: publicUser(user!) }, 201);
  });

  app.post("/login", async (c) => {
    if (!loginLimiter.take(clientIp(c))) fail(429, "rate_limited", "Too many sign-in attempts. Wait a minute.");
    const body = await parseBody(c, loginBody);
    const { db } = c.var.deps;
    const [user] = await db.select().from(users).where(eq(users.email, body.email)).limit(1);
    const ok = await verifyPassword(user?.passwordHash ?? (await dummyPasswordHash()), body.password);
    if (!user || !ok) fail(401, "invalid_credentials", "Wrong email or password.");

    if (user.totpEnabled) {
      if (!body.totp) fail(401, "totp_required", "Enter the code from your authenticator app.");
      if (!(await checkTotp(user.totpSecret!, body.totp))) fail(401, "invalid_totp", "That code is not valid.");
    }

    const session = await createSession(db, user.id, c.req.header("user-agent"));
    startSession(c, session.token, session.expiresAt);
    return c.json({ token: session.token, user: publicUser(user) });
  });

  app.post("/logout", requireUser, async (c) => {
    await deleteSession(c.var.deps.db, sessionToken(c)!);
    deleteCookie(c, SESSION_COOKIE, { path: "/" });
    return c.json({ ok: true });
  });

  /** Signs out every device, including this one. */
  app.post("/logout-all", requireUser, async (c) => {
    await c.var.deps.db.delete(sessions).where(eq(sessions.userId, c.var.user.id));
    deleteCookie(c, SESSION_COOKIE, { path: "/" });
    return c.json({ ok: true });
  });

  app.get("/me", requireUser, (c) => c.json({ user: publicUser(c.var.user) }));

  // ── Two-factor (authenticator app) ──

  /** Step 1: a fresh secret to show as a QR code. Not active until confirmed. */
  app.post("/totp/setup", requireUser, async (c) => {
    const user = c.var.user;
    if (user.totpEnabled) fail(409, "totp_already_enabled");
    const secret = newTotpSecret();
    await c.var.deps.db.update(users).set({ totpSecret: secret }).where(eq(users.id, user.id));
    return c.json({ secret, otpauth_url: totpUri(secret, user.email) });
  });

  /** Step 2: confirm with a code from the app. */
  app.post("/totp/enable", requireUser, async (c) => {
    const { code } = await parseBody(c, codeBody);
    const user = c.var.user;
    if (user.totpEnabled) fail(409, "totp_already_enabled");
    if (!user.totpSecret) fail(400, "totp_not_set_up");
    if (!(await checkTotp(user.totpSecret, code))) fail(400, "invalid_totp", "That code is not valid.");
    await c.var.deps.db.update(users).set({ totpEnabled: true }).where(eq(users.id, user.id));
    return c.json({ ok: true });
  });

  app.post("/totp/disable", requireUser, async (c) => {
    const body = await parseBody(c, disableTotpBody);
    const user = c.var.user;
    if (!user.totpEnabled) fail(409, "totp_not_enabled");
    if (!(await verifyPassword(user.passwordHash, body.password))) fail(401, "invalid_credentials");
    if (!(await checkTotp(user.totpSecret!, body.code))) fail(400, "invalid_totp");
    await c.var.deps.db.update(users).set({ totpEnabled: false, totpSecret: null }).where(eq(users.id, user.id));
    return c.json({ ok: true });
  });

  return app;
}
