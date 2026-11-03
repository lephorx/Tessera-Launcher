# The API on the home server

`api.tesseralauncher.com` is the Tessera API running on `s-ubutsra01` (ssh host `tessera-srv`),
reached only through Cloudflare Tunnel: no open ports, the server's address stays hidden. Same code
as the Neon Function (`functions/api/index.ts`) and the same database and bucket; the Neon
Function stays deployed as the fallback the launcher (0.6.26+) switches to when the tunnel is down,
and for older launchers that still call it directly.

- `server.mjs`: `functions/api/server.ts` bundled with esbuild, in `/opt/tessera-api`
- `tessera-api.service`: systemd, user `tessera`, listens on `127.0.0.1:8787` only
- `/etc/tessera/api.env` (root:tessera 640): Neon's branch env from `.env.local` plus `.env.mail`,
  `BEHIND_CLOUDFLARE=1` (trust CF-Connecting-IP) and `PORT`
- `cloudflared.service`: tunnel `tessera-api`, config in `/etc/cloudflared/config.yml`

## Nightly API

`api-nightly.tesseralauncher.com` runs main's newest API for Tessera Launcher Nightly, next to the
released one: same database, bucket and accounts (schema changes still go through drizzle
migrations by hand), port 8788, the same tunnel.

- `tessera-api-nightly.service`: like `tessera-api.service`, from `/opt/tessera-api-nightly`, env
  `/etc/tessera/api-nightly.env` (api.env with `PORT=8788`)
- `tessera-api-nightly-update.timer`: every 5 minutes `tessera-api-nightly-update.sh` downloads
  `api-nightly.mjs` from the `nightly` GitHub release (built by `.github/workflows/nightly-api.yml`
  on cloud pushes to main) and restarts the service when it changed
- One-time setup: `sh deploy/setup-api-nightly.sh` (rerun after `deploy-api.sh --env` to copy the env)

## Deploying the released API

Deploy: `sh deploy/deploy-api.sh` from `cloud/` (`--env` also rewrites api.env). Deploy the Neon
Function too (`neon deploy --env .env.mail --no-env-pull`) so both answer the same.
