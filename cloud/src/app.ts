import { Hono } from "hono";
import { HTTPException } from "hono/http-exception";
import { RateLimiter } from "./lib/http.js";
import { clientIp } from "./routes/auth.js";
import type { Logger } from "pino";
import type { Config } from "./config.js";
import type { Db } from "./db/client.js";
import type { User } from "./db/schema.js";
import type { AgentHub } from "./relay/hub.js";
import { authRoutes } from "./routes/auth.js";
import { machineRoutes } from "./routes/machines.js";
import { pairingRoutes } from "./routes/pairing.js";

export interface Deps {
  db: Db;
  config: Config;
  hub: AgentHub;
  log: Logger;
}

export type AppEnv = { Variables: { deps: Deps; user: User } };

export const VERSION = "0.1.0";

export function createApp(deps: Deps) {
  const app = new Hono<AppEnv>();

  app.use("*", async (c, next) => {
    c.set("deps", deps);
    await next();
  });

  // Every API route: 300 requests per minute per address. Login and pairing have tighter limits
  // of their own, and the relay is limited per user (routes/relay.ts).
  const perIp = new RateLimiter(300, 60_000);
  app.use("/api/*", async (c, next) => {
    if (!perIp.take(clientIp(c))) {
      return c.json({ error: "rate_limited", message: "Too many requests. Slow down a little." }, 429, { "retry-after": "60" });
    }
    await next();
  });

  app.get("/health", (c) => c.json({ ok: true, version: VERSION }));
  app.route("/api/auth", authRoutes());
  app.route("/api/pairing", pairingRoutes());
  app.route("/api/machines", machineRoutes());

  app.notFound((c) => c.json({ error: "not_found" }, 404));
  app.onError((err, c) => {
    if (err instanceof HTTPException) return err.getResponse();
    deps.log.error({ err, path: c.req.path }, "unhandled error");
    return c.json({ error: "internal_error" }, 500);
  });

  return app;
}
