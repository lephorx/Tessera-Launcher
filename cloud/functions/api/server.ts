/**
 * The API as a plain Node server, for the home server (s-ubutsra01) behind Cloudflare Tunnel:
 * api.tesseralauncher.com → cloudflared → 127.0.0.1:8787. Same code as the Neon Function, same
 * database and bucket; the env comes from /etc/tessera/api.env (see deploy/README.md).
 */
import { serve } from "@hono/node-server";
import app from "./index.js";

const port = Number(process.env.PORT ?? 8787);
serve({ fetch: app.fetch, port, hostname: process.env.HOST ?? "127.0.0.1" }, (info) => console.log(`Tessera API on ${info.address}:${info.port}`));
