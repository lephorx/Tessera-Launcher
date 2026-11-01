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

Deploy: `sh deploy/deploy-api.sh` from `cloud/` (`--env` also rewrites api.env). Deploy the Neon
Function too (`neon deploy --env .env.mail --no-env-pull`) so both answer the same.
