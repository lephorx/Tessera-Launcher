import { randomBytes } from "node:crypto";
import { eq } from "drizzle-orm";
import type { Logger } from "pino";
import type { WebSocket } from "ws";
import type { Db } from "../db/client.js";
import { machines } from "../db/schema.js";
import { agentFrame, decodeBody, PROTOCOL_VERSION, type AgentFrame, type CloudFrame } from "./protocol.js";

export class RelayError extends Error {
  constructor(
    public status: number,
    public code: string,
    message?: string,
  ) {
    super(message ?? code);
  }
}

export interface RelayRequest {
  method: string;
  path: string;
  headers: Record<string, string>;
  body?: Uint8Array;
}

export interface RelayResponse {
  status: number;
  headers: Record<string, string>;
  body: Uint8Array | ReadableStream<Uint8Array>;
}

interface Pending {
  resolve(res: RelayResponse): void;
  reject(err: RelayError): void;
  timer?: NodeJS.Timeout;
  /** Set once a streamed response has started. */
  stream?: ReadableStreamDefaultController<Uint8Array>;
}

const HEARTBEAT_MS = 25_000;
const MAX_PENDING_PER_MACHINE = 64;

class AgentConnection {
  pending = new Map<string, Pending>();
  alive = true;
  version: string | null = null;
  connectedAt = new Date();

  constructor(
    readonly machineId: string,
    readonly ws: WebSocket,
  ) {}

  send(frame: CloudFrame) {
    this.ws.send(JSON.stringify(frame));
  }

  /** Ends every open request; used when the socket goes away. */
  failAll(err: RelayError) {
    for (const p of this.pending.values()) {
      clearTimeout(p.timer);
      if (p.stream) p.stream.error(err);
      else p.reject(err);
    }
    this.pending.clear();
  }
}

/** Keeps one WebSocket per connected machine and relays HTTP requests over it. In memory: one cloud instance. */
export class AgentHub {
  private agents = new Map<string, AgentConnection>();
  private heartbeat: NodeJS.Timeout;

  constructor(
    private db: Db,
    private log: Logger,
    private timeoutMs: number,
  ) {
    this.heartbeat = setInterval(() => this.sweep(), HEARTBEAT_MS);
    this.heartbeat.unref();
  }

  isOnline(machineId: string): boolean {
    return this.agents.has(machineId);
  }

  status(machineId: string) {
    const conn = this.agents.get(machineId);
    return conn ? { online: true, connected_at: conn.connectedAt.toISOString() } : { online: false, connected_at: null };
  }

  attach(machineId: string, ws: WebSocket) {
    // A newer connection from the same machine wins (agent restarted, network changed).
    this.agents.get(machineId)?.ws.close(4000, "replaced by a newer connection");

    const conn = new AgentConnection(machineId, ws);
    this.agents.set(machineId, conn);
    this.log.info({ machineId }, "agent connected");
    conn.send({ type: "welcome", protocol: PROTOCOL_VERSION, machine_id: machineId });

    ws.on("pong", () => (conn.alive = true));
    ws.on("message", (data, isBinary) => {
      if (isBinary) return;
      let frame: AgentFrame;
      try {
        frame = agentFrame.parse(JSON.parse(data.toString()));
      } catch {
        this.log.warn({ machineId }, "invalid frame from agent");
        return;
      }
      void this.onFrame(conn, frame);
    });
    ws.on("close", () => {
      conn.failAll(new RelayError(502, "agent_disconnected", "The machine disconnected."));
      if (this.agents.get(machineId) === conn) {
        this.agents.delete(machineId);
        this.log.info({ machineId }, "agent disconnected");
        void this.touch(machineId).catch(() => {});
      }
    });
    ws.on("error", (err) => this.log.warn({ machineId, err: err.message }, "agent socket error"));
  }

