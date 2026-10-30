/**
 * A stand-in for CSCM's cloud connector (docs/05-cscm-changes.md, phase 2), used by the tests
 * and runnable by hand until the real connector exists:
 *
 *   pnpm fake-agent [cloud-url]     → prints a pairing code, waits for it to be claimed, connects
 */
import WebSocket from "ws";

export interface FakeRequest {
  id: string;
  method: string;
  path: string;
  headers: Record<string, string>;
  body?: string;
}

/** Fake CSCM API. Streams `/api/servers/:id/console/stream` as three chunks. */
export function fakeCscm(ws: WebSocket, req: FakeRequest, log: FakeRequest[]) {
  log.push(req);
  const send = (f: object) => ws.send(JSON.stringify(f));
  const json = (status: number, data: unknown) =>
    send({ type: "response", id: req.id, status, headers: { "content-type": "application/json" }, body: JSON.stringify(data) });

  if (req.path === "/api/servers" && req.method === "GET") {
    return json(200, [{ id: 1, name: "Survival", type: "paper", version: "1.21.4", runtime_status: "running" }]);
  }
  if (req.path.startsWith("/api/echo")) {
    const body = req.body === undefined ? null : Buffer.from(req.body, "base64").toString("utf8");
    return json(200, { method: req.method, path: req.path, headers: req.headers, body });
  }
  if (/^\/api\/servers\/\d+\/console\/stream$/.test(req.path)) {
    send({ type: "response", id: req.id, status: 200, headers: { "content-type": "text/event-stream" }, stream: true });
    ["data: line 1\n\n", "data: line 2\n\n", "data: line 3\n\n"].forEach((data, i) =>
      setTimeout(() => {
        send({ type: "chunk", id: req.id, data });
        if (i === 2) send({ type: "end", id: req.id });
      }, 10 * (i + 1)),
    );
    return;
  }
  if (req.path === "/api/slow") return; // never answers
  if (req.path.startsWith("/api/settings")) {
    return send({ type: "error", id: req.id, status: 403, message: "remote admin not allowed" });
  }
  json(404, { error: "Not found" });
}

export function connectAgent(agentUrl: string, credential: string, log: FakeRequest[] = []) {
  const ws = new WebSocket(agentUrl, { headers: { authorization: `Bearer ${credential}` } });
  ws.on("open", () =>
    ws.send(JSON.stringify({ type: "hello", version: "2.0.0-fake", protocol: 1, servers: 1, capabilities: ["mods"] })),
  );
  ws.on("message", (data) => {
    const frame = JSON.parse(data.toString());
    if (frame.type === "request") fakeCscm(ws, frame, log);
  });
  return ws;
}

async function main() {
  const cloud = process.argv[2] ?? "http://localhost:8080";
  const start = await fetch(`${cloud}/api/pairing`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ machine_hint: "Fake agent", version: "2.0.0-fake" }),
  }).then((r) => r.json() as Promise<{ code: string; secret: string }>);
  console.log(`Pairing code: ${start.code} (claim it with POST /api/pairing/claim)`);

  for (;;) {
    await new Promise((r) => setTimeout(r, 2000));
    const res = await fetch(`${cloud}/api/pairing/exchange`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ code: start.code, secret: start.secret }),
    });
    const body = (await res.json()) as { status?: string; agent_url?: string; credential?: string; error?: string };
    if (res.status === 202) continue;
    if (!res.ok) throw new Error(`pairing failed: ${body.error}`);
    console.log(`Paired. Connecting to ${body.agent_url}`);
    const ws = connectAgent(body.agent_url!, body.credential!);
    ws.on("open", () => console.log("Connected. Relaying requests to the fake CSCM API."));
    ws.on("close", (code, reason) => console.log(`Closed: ${code} ${reason}`));
    return;
  }
}

if (import.meta.url === `file://${process.argv[1]}`) void main();
