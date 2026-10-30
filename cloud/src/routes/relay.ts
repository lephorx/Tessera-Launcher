import type { Context } from "hono";
import type { AppEnv } from "../app.js";
import { fail, RateLimiter } from "../lib/http.js";
import { RelayError } from "../relay/hub.js";
import { membership } from "./machines.js";

/** Request headers the agent gets from the caller. Cookies and our own auth never leave the cloud. */
const FORWARD_REQUEST = ["accept", "accept-language", "content-type", "if-none-match", "last-event-id", "range"];
/** Response headers we never pass back from the agent. */
const DROP_RESPONSE = new Set(["set-cookie", "connection", "keep-alive", "transfer-encoding", "content-length", "upgrade"]);

/** Relayed requests per user: 600 per minute (a dashboard polls a few routes every few seconds). */
const perUser = new RateLimiter(600, 60_000);

/**
 * `/api/machines/<id>/<rest>` → the agent's `/api/<rest>`, so the CSCM frontend works with
 * `API_BASE = /api/machines/<id>`. The agent applies its own route allowlist and maps
 * `x-cscm-user` to a local user (docs/05-cscm-changes.md, phase 2).
 */
export async function relayHandler(c: Context<AppEnv>) {
  const { db, hub, config } = c.var.deps;
  if (!perUser.take(c.var.user.id)) fail(429, "rate_limited", "Too many requests to this machine. Slow down a little.");
  const machineId = c.req.param("id")!;
  const { role } = await membership(db, machineId, c.var.user.id);

  const url = new URL(c.req.url);
  const prefix = `/api/machines/${machineId}`;
  const rest = url.pathname.slice(prefix.length); // "/servers/3/start"
  if (!rest.startsWith("/") || rest.startsWith("/members")) fail(404, "not_found");
  const path = `/api${rest}${url.search}`;

  const headers: Record<string, string> = {};
  for (const name of FORWARD_REQUEST) {
    const v = c.req.header(name);
    if (v) headers[name] = v;
  }
  headers["x-cscm-user"] = c.var.user.id;
  headers["x-cscm-username"] = c.var.user.username;
  headers["x-cscm-role"] = role;

  let body: Uint8Array | undefined;
  if (!["GET", "HEAD"].includes(c.req.method)) {
    const declared = Number(c.req.header("content-length") ?? 0);
    if (declared > config.relayMaxBodyBytes) fail(413, "body_too_large");
    body = new Uint8Array(await c.req.arrayBuffer());
    if (body.length > config.relayMaxBodyBytes) fail(413, "body_too_large");
  }

  try {
    const res = await hub.request(machineId, { method: c.req.method, path, headers, body }, c.req.raw.signal);
    const out = new Headers();
    for (const [k, v] of Object.entries(res.headers)) if (!DROP_RESPONSE.has(k.toLowerCase())) out.set(k, v);
    out.set("x-tessera-relay", "1");
    return new Response(c.req.method === "HEAD" ? null : (res.body as BodyInit), { status: res.status, headers: out });
  } catch (err) {
    if (err instanceof RelayError) {
      const status = err.status === 499 ? 400 : err.status;
      return c.json({ error: err.code, message: err.message }, status as 400);
    }
    throw err;
  }
}
