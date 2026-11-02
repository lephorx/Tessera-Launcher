#!/bin/sh
# Builds the Arch package (tessera-launcher-<version>-1-x86_64.pkg.tar.zst) from the .deb's
# files, with nfpm in Docker:
#
#     scripts/arch-package.sh <file.deb> <version> <out dir>
#
# The .deb's binary is marked as a .deb build, which would make the launcher update itself
# with dpkg. The marker is cleared, so it recognizes the Arch package and updates through
# pacman instead (install_pacman_update in lib.rs).
set -eu
DEB=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
VERSION=$2
OUT=$3

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/root" "$OUT"
(cd "$work" && ar x "$DEB")
tar -xf "$work"/data.tar.* -C "$work/root"

python3 - "$work/root/usr/bin/tessera-launcher" <<'EOF'
import sys
path = sys.argv[1]
data = open(path, "rb").read()
old, new = b"__TAURI_BUNDLE_TYPE_VAR_DEB", b"__TAURI_BUNDLE_TYPE_VAR_UNK"
if data.count(old) != 1:
    sys.exit(f"expected one bundle-type marker in {path}, found {data.count(old)}")
open(path, "wb").write(data.replace(old, new))
EOF

cat > "$work/nfpm.yaml" <<EOF
name: tessera-launcher
arch: amd64
platform: linux
version: $VERSION
release: 1
maintainer: lephor <ramonzielke09@gmail.com>
description: Minecraft launcher for modpacks, friends and servers
homepage: https://tesseralauncher.com
license: custom
contents:
  - src: /work/root/
    dst: /
    type: tree
depends:
  - webkit2gtk-4.1
  - gtk3
EOF

docker run --rm -v "$work:/work" -v "$(cd "$OUT" && pwd):/out" goreleaser/nfpm:v2.48.0 \
  pkg -f /work/nfpm.yaml -p archlinux -t /out
ls -l "$OUT"/*.pkg.tar.zst
