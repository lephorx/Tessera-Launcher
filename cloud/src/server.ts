import type { IncomingMessage, Server } from "node:http";
import type { Duplex } from "node:stream";
import { serve } from "@hono/node-server";
import { eq } from "drizzle-orm";
import pino, { type Logger } from "pino";
import { WebSocketServer } from "ws";
import { createApp } from "./app.js";
import type { Config } from "./config.js";
import { openDatabase } from "./db/client.js";
import { machines } from "./db/schema.js";
import { hashToken } from "./lib/tokens.js";
import { AgentHub } from "./relay/hub.js";

export interface RunningServer {
  port: number;
  hub: AgentHub;
  close(): Promise<void>;
}

/** Starts the HTTP API and the agent WebSocket endpoint (`/agent`) on one port. */
export async function startServer(config: Config, log: Logger = pino({ level: config.LOG_LEVEL })): Promise<RunningServer> {
  const database = await openDatabase({ url: config.DATABASE_URL, dataDir: config.DATA_DIR });
  const hub = new AgentHub(database.db, log, config.relayTimeoutMs);
  const app = createApp({ db: database.db, config, hub, log });

  const wss = new WebSocketServer({ noServer: true, maxPayload: 16 * 1024 * 1024 });

  const server = await new Promise<Server>((resolve) => {
    const s = serve({ fetch: app.fetch, port: config.PORT }, () => resolve(s as Server)) as Server;
  });

  server.on("upgrade", (req: IncomingMessage, socket: Duplex, head: Buffer) => {
    const reject = (status: number, text: string) => {
      socket.end(`HTTP/1.1 ${status} ${text}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n`);
    };
    const path = new URL(req.url ?? "/", "http://x").pathname;
    if (path !== "/agent") return reject(404, "Not Found");

    const auth = req.headers.authorization;
    if (!auth?.startsWith("Bearer ")) return reject(401, "Unauthorized");
    database.db
      .select({ id: machines.id })
      .from(machines)
      .where(eq(machines.credentialHash, hashToken(auth.slice(7))))
      .limit(1)
      .then(([machine]) => {
        if (!machine) return reject(401, "Unauthorized");
        wss.handleUpgrade(req, socket, head, (ws) => hub.attach(machine.id, ws));
      })
      .catch((err) => {
        log.error({ err }, "agent upgrade failed");
        reject(500, "Internal Server Error");
      });
  });

  const address = server.address();
  const port = typeof address === "object" && address ? address.port : config.PORT;
  log.info({ port }, "tessera cloud listening");

  return {
    port,
    hub,
    async close() {
      hub.close();
      wss.close();
      server.closeAllConnections?.();
      await new Promise<void>((r) => server.close(() => r()));
      await database.close();
    },
  };
}
