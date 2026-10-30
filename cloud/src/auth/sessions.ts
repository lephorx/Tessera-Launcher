import { and, eq, gt } from "drizzle-orm";
import type { Context, MiddlewareHandler } from "hono";
import { getCookie } from "hono/cookie";
import type { AppEnv } from "../app.js";
import type { Db } from "../db/client.js";
import { sessions, users, type User } from "../db/schema.js";
import { fail } from "../lib/http.js";
import { hashToken, randomToken } from "../lib/tokens.js";

export const SESSION_COOKIE = "tessera_session";
const SESSION_TTL_MS = 30 * 24 * 60 * 60 * 1000;
const TOUCH_INTERVAL_MS = 5 * 60 * 1000;
const SAFE_METHODS = new Set(["GET", "HEAD", "OPTIONS"]);

export async function createSession(db: Db, userId: string, userAgent?: string) {
  const token = randomToken();
  const expiresAt = new Date(Date.now() + SESSION_TTL_MS);
  await db.insert(sessions).values({ userId, tokenHash: hashToken(token), userAgent, expiresAt });
  return { token, expiresAt };
}

export async function deleteSession(db: Db, token: string) {
  await db.delete(sessions).where(eq(sessions.tokenHash, hashToken(token)));
}

/** The launcher sends `Authorization: Bearer <token>`; the web dashboard uses the cookie. */
function readToken(c: Context): { token: string; viaCookie: boolean } | null {
  const header = c.req.header("authorization");
  if (header?.startsWith("Bearer ")) return { token: header.slice(7), viaCookie: false };
  const cookie = getCookie(c, SESSION_COOKIE);
  return cookie ? { token: cookie, viaCookie: true } : null;
}

export function sessionToken(c: Context): string | null {
  return readToken(c)?.token ?? null;
}

export const requireUser: MiddlewareHandler<AppEnv> = async (c, next) => {
  const found = readToken(c);
  if (!found) fail(401, "unauthenticated");

  // Cookies are sent cross-site, so cookie-authenticated writes must come from an allowed origin.
  if (found.viaCookie && !SAFE_METHODS.has(c.req.method)) {
    const origin = c.req.header("origin");
    const { config } = c.var.deps;
    const allowed = [new URL(config.PUBLIC_URL).origin, ...config.WEB_ORIGINS];
    if (!origin || !allowed.includes(origin)) fail(403, "bad_origin");
  }

  const { db } = c.var.deps;
  const [row] = await db
    .select({ session: sessions, user: users })
    .from(sessions)
    .innerJoin(users, eq(users.id, sessions.userId))
    .where(and(eq(sessions.tokenHash, hashToken(found.token)), gt(sessions.expiresAt, new Date())))
    .limit(1);
  if (!row) fail(401, "unauthenticated");

  if (Date.now() - row.session.lastUsedAt.getTime() > TOUCH_INTERVAL_MS) {
    await db.update(sessions).set({ lastUsedAt: new Date() }).where(eq(sessions.id, row.session.id));
  }
  c.set("user", row.user);
  await next();
};

export function publicUser(user: User) {
  return { id: user.id, email: user.email, username: user.username, totp_enabled: user.totpEnabled };
}
