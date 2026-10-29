# 06 Dependencies

## Tech stack (proposal)

| Part | Stack | Why |
|---|---|---|
| **CSCM agent** | Python 3.12 + Flask (existing), Docker SDK, Playwright, dnspython | Already built; add `websockets` for the connector |
| **CSCM Cloud API** | TypeScript, Node 22, Fastify or Hono, PostgreSQL 16, Drizzle ORM, `ws` for the relay | Same language as the frontends; good WebSocket support; alternative: Python FastAPI to share code with CSCM (see 09) |
| **Cloud web** | Next.js 16 + React 19 + shadcn/ui (existing CSCM frontend, API base switched) | Reuse every CSCM page |
| **Launcher** | Fork of the Modrinth App: Tauri v2, Rust (theseus core), Vue 3 | Launching, loaders, Java, `.mrpack` already solved |
| **Installers** | Inno Setup (Windows) or Tauri bundler, `.dmg` via Tauri, shell for Linux | Tauri lets the launcher double as installer/tray app |
| **Hosting proxy** | Velocity (or a custom TCP router) + wake-on-join plugin | Route `name.<domain>` to the right machine, wake sleeping servers |

## Libraries

### CSCM agent (additions)
- `websockets` — outbound relay connection
- `requests` (exists) — Modrinth API
- `zipfile`/`hashlib` (stdlib) — `.mrpack` import, sha512 checks

### Cloud
- `fastify` / `hono`, `@fastify/websocket` or `ws`
- `drizzle-orm`, `pg`
- `argon2` (passwords), `otplib` (TOTP), `jose` (JWT)
- `stripe` (billing, phase 5)
- `zod` (validation), `pino` (logs)

### Launcher (on top of the Modrinth App fork)
- existing: `tauri` 2, `tokio`, `reqwest`, `serde`, Vue 3, Pinia
- added: CSCM cloud client module (Rust or TS), keychain storage (`keyring` crate), Tauri updater with signing keys

## External APIs & services

| Service | Use | Notes / approvals |
|---|---|---|
| **Microsoft identity + Xbox Live + Minecraft services** | Player sign-in in the launcher | **Azure app must be approved by Mojang** for the Minecraft API — apply first |
| **Modrinth API v2** | Search, versions, dependencies, downloads | Open, needs a proper `User-Agent`; respect rate limits (300 req/min) |
| CurseForge API | Optional later | Needs an API key; some mods forbid third-party download |
| **Playit.gg** | Self-hosted public addresses | Existing browser automation; fragile when Playit changes UI → watch for an official API |
| **Cloudflare API** | Optional custom domains (self-hosted), our domain (hosted) | Existing |
| Mojang profile API | Name → UUID (exists for default players/bans) | Rate limited |
| **Stripe** | Paid plans | Phase 5; tax handling (Stripe Tax) |
| Docker Desktop / Docker Engine | Runtime on every machine | Desktop licence free for personal/small business |
| itzg/minecraft-server, itzg/minecraft-bedrock-server | Server images | Existing |

## Accounts / costs to set up

| Item | Approx. cost |
|---|---|
| VPS for the cloud (start) | €5–20 / month |
| Domain for hosted servers + cloud | €10–20 / year |
| Apple Developer Program (macOS signing/notarisation) | $99 / year |
| Windows code-signing certificate | €100–400 / year |
| Hosting machines (hosted tier) | e.g. Hetzner dedicated €40–100 / month each |
| Azure app registration | free (approval needed) |
