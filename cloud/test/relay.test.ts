import WebSocket from "ws";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { connectAgent, type FakeRequest } from "./fake-agent.js";
import { startTestCloud, waitFor, type TestCloud } from "./helpers.js";

let cloud: TestCloud;
beforeAll(async () => {
  cloud = await startTestCloud();
});
afterAll(() => cloud.server.close());

/** Runs the whole pairing flow and connects the fake agent. */
async function pairMachine(ownerToken: string, name = "Home PC") {
  const start = await cloud.api("/api/pairing", { method: "POST", json: { machine_hint: "lephor-pc", version: "2.0.0" } });
  expect(start.status).toBe(201);
  const { code, secret } = await start.json();
  expect(code).toMatch(/^[A-Z2-9]{3}-[A-Z2-9]{3}$/);

  const pending = await cloud.api("/api/pairing/exchange", { method: "POST", json: { code, secret } });
  expect(pending.status).toBe(202);

  // Users may type the code in lowercase and without the dash.
  const claim = await cloud.api("/api/pairing/claim", {
    method: "POST",
    token: ownerToken,
    json: { code: code.replace("-", "").toLowerCase(), name },
  });
  expect(claim.status).toBe(201);
  const machineId: string = (await claim.json()).machine.id;

  const exchange = await cloud.api("/api/pairing/exchange", { method: "POST", json: { code, secret } });
  expect(exchange.status).toBe(200);
  const paired = await exchange.json();
  expect(paired.machine_id).toBe(machineId);
  expect(paired.agent_url).toMatch(/^ws:\/\/.*\/agent$/);
  expect(paired.owner.username).toBeTypeOf("string");

  const requests: FakeRequest[] = [];
  const ws = connectAgent(paired.agent_url, paired.credential, requests);
  await new Promise((r) => ws.once("open", r));
  await waitFor(() => cloud.server.hub.isOnline(machineId));
  return { machineId, ws, requests, code, secret, credential: paired.credential as string };
}

