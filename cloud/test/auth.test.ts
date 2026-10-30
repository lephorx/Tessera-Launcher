import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { startTestCloud, totpCode, type TestCloud } from "./helpers.js";

let cloud: TestCloud;
beforeAll(async () => {
  cloud = await startTestCloud();
});
afterAll(() => cloud.server.close());

describe("accounts", () => {
  it("registers, reads /me and signs out", async () => {
    const { token, user } = await cloud.register("alice");
    expect(user.username).toBe("alice");

    const me = await cloud.api("/api/auth/me", { token });
    expect(me.status).toBe(200);
    expect((await me.json()).user.email).toBe("alice@example.com");

    expect((await cloud.api("/api/auth/logout", { method: "POST", token })).status).toBe(200);
    expect((await cloud.api("/api/auth/me", { token })).status).toBe(401);
  });

  it("rejects duplicates and weak input", async () => {
    await cloud.register("bob");
    const dup = await cloud.api("/api/auth/register", {
      method: "POST",
      json: { email: "BOB@example.com", username: "bobby", password: "correct horse battery" },
    });
    expect(dup.status).toBe(409);
    expect((await dup.json()).error).toBe("email_taken");

    const weak = await cloud.api("/api/auth/register", {
      method: "POST",
      json: { email: "c@example.com", username: "carol", password: "short" },
    });
    expect(weak.status).toBe(400);
  });

  it("signs in with email and password", async () => {
    await cloud.register("dave");
    const wrong = await cloud.api("/api/auth/login", {
      method: "POST",
      json: { email: "dave@example.com", password: "nope nope nope" },
    });
    expect(wrong.status).toBe(401);

    const ok = await cloud.api("/api/auth/login", {
      method: "POST",
      json: { email: "Dave@Example.com", password: "correct horse battery" },
    });
    expect(ok.status).toBe(200);
    expect(ok.headers.get("set-cookie")).toContain("tessera_session=");
  });

  it("enables two-factor and then requires it", async () => {
    const { token } = await cloud.register("erin");
    const setup = await (await cloud.api("/api/auth/totp/setup", { method: "POST", token })).json();
    expect(setup.otpauth_url).toMatch(/^otpauth:\/\/totp\/Tessera/);

    const bad = await cloud.api("/api/auth/totp/enable", { method: "POST", token, json: { code: "000000" } });
    expect(bad.status).toBe(400);
    const good = await cloud.api("/api/auth/totp/enable", {
      method: "POST",
      token,
      json: { code: await totpCode(setup.secret) },
    });
    expect(good.status).toBe(200);

    const creds = { email: "erin@example.com", password: "correct horse battery" };
    const noCode = await cloud.api("/api/auth/login", { method: "POST", json: creds });
    expect((await noCode.json()).error).toBe("totp_required");
    const withCode = await cloud.api("/api/auth/login", {
      method: "POST",
      json: { ...creds, totp: await totpCode(setup.secret) },
    });
    expect(withCode.status).toBe(200);
  });

  it("accepts the session cookie but checks Origin on writes", async () => {
    const { token } = await cloud.register("frank");
    const cookie = `tessera_session=${token}`;
    expect((await cloud.api("/api/auth/me", { headers: { cookie } })).status).toBe(200);

    const evil = await cloud.api("/api/auth/totp/setup", { method: "POST", headers: { cookie, origin: "https://evil.example" } });
    expect(evil.status).toBe(403);
    const same = await cloud.api("/api/auth/totp/setup", { method: "POST", headers: { cookie, origin: cloud.url } });
    expect(same.status).toBe(200);
  });
});

describe("rate limits", () => {
  it("limits sign-in attempts per address", async () => {
    const statuses: number[] = [];
    for (let i = 0; i < 12; i++) {
      const res = await cloud.api("/api/auth/login", { method: "POST", json: { email: "nobody@example.com", password: "x" } });
      statuses.push(res.status);
    }
    // earlier tests in this file used some attempts already: 401s, then 429 from then on
    const first = statuses.indexOf(429);
    expect(first).toBeGreaterThan(0);
    expect(statuses.slice(0, first).every((s) => s === 401)).toBe(true);
    expect(statuses.slice(first).every((s) => s === 429)).toBe(true);
  });
});
