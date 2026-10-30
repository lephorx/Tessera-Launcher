# 08 Roadmap

Each phase is useful on its own. Self-hosted CSCM never requires a later phase.

## Phase 0 — Decisions & approvals
- [ ] Decide the open questions in [09](09-open-questions.md) (name, licence, cloud stack)
- [ ] Register the Azure app and apply for Minecraft API access (long lead time)
- [ ] Add a licence to CSCM

## Phase 1 — Mods & modpacks in CSCM
- [ ] Modrinth search + install with dependencies on the server page
- [ ] Installed mods list with updates and removal
- [ ] Import `.mrpack` / create server from pack (server-side files only)
- [ ] Export pack
**Done when:** a Fabric server can be created from a Modrinth pack in a few clicks.

## Phase 2 — Cloud MVP (self-hosted machines)
- [x] Cloud: accounts (email + 2FA), machines, pairing codes
- [x] Cloud: relay hub (agent WebSocket, request/stream frames), tested with a fake agent
- [x] Agent connector: outbound WebSocket, relay executor, identity mapping, audit log (CSCM branch `cloud-connector`)
- [ ] Cloud web dashboard: pick a machine → existing CSCM pages through the relay
- [ ] Console and file transfers through the relay
**Done when:** a CSCM on a home PC behind NAT is fully manageable from the cloud dashboard.

## Phase 3 — Do-it-yourself installer & setup
- [ ] Linux script installs Docker, systemd
- [ ] Beginner wizard with checks and outside reachability test
- [ ] Windows installer (WSL2 + Docker Desktop + CSCM)
- [ ] macOS installer
**Done when:** a non-technical tester gets a reachable server on Windows without help.

## Phase 4 — Launcher
- [ ] Fork + rebrand the Modrinth App, remove ads/telemetry
- [ ] Tessera UI: tokens, Modern/Classic switch, shared components, screenshot check against the dashboard ([10](10-ui-design.md))
- [ ] Microsoft sign-in with our approved app ID
- [ ] CSCM module: account, machines, servers, server pages
- [ ] Pack ↔ server sync, "Join server" invites
- [ ] "Host on this device" (installer inside the launcher, tray mode)
**Done when:** a friend clicks an invite, the launcher installs the pack and joins the server.

## Phase 5 — Hosted tier
- [ ] Hosted mode in the agent: limits, sleep/wake, no Playit
- [ ] Proxy with hostname routing + wake on join
- [ ] Placement, plans, Stripe billing, legal pages
- [ ] Invite-only beta with a few machines
**Done when:** paying beta users run servers on our machines.

## Phase 6 — Sharing & community
- [ ] Public packs, pack pages, versions and changelogs
- [ ] Pack updates propagated to servers and players
- [ ] Optional CurseForge import
