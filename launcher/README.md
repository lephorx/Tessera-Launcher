# Tessera Launcher

Desktop app: build instances, add mods from Modrinth, launch Minecraft, and manage your CSCM servers. Tauri v2 with a Rust core and a Vue 3 + TypeScript UI, written for Tessera (no Modrinth App code). UI spec: [10 UI design](../docs/10-ui-design.md).

## Run

Needs Node 22+, pnpm and Rust (`rustup`, stable).

```sh
pnpm install
pnpm --filter @tessera/launcher dev     # opens the app with hot reload
pnpm --filter @tessera/launcher test    # Rust unit tests
# real download + launch of Fabric 1.21.1 (opens a game window for a moment):
cd launcher/src-tauri && cargo test --release -- --ignored --nocapture --test-threads 1 smoke_
```

Data lives in the app data dir (macOS: `~/Library/Application Support/dev.tessera.launcher`): `instances/`, shared `meta/` (versions, libraries, assets, Java), `settings.json`, `servers.json`.

## What works

- **Create instance:** search, *Custom setup* (Vanilla, Fabric, NeoForge, Forge, Quilt; any game version; loader version Stable/Latest/pick one), *Start from a modpack*, *Upload a modpack* (.mrpack), *Import instance* (Prism Launcher, MultiMC, CurseForge; found automatically or from a launcher path).
- **Icons:** every instance gets a random one; the icon editor mixes 12 gradients with 25 pixel blocks drawn in code, or takes an uploaded image. Modpacks use their own icon.
- **Play:** Java, libraries and assets download automatically; Forge/NeoForge installers are processed by the launcher itself (no installer window). Live logs, stop. Offline player name until Microsoft sign-in is approved.
- **Discover:** Modpacks, Mods, Resource Packs, Data Packs, Shaders from Modrinth with sort, page size and pages. Installing a modpack creates an instance; anything else goes into a compatible instance you pick (datapacks into a world). Mods bring their required dependencies.
- **Content tab:** everything in the instance (also files added by hand or imported), turn on/off, remove.
- **Tessera packs (.tapack):** *Export* on an instance page saves a pack; *Upload a modpack* opens .tapack and .mrpack. See below.
- **Servers → Host a server:** *Rent* (coming soon) or *Do it yourself*: on this computer (Docker check), another machine over LAN/Tailscale, or an existing CSCM. Start/stop its servers.

Real launches are tested with `smoke_*` (Vanilla, Fabric, Quilt, NeoForge 1.21.1, Forge 1.20.1); `TESSERA_SMOKE_GAME=1.20.1` picks the vanilla version.

## The .tapack format

A zip file:

```text
tessera.pack.json   manifest (below)
overrides/…         copied into the game folder as is: configs, options.txt, files Modrinth doesn't know
icon.png            only when the instance icon is an uploaded image
```

```jsonc
{
  "format": 1,                       // readers refuse newer formats
  "name": "Friends SMP", "version": "1.2.0", "author": "lephor", "summary": "",
  "minecraft": "1.21.1",
  "loader": { "type": "fabric", "version": "0.16.9" },   // vanilla | fabric | quilt | forge | neoforge
  "icon": { "kind": "generated", "background": "rose", "symbol": "creeper" },  // or { "kind": "file" }
  "memory_mb": null,
  "content": [{
    "kind": "mod", "path": "mods/fabric-api-0.116.jar", "enabled": true,
    "url": "https://cdn.modrinth.com/…", "sha1": "…", "size": 2304120,
    "title": "Fabric API", "version": "0.116.0+1.21.1", "icon_url": "…",
    "source": { "type": "modrinth", "project_id": "P7dR8mSH", "version_id": "…" },
    "env": { "client": "required", "server": "required" }    // for syncing packs to servers
  }]
}
```

On export, every file in `mods/`, `resourcepacks/` and `shaderpacks/` is looked up on Modrinth by SHA-1 (also files added by hand); known ones become `content` links, the rest go into `overrides/`. On import, links are downloaded and hash-checked, and override paths may not leave the game folder. Logs, crash reports and caches are never exported.

## Code

| | |
|---|---|
| `src-tauri/src/core/` | Launch engine: `versions` (Mojang manifest, rules), `download`, `java`, `assets`, `loaders` (versions, Fabric/Quilt profiles), `forge` (Forge/NeoForge installer processing), `launch` (classpath, arguments), `instances`, `modrinth`, `mrpack`, `tapack`, `import`, `servers` (CSCM over HTTP) |
| `src-tauri/src/lib.rs` | Tauri commands and events (`launch-progress`, `instance-state`, `instance-log`) |
| `src/` | Vue app: `pages/`, `components/`, `lib/api.ts` (typed commands), `styles/` (tokens + Modern/Classic) |
