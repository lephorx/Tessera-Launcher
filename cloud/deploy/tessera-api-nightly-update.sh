#!/bin/sh
# Installs the newest nightly API build when it changed: GitHub Actions bundles main's API into
# the Tessera-Launcher "nightly" release (nightly-api.yml). Run by tessera-api-nightly-update.timer.
set -eu
URL=https://github.com/lephorx/Tessera-Launcher/releases/download/nightly/api-nightly.mjs
DIR=/opt/tessera-api-nightly
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
curl -fsSL --max-time 120 -o "$tmp" "$URL"
[ -s "$tmp" ] || exit 0
if ! cmp -s "$tmp" "$DIR/server.mjs"; then
  install -m 644 -o root -g root "$tmp" "$DIR/server.mjs"
  systemctl restart tessera-api-nightly
  echo "installed nightly API $(sha256sum "$DIR/server.mjs" | cut -c1-12)"
fi
