# 02 Architecture

## Overview

```
                    ┌──────────────────────────────────────────┐
                    │               CSCM Cloud                 │
                    │  accounts · machines · permissions       │
                    │  modpacks · sharing · plans & billing    │
                    │  relay (WebSocket hub)                   │
                    └───▲──────────────▲───────────────▲───────┘
                        │ HTTPS        │ HTTPS         │ WSS (outbound from agents)
                 ┌──────┴─────┐  ┌─────┴──────┐        │
                 │ Web        │  │ Launcher   │        │
                 │ dashboard  │  │ (desktop)  │        │
                 └────────────┘  └────────────┘        │
          ┌────────────────────────────┬───────────────┴─────────────┐
          │                            │                             │
 ┌────────┴────────┐        ┌──────────┴────────┐        ┌───────────┴──────────┐
 │ CSCM (agent)    │        │ CSCM (agent)      │        │ CSCM (agent) × N     │
 │ Alice's PC      │        │ Bob's home server │        │ our hosting machines │
 │ Docker, Playit  │        │ Docker, Playit,   │        │ Docker, our proxy,   │
 │                 │        │ Cloudflare        │        │ quotas               │
 └─────────────────┘        └───────────────────┘        └──────────────────────┘
```

## Components

### CSCM (agent) — existing repo `lephorx/CSCM`
- Unchanged role: runs Minecraft containers, Playit tunnels, DNS, backups, files, players.
- New: an optional **cloud connector** that keeps one outbound WebSocket to the cloud and executes relayed API calls against its own local API.
- Keeps its local dashboard and local accounts. Works fully offline / without the cloud.

### CSCM Cloud — new
- **Accounts:** sign-up/sign-in (email + password + 2FA, optionally "Sign in with Microsoft").
- **Machines:** each paired CSCM install, owner, members and their role per machine, online status, version.
- **Relay:** forwards API requests from dashboard/launcher to the right machine over its WebSocket and streams responses (incl. console log streams).
- **Modpacks & sharing:** stored pack definitions (`.mrpack`-compatible), visibility, invites.
- **Hosted tier:** plans, quotas, placement on hosting machines, billing (see 07).

### Web dashboard
- Reuse of the CSCM Next.js frontend: the same pages, but the API base becomes `https://cloud/api/machines/<id>/…` instead of the local `/api`.
- Plus cloud pages: machines, account, packs, billing.

### Launcher — see 03
- Desktop app; talks only to the cloud API (and to Modrinth/Mojang for game files).

## Connecting a machine (pairing)

1. In CSCM (local dashboard → Settings → Cloud) click **Connect to CSCM account**. CSCM requests a pairing code from the cloud: `POST /pairing` → `{ code: "KX7-42P", expires_in: 600 }`, holding a one-time secret.
2. The user enters the code at `cloud/machines/add` (or the launcher).
3. The cloud binds the machine to the account and returns a **machine credential** (long-lived, revocable) to CSCM, which stores it in its DB.
4. CSCM opens `wss://cloud/agent` with that credential and keeps it open (reconnect with backoff).
5. Removing the machine in the cloud revokes the credential and closes the socket.

No port forwarding, no public HTTPS on the machine: the connection is outbound only.

## Relay protocol (draft)

Frames over the agent WebSocket (JSON, later msgpack):

```jsonc
// cloud → agent
{ "id": "r1", "type": "request", "method": "POST", "path": "/api/servers/3/start",
  "headers": { "x-cscm-user": "<cloud user id>", "x-cscm-role": "admin" }, "body": "..." }
// agent → cloud
{ "id": "r1", "type": "response", "status": 200, "headers": {...}, "body": "..." }
// streams (console, progress): chunked
{ "id": "r2", "type": "chunk", "data": "..." }   { "id": "r2", "type": "end" }
// keep-alive / status
{ "type": "hello", "version": "2.0.0", "servers": 4, "capabilities": ["mods", "packs"] }
```

The agent only accepts paths under `/api/` and maps the cloud user to a local identity (see below). File uploads/downloads go through the same channel in chunks (size limit per frame).

## Identity & permissions

- **Cloud user** = a person. **Machine membership** = (cloud user, machine, role).
- Roles per machine mirror CSCM today: `admin` (everything) and `member` (own servers only).
- The agent trusts the cloud's statement of who is calling, so CSCM needs a mapping table `cloud_users(cloud_user_id → local user id)`; first use creates a local user with the role from the cloud.
- Local accounts keep working for local access.

## Security

- Whoever controls the cloud can control every connected machine. Therefore:
  - Machine credentials: random 256-bit, stored hashed in the cloud, revocable, rotated.
  - Agent-side allowlist of API routes reachable through the cloud; dangerous ones (CSCM self-update, settings with secrets) need local opt-in ("Allow remote admin").
  - Audit log on the agent of every relayed request (user, route, time), visible locally.
  - Optional: commands signed by the cloud with a key whose public part is pinned in the agent at pairing time.
- Secrets (Playit password, Cloudflare token) never leave the machine through the relay (GET /settings keeps masking them).
- TLS everywhere; rate limits per machine and per user.

## Data model (cloud, draft)

```
users(id, email, password_hash, totp_secret, microsoft_id, created_at)
machines(id, owner_id, name, credential_hash, version, last_seen, kind: self_hosted|hosted)
machine_members(machine_id, user_id, role)
pairing_codes(code, secret_hash, expires_at, machine_hint)
packs(id, owner_id, name, mc_version, loader, visibility: private|link|public, current_version_id)
pack_versions(id, pack_id, mrpack_index_json, created_at)
pack_shares(pack_id, user_id)
servers_index(machine_id, server_id, name, pack_version_id)   -- cache for listing
plans / subscriptions / usage                                   -- hosted tier, see 07
```

## Where things run

- Cloud: one VPS to start (Docker Compose: API, Postgres, relay, web). Later separate relay nodes.
- Hosted machines: dedicated servers (e.g. Hetzner) running the normal CSCM agent in "hosted mode".