describe("pairing and relay", () => {
  it("pairs a machine and hands out the credential only once", async () => {
    const { token } = await cloud.register("owner1");
    const { machineId, code, secret, ws } = await pairMachine(token);

    const again = await cloud.api("/api/pairing/exchange", { method: "POST", json: { code, secret } });
    expect(again.status).toBe(410);
    const wrongSecret = await cloud.api("/api/pairing/exchange", { method: "POST", json: { code, secret: "x" } });
    expect(wrongSecret.status).toBe(404);

    await waitFor(async () => {
      const list = await (await cloud.api("/api/machines", { token })).json();
      return list.machines[0]?.version === "2.0.0-fake";
    });
    const list = await (await cloud.api("/api/machines", { token })).json();
    expect(list.machines).toHaveLength(1);
    expect(list.machines[0]).toMatchObject({ id: machineId, name: "Home PC", online: true, role: "admin", paired: true });
    ws.close();
  });

  it("rejects agents with an unknown credential", async () => {
    const ws = new WebSocket(`${cloud.url.replace("http", "ws")}/agent`, { headers: { authorization: "Bearer nope" } });
    const status = await new Promise<number>((resolve) => ws.once("unexpected-response", (_req, res) => resolve(res.statusCode!)));
    expect(status).toBe(401);
  });

  it("relays requests with the caller's identity", async () => {
    const { token, user } = await cloud.register("owner2");
    const { machineId, ws, requests } = await pairMachine(token);

    const servers = await cloud.api(`/api/machines/${machineId}/servers`, { token });
    expect(servers.status).toBe(200);
    expect(servers.headers.get("x-tessera-relay")).toBe("1");
    expect((await servers.json())[0].name).toBe("Survival");

    const echo = await cloud.api(`/api/machines/${machineId}/echo/x?a=1`, {
      method: "POST",
      token,
      json: { command: "say hi" },
      headers: { cookie: "secret=1" },
    });
    const echoed = await echo.json();
    expect(echoed.path).toBe("/api/echo/x?a=1");
    expect(echoed.body).toBe('{"command":"say hi"}');
    expect(echoed.headers).toMatchObject({ "x-cscm-user": user.id, "x-cscm-username": "owner2", "x-cscm-role": "admin" });
    expect(echoed.headers.cookie).toBeUndefined();
    expect(echoed.headers.authorization).toBeUndefined();
    expect(requests.length).toBe(2);

    const refused = await cloud.api(`/api/machines/${machineId}/settings`, { token });
    expect(refused.status).toBe(403);
    expect((await refused.json()).error).toBe("agent_error");
    ws.close();
  });

  it("streams console output", async () => {
    const { token } = await cloud.register("owner3");
    const { machineId, ws } = await pairMachine(token);
    const res = await cloud.api(`/api/machines/${machineId}/servers/1/console/stream`, { token });
    expect(res.headers.get("content-type")).toBe("text/event-stream");
    expect(await res.text()).toBe("data: line 1\n\ndata: line 2\n\ndata: line 3\n\n");
    ws.close();
  });

  it("enforces membership and roles", async () => {
    const owner = await cloud.register("owner4");
    const friend = await cloud.register("friend4");
    const stranger = await cloud.register("stranger4");
    const { machineId, ws } = await pairMachine(owner.token);

    expect((await cloud.api(`/api/machines/${machineId}/servers`, { token: stranger.token })).status).toBe(404);
    expect((await cloud.api(`/api/machines/${machineId}`, { token: stranger.token })).status).toBe(404);

    const add = await cloud.api(`/api/machines/${machineId}/members`, {
      method: "POST",
      token: owner.token,
      json: { username: "friend4", role: "member" },
    });
    expect(add.status).toBe(201);

    const echo = await (await cloud.api(`/api/machines/${machineId}/echo`, { token: friend.token })).json();
    expect(echo.headers["x-cscm-role"]).toBe("member");

    // Members can't manage the machine or remove the owner.
    expect((await cloud.api(`/api/machines/${machineId}`, { method: "PATCH", token: friend.token, json: { name: "x" } })).status).toBe(403);
    expect((await cloud.api(`/api/machines/${machineId}/members/${owner.user.id}`, { method: "DELETE", token: owner.token })).status).toBe(400);

    const members = await (await cloud.api(`/api/machines/${machineId}/members`, { token: friend.token })).json();
    expect(members.members.map((m: { username: string }) => m.username)).toEqual(["friend4", "owner4"]);
    ws.close();
  });

  it("returns 503 when the machine is offline", async () => {
    const { token } = await cloud.register("owner5");
    const { machineId, ws } = await pairMachine(token);
    ws.close();
    await waitFor(() => !cloud.server.hub.isOnline(machineId));
    const res = await cloud.api(`/api/machines/${machineId}/servers`, { token });
    expect(res.status).toBe(503);
    expect((await res.json()).error).toBe("machine_offline");
  });

  it("removing a machine revokes the credential and closes the socket", async () => {
    const { token } = await cloud.register("owner6");
    const { machineId, ws, credential } = await pairMachine(token);
    const closed = new Promise<number>((r) => ws.once("close", (code) => r(code)));

    expect((await cloud.api(`/api/machines/${machineId}`, { method: "DELETE", token })).status).toBe(200);
    expect(await closed).toBe(4001);

    const retry = new WebSocket(`${cloud.url.replace("http", "ws")}/agent`, { headers: { authorization: `Bearer ${credential}` } });
    const status = await new Promise<number>((resolve) => retry.once("unexpected-response", (_req, res) => resolve(res.statusCode!)));
    expect(status).toBe(401);
  });

  it("fails pending requests when the agent drops", async () => {
    const { token } = await cloud.register("owner7");
    const { machineId, ws } = await pairMachine(token);
    const pending = cloud.api(`/api/machines/${machineId}/slow`, { token });
    await new Promise((r) => setTimeout(r, 50));
    ws.terminate();
    const res = await pending;
    expect(res.status).toBe(502);
    expect((await res.json()).error).toBe("agent_disconnected");
  });
});
