# 05 Changes to CSCM (`lephorx/CSCM`)

Everything here keeps CSCM fully usable on its own. Grouped by phase (see 08).

## Phase 1 — Mods & modpacks in CSCM (useful without cloud or launcher)

| Change | Where |
|---|---|
| Modrinth client: search, project/version lookup, dependency resolution, download with hash check | new `backend/modrinth.py` |
| Installed-content tracking: which jar came from which project/version, updates available | new table `server_mods(server_id, project_id, version_id, file, sha512, side)`; `schema.sql`, `db.py` migration |
| Mod endpoints: search, install (with deps), remove, update, list | new `backend/routes/mods.py`, `openapi.yaml` |
| Loader/version compatibility checks (server `type` + `version` vs. mod versions) | `modrinth.py` |
| Packs: import `.mrpack`, create server from pack (server-side files only + `overrides/` + `server-overrides/`), export | new `backend/packs.py`, `routes/packs.py`; `server_manager.provision_server` gains `pack` |
| Pack version on a server (for later launcher sync) | `servers.pack_id`, `servers.pack_version` columns |
| UI: "Mods" tab on the server page, "Create from modpack" in the create dialog | `frontend/app/servers/[serverId]/page.tsx`, new `components/ModsPanel.tsx`, `ServerCreateModal.tsx` |

## Phase 2 — Cloud connector (agent)

| Change | Where |
|---|---|
| Pairing: request code, store machine credential | new `backend/cloud_connector.py`, table `cloud_link(machine_id, credential, cloud_url, paired_at)` |
| Outbound WebSocket with reconnect/backoff, heartbeat, `hello` frame (version, capabilities) | `cloud_connector.py` (thread or asyncio task started in `app.py`) |
| Relay executor: run relayed requests against the local Flask app (`app.test_client()` or internal WSGI call), stream responses | `cloud_connector.py` |
| Identity mapping cloud user → local user (+ role) | table `cloud_users`, `auth_helpers.authorize()` accepts relayed identity only from the connector |
| Route allowlist + "Allow remote admin" toggle for sensitive routes (update, settings with secrets, users) | `cloud_connector.py`, `routes/settings.py` |
| Audit log of relayed requests | table `audit_log`, page in Settings |
| Streaming for console (`/console/stream`) and progress through the relay | `routes/console.py` refactor to a generator usable by both SSE and relay |
| Chunked file upload/download through the relay | `routes/files.py` |
| Settings → Cloud tab: connect / disconnect / status | `frontend/app/settings/page.tsx`, new `components/CloudPanel.tsx` |
| Frontend API base configurable (local `/api` vs. cloud `/api/machines/<id>`) so the cloud dashboard can reuse the pages | `frontend/lib/api.ts` (`API_BASE`), `app/api/[...path]/route.ts` |

## Phase 3 — Installer & setup

| Change | Where |
|---|---|
| `install.sh` installs Docker if missing, systemd unit, non-interactive mode for the GUI installers | `scripts/install.sh` |
| Beginner wizard rewrite: one task per screen, checks after each step, first-server preset, outside reachability test | `frontend/components/SetupWizard.tsx`, new reachability endpoint (calls a cloud test service) |
| Native Playit agent fallback (outside Docker) for macOS/Windows | `settings_manager.py` agent handling |
| Health/info endpoint for installers and the tray app | `routes/system.py` |

## Phase 4 — Hosted mode

| Change | Where |
|---|---|
| `CSCM_MODE=hosted`: no Playit, no Cloudflare; servers get `name.<our domain>` through our proxy | `server_manager.py`, `settings_manager.py` |
| Resource limits per server: memory (exists), CPU quota, disk quota, network limits | `docker_manager.py` (`nano_cpus`, storage-opt / quota on the data volume) |
| Sleep when empty / wake on join, coordinated with the proxy | new `sleep_manager.py`, player count via RCON/query |
| Cloud is the only source of users/ownership in hosted mode (local login disabled) | `auth_helpers.py`, `app.py` |
| Usage reporting to the cloud (RAM-hours, disk) | `cloud_connector.py` |

## Cross-cutting

- **Versioning:** CSCM 2.0 when the connector lands; the cloud checks agent `version`/`capabilities`.
- **API stability:** `openapi.yaml` becomes the contract for cloud + launcher; breaking changes need a version prefix.
- **Licence:** decide before outside users install it (see 09); add `LICENSE`.
- **Tests:** CI with the backend smoke tests that exist only ad hoc today.
