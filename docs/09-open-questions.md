# 09 Open questions

| # | Question | Options | Leaning |
|---|---|---|---|
| 1 | Product / launcher name | keep "CSCM", new brand | **Decided 2026-10-30: Tessera** (Latin for one mosaic tile: every mod is a tile, the pack is the mosaic). Indigo accent (`#5b5bf0`, light `#7b7bff`), three-tile cube mark; assets in CSCM `brand/`. Line-up: Tessera (server app), Tessera Launcher, Tessera Cloud, Tessera Hosting. Some Minecraft mods are also called Tessera, so use "Tessera Launcher" / a qualifier in store and search listings. |
| 2 | Licence of CSCM | AGPL-3.0 (free to self-host, hosts must share changes), source-available (e.g. BSL/FSL), proprietary | AGPL-3.0 |
| 3 | Licence of the launcher | GPL-3.0 is forced if we fork the Modrinth App | **Decided 2026-10-30:** no fork. Own app on the same stack (Tauri v2, Rust, Vue 3), no Modrinth code; licence still open |
| 4 | Cloud backend language | TypeScript (Fastify/Hono) vs. Python (FastAPI, shares code with CSCM) | TypeScript |
| 5 | Cloud sign-in | email + password + TOTP, "Sign in with Microsoft", both | Both; Microsoft makes the launcher seamless |
| 6 | Free hosted tier? | none (self-host is free), small sleeping tier, trial only | None at start; trial later |
| 7 | Where the cloud runs | single VPS, managed platform | Single VPS (Hetzner) to start |
| 8 | Bedrock in the hosted tier | yes (needs UDP routing), Java only | Java only at start |
| 9 | Relay vs. direct connection | always relay, direct on LAN when possible | Relay always, LAN shortcut later |
| 10 | Does the cloud dashboard reuse the CSCM frontend or get its own app? | reuse with switchable API base, separate app | Reuse |
| 11 | Repo layout | launcher + cloud in this repo (monorepo), separate repos | **Decided 2026-10-30:** monorepo here (pnpm workspace): `cloud/` now, the launcher fork later |
| 12 | Playit long-term | keep browser automation, wait for official API, replace with own tunnel for self-hosters | Keep; watch for an API |
