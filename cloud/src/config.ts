import { z } from "zod";

const schema = z.object({
  DATABASE_URL: z.string().optional().transform((v) => v || undefined),
  DATA_DIR: z.string().default(".data"),
  PORT: z.coerce.number().int().nonnegative().default(8080),
  PUBLIC_URL: z.url().default("http://localhost:8080"),
  WEB_ORIGINS: z
    .string()
    .default("")
    .transform((v) => v.split(",").map((s) => s.trim()).filter(Boolean)),
  LOG_LEVEL: z.string().default("info"),
});

export type Config = z.infer<typeof schema> & {
  /** How long a relayed request may wait for the agent's first response frame. */
  relayTimeoutMs: number;
  /** Max size of a relayed request body (one frame). */
  relayMaxBodyBytes: number;
};

export function loadConfig(env: NodeJS.ProcessEnv = process.env): Config {
  return { ...schema.parse(env), relayTimeoutMs: 30_000, relayMaxBodyBytes: 8 * 1024 * 1024 };
}
