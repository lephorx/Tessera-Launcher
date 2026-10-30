# Tessera Cloud

Phase 2 of the [roadmap](../docs/08-roadmap.md): accounts, machines, pairing, and the relay that lets you manage a CSCM install behind NAT from anywhere. Design: [02 Architecture](../docs/02-architecture.md).

Stack: Node 22+, TypeScript, Hono, Drizzle ORM, PostgreSQL (embedded PGlite when `DATABASE_URL` is empty), `ws`.

## Run

```sh
pnpm install
cp cloud/.env.example cloud/.env   # optional
pnpm dev                           # http://localhost:8080, PGlite in cloud/.data
pnpm test
```

Until CSCM has its cloud connector, a fake agent stands in for it:

```sh
pnpm --filter @tessera/cloud fake-agent http://localhost:8080
# prints a pairing code; claim it with POST /api/pairing/claim, then call /api/machines/<id>/servers
```

After changing `src/db/schema.ts`: `pnpm --filter @tessera/cloud db:generate`. Migrations run on start.

## API

Auth: `Authorization: Bearer <token>` (launcher) or the `tessera_session` cookie (web; writes need an allowed `Origin`). Errors are `{ "error": "<code>", "message"?: "…" }`.

| Method | Path | |
|---|---|---|
| POST | `/api/auth/register` | `{ email, username, password }` → `{ token, user }` |
| POST | `/api/auth/login` | `{ email, password, totp? }` → `{ token, user }`; `401 totp_required` when 2FA is on |
| POST | `/api/auth/logout`, `/api/auth/logout-all` | |
| GET | `/api/auth/me` | |
| POST | `/api/auth/totp/setup` → `/totp/enable { code }`, `/totp/disable { password, code }` | Authenticator app |
| POST | `/api/pairing` | **Agent**, no auth: `{ machine_hint?, version? }` → `{ code: "KX7-42P", secret, expires_in: 600 }` |
| POST | `/api/pairing/claim` | **User**: `{ code, name? }` → `{ machine }` |
| POST | `/api/pairing/exchange` | **Agent** polls `{ code, secret }`: `202 pending` → `200 { machine_id, credential, agent_url, owner: { id, username } }` exactly once |
| GET | `/api/machines` | Machines you belong to, with `role`, `online`, `version` |
| GET/PATCH/DELETE | `/api/machines/:id` | Rename (admin); delete = owner removes and revokes, others leave |
| GET/POST | `/api/machines/:id/members` | `{ username, role: "admin" \| "member" }` |
| PATCH/DELETE | `/api/machines/:id/members/:userId` | |
| any | `/api/machines/:id/<rest>` | **Relayed** to the agent's `/api/<rest>` (CSCM's `openapi.yaml`) |

## Agent connection

The agent opens `GET /agent` as a WebSocket with `Authorization: Bearer <credential>` and keeps it open (reconnect with backoff; the cloud pings every 25 s and drops agents that miss a ping). A newer connection from the same machine replaces the old one; close code `4001` means the machine was removed, so stop reconnecting.

Frames are JSON, defined in [`src/relay/protocol.ts`](src/relay/protocol.ts):

```jsonc
// cloud → agent
{ "type": "welcome", "protocol": 1, "machine_id": "…" }
{ "type": "request", "id": "r1", "method": "POST", "path": "/api/servers/3/start",
  "headers": { "content-type": "…", "x-cscm-user": "<cloud user id>", "x-cscm-username": "alex", "x-cscm-role": "admin" },
  "body": "<base64>", "encoding": "base64" }
{ "type": "cancel", "id": "r1" }                       // caller went away, stop streaming
// agent → cloud
{ "type": "hello", "version": "2.0.0", "protocol": 1, "servers": 4, "capabilities": ["mods"] }
{ "type": "response", "id": "r1", "status": 200, "headers": {…}, "body": "…", "encoding": "utf8" }
{ "type": "response", "id": "r2", "status": 200, "headers": {…}, "stream": true }  // then:
{ "type": "chunk", "id": "r2", "data": "…" }   { "type": "end", "id": "r2" }
{ "type": "error", "id": "r3", "status": 403, "message": "remote admin not allowed" }
```

The cloud only forwards `accept`, `accept-language`, `content-type`, `if-none-match`, `last-event-id` and `range`, never cookies or its own `Authorization`. The agent must accept `x-cscm-*` headers **only** from this socket, apply its route allowlist, and answer within 30 s (the first frame; streams can run longer).

## Not done yet

- Credential rotation (needs a `rotate` frame), signed commands, per-user rate limits on the relay
- TOTP replay protection (a code works for its whole window), email verification, password reset
- "Sign in with Microsoft"
- The hub is in memory, so one cloud instance only
- Deployment (Dockerfile / Compose)
