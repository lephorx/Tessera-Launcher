/**
 * Frames on the agent WebSocket (`wss://<cloud>/agent`). JSON text frames.
 * See docs/02-architecture.md → "Relay protocol". Bump PROTOCOL_VERSION on breaking changes.
 */
import { z } from "zod";

export const PROTOCOL_VERSION = 1;

/** Bodies are UTF-8 text unless `encoding` is "base64" (files, icons, backups). */
const encoding = z.enum(["utf8", "base64"]).default("utf8");
const headers = z.record(z.string(), z.string()).default({});

// ── cloud → agent ────────────────────────────────────────────────────────

export type CloudFrame =
  | {
      type: "request";
      id: string;
      method: string;
      /** Always starts with `/api/`. */
      path: string;
      headers: Record<string, string>;
      body?: string;
      encoding?: "utf8" | "base64";
    }
  /** The caller went away: stop streaming this response. */
  | { type: "cancel"; id: string }
  | { type: "welcome"; protocol: number; machine_id: string };

// ── agent → cloud ────────────────────────────────────────────────────────

export const agentFrame = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("hello"),
    version: z.string().max(64),
    protocol: z.number().int().optional(),
    servers: z.number().int().nonnegative().optional(),
    capabilities: z.array(z.string().max(64)).max(64).default([]),
  }),
  /**
   * Answer to a request. With `stream: true` the body follows as `chunk` frames until `end`
   * (console, progress, downloads); otherwise `body` is the whole response.
   */
  z.object({
    type: z.literal("response"),
    id: z.string(),
    status: z.number().int().min(100).max(599),
    headers,
    body: z.string().optional(),
    encoding,
    stream: z.boolean().default(false),
  }),
  z.object({ type: z.literal("chunk"), id: z.string(), data: z.string(), encoding }),
  z.object({ type: z.literal("end"), id: z.string() }),
  /** The agent could not handle the request at all (route not allowed, crash). */
  z.object({ type: z.literal("error"), id: z.string(), status: z.number().int().default(502), message: z.string() }),
]);

export type AgentFrame = z.infer<typeof agentFrame>;

export function decodeBody(data: string, enc: "utf8" | "base64"): Uint8Array {
  return enc === "base64" ? Buffer.from(data, "base64") : Buffer.from(data, "utf8");
}
