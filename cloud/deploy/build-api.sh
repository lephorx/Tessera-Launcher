#!/bin/sh
# Bundles the API (functions/api/server.ts) into one file: dist-api/server.mjs, or the path given.
# Used by deploy-api.sh and the nightly API build (.github/workflows/nightly-api.yml).
set -e
cd "$(dirname "$0")/.."
OUT=${1:-dist-api/server.mjs}
ESBUILD=$(node -e "console.log(require.resolve('esbuild/bin/esbuild', { paths: [require.resolve('tsx')] }))")
"$ESBUILD" functions/api/server.ts --bundle --platform=node --format=esm --target=node22 --external:pg-native \
  --outfile="$OUT" "--banner:js=import { createRequire as __cr } from 'module'; const require = __cr(import.meta.url);" --log-level=warning
