import pino from "pino";
import { generate } from "otplib";
import { loadConfig } from "../src/config.js";
import { startServer, type RunningServer } from "../src/server.js";

export interface TestCloud {
  server: RunningServer;
  url: string;
  api(path: string, init?: RequestInit & { token?: string; json?: unknown }): Promise<Response>;
  register(username: string): Promise<{ token: string; user: { id: string; username: string } }>;
}

export async function startTestCloud(): Promise<TestCloud> {
  const config = loadConfig({ DATA_DIR: ":memory:", PORT: "0", PUBLIC_URL: "http://localhost:1", LOG_LEVEL: "silent" });
  const server = await startServer(config, pino({ level: "silent" }));
  const url = `http://127.0.0.1:${server.port}`;
  // PUBLIC_URL is only known after listening; the exchange endpoint uses it for agent_url.
  config.PUBLIC_URL = url;

  const api: TestCloud["api"] = (path, init = {}) => {
    const headers = new Headers(init.headers);
    if (init.token) headers.set("authorization", `Bearer ${init.token}`);
    if (init.json !== undefined) headers.set("content-type", "application/json");
    return fetch(url + path, {
      ...init,
      headers,
      body: init.json !== undefined ? JSON.stringify(init.json) : init.body,
    });
  };

  const register = async (username: string) => {
    const res = await api("/api/auth/register", {
      method: "POST",
      json: { email: `${username}@example.com`, username, password: "correct horse battery" },
    });
    if (res.status !== 201) throw new Error(`register failed: ${res.status} ${await res.text()}`);
    return res.json() as Promise<{ token: string; user: { id: string; username: string } }>;
  };

  return { server, url, api, register };
}

export function totpCode(secret: string) {
  return generate({ secret });
}

export async function waitFor(check: () => boolean | Promise<boolean>, ms = 3000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return;
    await new Promise((r) => setTimeout(r, 20));
  }
  throw new Error("waitFor timed out");
}
