# 03 Launcher

## What it does

| Area | Features |
|---|---|
| **Play** | Microsoft sign-in, instances (version + loader + mods), launch, logs, Java managed automatically |
| **Modpacks** | Search Modrinth, add mods with dependencies, versions/updates, import/export `.mrpack` |
| **Servers** | List servers from all paired machines and hosted ones, start/stop, console, players, files — the CSCM dashboard inside the app |
| **Create** | "Create server" from a pack: on one of your machines, or hosted (paid) |
| **Sync** | Pack ↔ server: server-side mods installed on the server, client-side mods in the instance, updates both together |
| **Share** | Share a pack (link, user, public), "Join server" invites that install the pack and add the server |
| **Host on this device** | Install CSCM locally from the launcher (see 04) — the launcher becomes the tray app |

## Base: fork the Modrinth App

| Option | Pros | Cons |
|---|---|---|
| **Fork Modrinth App** (Tauri v2, Rust "theseus" core, Vue 3 UI) — **recommended** | Launching, Java, loaders, `.mrpack`, Modrinth search, MS auth flow already done and maintained | GPL-3.0 → our launcher must be GPL-3.0 and open source; must remove Modrinth branding/ads/telemetry; keep up with upstream |
| Fork Prism Launcher (C++/Qt) | Very mature, many loaders | C++/Qt, different stack from CSCM, GPL-3.0 |
| Own Tauri app + launch libraries | Full control, any licence | Months of work on launching alone; many edge cases (Forge/NeoForge installers, natives, Java versions) |

Decision proposal: fork the Modrinth App, keep its core for launching and content, add a **CSCM module** (cloud client + server pages + sync) and rebrand. Re-evaluate if upstream changes make the fork too costly.

## Mod sync — how client/server split works

- Every Modrinth project has `client_side` and `server_side`: `required | optional | unsupported | unknown`.
- `.mrpack` files list each file with `env: { client, server }` using the same values.
- Rule:
  - Server installs files where `server ∈ {required, optional}`.
  - Client installs files where `client ∈ {required, optional}`.
  - `unknown` → ask the pack author once, remember per pack.
- Config files: `overrides/` (both), `server-overrides/` (server only), `client-overrides/` (client only) — standard `.mrpack` folders.
- Version check on join: the server reports its pack version; the launcher updates the instance before connecting.

## Pages (draft)

```
Home         recent instances, "Join <friend's server>" invites, news
Library      instances (play, edit mods, export)
Discover     Modrinth search (mods, packs, shaders, resource packs)
Servers      all servers across machines → server page (CSCM dashboard features)
Machines     paired machines, "Add machine" (code), "Host on this device"
Packs        my packs, shared with me, publish
Account      Microsoft account(s), CSCM account, plan & billing
```

## Microsoft / Minecraft sign-in

- Uses Microsoft OAuth → Xbox Live → XSTS → Minecraft services token.
- **New Azure app IDs must be approved by Mojang** to use the Minecraft services API (review form). The Modrinth App's client ID belongs to Modrinth and cannot be reused in a fork → apply early, plan for a wait or a rejection.
- Tokens stored in the OS keychain.

## Talking to CSCM

- Launcher → CSCM Cloud API only (never directly to machines, except optionally on the LAN for speed).
- Same REST routes as CSCM's `openapi.yaml`, prefixed with the machine: `/api/machines/<id>/servers/...`.
- Console/progress streams via the relay's streaming frames.

## Branding & distribution

- Name: **Tessera Launcher** (see 09). The UI matches the Tessera dashboard exactly, Modern and Classic looks included: see [10 UI design](10-ui-design.md).
- Auto-updates via Tauri updater with signed releases.
- Code signing: Apple Developer ID (notarisation) and a Windows code-signing certificate, otherwise users see security warnings.
