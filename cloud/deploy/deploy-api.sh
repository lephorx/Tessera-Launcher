#!/bin/sh
# Builds the API into one file and ships it to the home server (ssh host "tessera-srv").
# Run from cloud/: sh deploy/deploy-api.sh   (add --env to also upload /etc/tessera/api.env)
set -e
cd "$(dirname "$0")/.."
ESBUILD=$(node -e "console.log(require.resolve('esbuild/bin/esbuild', { paths: [require.resolve('tsx')] }))")
"$ESBUILD" functions/api/server.ts --bundle --platform=node --format=esm --target=node22 --external:pg-native \
  --outfile=dist-api/server.mjs "--banner:js=import { createRequire as __cr } from 'module'; const require = __cr(import.meta.url);" --log-level=warning
if [ "$1" = "--env" ]; then
  # Neon's branch env (DATABASE_URL, bucket keys, Neon Auth URLs) + the mail secrets
  { grep -hE '^(DATABASE_URL|NEON_AUTH_BASE_URL|NEON_AUTH_JWKS_URL|AWS_ACCESS_KEY_ID|AWS_SECRET_ACCESS_KEY|AWS_ENDPOINT_URL_S3|AWS_REGION)=' .env.local
    grep -hE '^(SMTP_HOST|SMTP_PORT|SMTP_USER|SMTP_PASS|MAIL_FROM|MAIL_SECRET|TESSERA_SITE_SECRET)=' .env.mail
    echo "BEHIND_CLOUDFLARE=1"; echo "PORT=8787"; } | ssh tessera-srv 'sudo install -d -m 750 -o root -g tessera /etc/tessera && sudo tee /etc/tessera/api.env >/dev/null && sudo chown root:tessera /etc/tessera/api.env && sudo chmod 640 /etc/tessera/api.env'
fi
scp -q dist-api/server.mjs deploy/tessera-api.service tessera-srv:/tmp/
ssh tessera-srv 'sudo install -m 644 -o root -g root /tmp/server.mjs /opt/tessera-api/server.mjs && sudo install -m 644 /tmp/tessera-api.service /etc/systemd/system/tessera-api.service && rm /tmp/server.mjs /tmp/tessera-api.service && sudo systemctl daemon-reload && sudo systemctl enable -q tessera-api && sudo systemctl restart tessera-api && sleep 2 && systemctl is-active tessera-api && curl -s -o /dev/null -w "local check %{http_code}\n" 127.0.0.1:8787/v1/me/email'