  /** Closes the machine's socket, e.g. after the machine was removed (its credential no longer exists). */
  disconnect(machineId: string, reason = "removed") {
    this.agents.get(machineId)?.ws.close(4001, reason);
  }

  request(machineId: string, req: RelayRequest, signal?: AbortSignal): Promise<RelayResponse> {
    const conn = this.agents.get(machineId);
    if (!conn) return Promise.reject(new RelayError(503, "machine_offline", "The machine is not connected."));
    if (conn.pending.size >= MAX_PENDING_PER_MACHINE)
      return Promise.reject(new RelayError(429, "too_many_requests", "Too many requests in flight for this machine."));

    const id = randomBytes(9).toString("base64url");
    return new Promise<RelayResponse>((resolve, reject) => {
      const cancel = (err: RelayError) => {
        const p = conn.pending.get(id);
        if (!p) return;
        clearTimeout(p.timer);
        conn.pending.delete(id);
        if (conn.ws.readyState === conn.ws.OPEN) conn.send({ type: "cancel", id });
        if (p.stream) p.stream.error(err);
        else p.reject(err);
      };
      conn.pending.set(id, {
        resolve,
        reject,
        timer: setTimeout(
          () => cancel(new RelayError(504, "agent_timeout", "The machine did not answer in time.")),
          this.timeoutMs,
        ),
      });
      signal?.addEventListener("abort", () => cancel(new RelayError(499, "client_closed")), { once: true });

      conn.send({
        type: "request",
        id,
        method: req.method,
        path: req.path,
        headers: req.headers,
        ...(req.body?.length ? { body: Buffer.from(req.body).toString("base64"), encoding: "base64" as const } : {}),
      });
    });
  }

  close() {
    clearInterval(this.heartbeat);
    for (const conn of this.agents.values()) conn.ws.close(1001, "cloud shutting down");
    this.agents.clear();
  }

  private async onFrame(conn: AgentConnection, frame: AgentFrame) {
    if (frame.type === "hello") {
      conn.version = frame.version;
      await this.db
        .update(machines)
        .set({ version: frame.version, capabilities: frame.capabilities, lastSeen: new Date() })
        .where(eq(machines.id, conn.machineId));
      return;
    }

    const p = conn.pending.get(frame.id);
    if (!p) return; // cancelled or timed out already

    switch (frame.type) {
      case "response": {
        clearTimeout(p.timer);
        p.timer = undefined;
        if (!frame.stream) {
          conn.pending.delete(frame.id);
          p.resolve({
            status: frame.status,
            headers: frame.headers,
            body: frame.body === undefined ? new Uint8Array() : decodeBody(frame.body, frame.encoding),
          });
          return;
        }
        const body = new ReadableStream<Uint8Array>({
          start: (controller) => {
            p.stream = controller;
          },
          cancel: () => {
            if (conn.pending.delete(frame.id) && conn.ws.readyState === conn.ws.OPEN)
              conn.send({ type: "cancel", id: frame.id });
          },
        });
        p.resolve({ status: frame.status, headers: frame.headers, body });
        return;
      }
      case "chunk":
        p.stream?.enqueue(decodeBody(frame.data, frame.encoding));
        return;
      case "end":
        conn.pending.delete(frame.id);
        p.stream?.close();
        return;
      case "error": {
        clearTimeout(p.timer);
        conn.pending.delete(frame.id);
        const err = new RelayError(frame.status, "agent_error", frame.message);
        if (p.stream) p.stream.error(err);
        else p.reject(err);
        return;
      }
    }
  }

  private sweep() {
    for (const conn of this.agents.values()) {
      if (!conn.alive) {
        this.log.info({ machineId: conn.machineId }, "agent missed heartbeat");
        conn.ws.terminate();
        continue;
      }
      conn.alive = false;
      conn.ws.ping();
    }
  }

  private async touch(machineId: string) {
    await this.db.update(machines).set({ lastSeen: new Date() }).where(eq(machines.id, machineId));
  }
}
