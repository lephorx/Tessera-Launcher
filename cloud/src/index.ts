import { existsSync } from "node:fs";
import { loadConfig } from "./config.js";
import { startServer } from "./server.js";

if (existsSync(".env")) process.loadEnvFile(".env");

const running = await startServer(loadConfig());

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.once(signal, () => {
    void running.close().then(() => process.exit(0));
  });
}
