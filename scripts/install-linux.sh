#!/bin/sh
# Installs Tessera Launcher on Linux with the package for your distribution:
#
#     curl -fsSL https://raw.githubusercontent.com/lephorx/Tessera-Launcher/main/scripts/install-linux.sh | sh
#
# Asks which distribution you use (it suggests the one it detects; TESSERA_DISTRO=<number>
# answers for you) and installs the newest release's .deb, .rpm or Arch package, or the
# AppImage for everything else. The launcher keeps itself up to date afterwards.
set -eu

REPO=lephorx/Tessera-Launcher
ANSWER=

say() { printf '%s\n' "$*" >&2; }
fail() { say "Error: $*"; exit 1; }

ask() {
  # no terminal to ask in (e.g. piped in a script): take the suggestion
  if ! (: </dev/tty) 2>/dev/null; then ANSWER=$2; say "$1: $2"; return; fi
  printf '%s [%s]: ' "$1" "$2" >/dev/tty
  IFS= read -r ANSWER </dev/tty || ANSWER=
  [ -n "$ANSWER" ] || ANSWER=$2
}

# Root commands: directly as root, otherwise through sudo.
as_root() {
  if [ "$(id -u)" -eq 0 ]; then "$@"; else
    command -v sudo >/dev/null 2>&1 || fail "sudo is required (or run this as root)."
    sudo "$@"
  fi
}

os_release() { (. /etc/os-release 2>/dev/null; eval "printf '%s' \"\${$1:-}\""); }

# The distribution this looks like, as a number of the menu below.
detect_distro() {
  ids=" $(os_release ID) $(os_release ID_LIKE) "
  case "$ids" in
    *" debian "*|*" ubuntu "*) echo 1 ;;
    *" rhel "*|*" centos "*) echo 3 ;;
    *" fedora "*) echo 2 ;;
    *opensuse*|*" suse "*|*" sles "*) echo 4 ;;
    *" arch "*) echo 5 ;;
    *) echo 6 ;;
  esac
}

# The download URL of the newest release's file whose name ends in $1.
asset_url() {
  curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | grep -o "\"browser_download_url\": *\"[^\"]*$1\"" \
    | sed 's/.*"\(https[^"]*\)"$/\1/' | head -n 1
}

# Downloads the newest file ending in $1 into $tmp and prints its path.
fetch() {
  url=$(asset_url "$1") || true
  [ -n "$url" ] || fail "Couldn't find the download. Check the internet connection, or get it from https://github.com/$REPO/releases/latest"
  file=$tmp/${url##*/}
  say "Downloading ${url##*/}…"
  curl -fsSL --retry 3 -o "$file" "$url" || fail "The download failed. Check the internet connection and retry."
  chmod 644 "$file"
  printf '%s' "$file"
}

# The AppImage in ~/.local/bin with a menu entry; it updates itself in place.
install_appimage() {
  bin=$HOME/.local/bin
  apps=${XDG_DATA_HOME:-$HOME/.local/share}/applications
  icons=${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/128x128/apps
  mkdir -p "$bin" "$apps" "$icons"
  file=$(fetch .AppImage)
  curl -fsSL -o "$icons/tessera-launcher.png" "https://raw.githubusercontent.com/$REPO/main/launcher/src-tauri/icons/128x128.png" || true
  mv "$file" "$bin/tessera-launcher.AppImage"
  chmod 755 "$bin/tessera-launcher.AppImage"
  cat > "$apps/tessera-launcher.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Tessera Launcher
Comment=Minecraft launcher for modpacks, friends and servers
Exec=$bin/tessera-launcher.AppImage
Icon=tessera-launcher
Categories=Game;
StartupWMClass=tessera-launcher
Terminal=false
EOF
  say ""
  say "Installed to $bin/tessera-launcher.AppImage, with an entry in your app menu."
  if ! ls /usr/lib*/libfuse.so.2 /usr/lib/*/libfuse.so.2 /lib*/libfuse.so.2 /lib/*/libfuse.so.2 >/dev/null 2>&1; then
    say "AppImages need FUSE 2: install libfuse2 (Debian/Ubuntu: libfuse2t64 or libfuse2, Fedora/RHEL: fuse-libs)."
  fi
}

[ "$(uname -s)" = Linux ] || fail "This installer is for Linux. Get Tessera Launcher for your system at https://tesseralauncher.com/download"
[ "$(uname -m)" = x86_64 ] || fail "Tessera Launcher for Linux needs a 64-bit Intel or AMD processor (x86_64) for now."
command -v curl >/dev/null 2>&1 || fail "curl is required."

DETECTED=$(detect_distro)
say "Tessera Launcher setup (Linux)"
say ""
say "Which Linux do you use?"
say "  1) Debian, Ubuntu, Linux Mint, Pop!_OS, Zorin and other Debian-based (.deb)"
say "  2) Fedora (.rpm)"
say "  3) RHEL, Rocky Linux, AlmaLinux 10 (.rpm, adds EPEL; version 9 gets the AppImage)"
say "  4) openSUSE (.rpm)"
say "  5) Arch, Manjaro, EndeavourOS, CachyOS (.pkg.tar.zst)"
say "  6) Something else: the AppImage, for your user only"
ask "Number" "${TESSERA_DISTRO:-$DETECTED}"

tmp=$(mktemp -d)
chmod 755 "$tmp"
trap 'rm -rf "$tmp"' EXIT

case "$ANSWER" in
  1)
    file=$(fetch .deb)
    say "Installing it (this asks for your password)…"
    as_root apt-get update
    as_root apt-get install -y "$file"
    ;;
  2)
    file=$(fetch .rpm)
    say "Installing it (this asks for your password)…"
    as_root dnf install -y "$file"
    ;;
  3)
    major=$(os_release VERSION_ID | cut -d. -f1)
    if [ "${major:-0}" -lt 10 ]; then
      say "Version ${major:-?} has no WebKitGTK 4.1, which the package needs: installing the AppImage instead."
      install_appimage
    else
      # WebKitGTK 4.1 comes from EPEL
      if ! rpm -q epel-release >/dev/null 2>&1; then
        say "Adding EPEL (Extra Packages for Enterprise Linux)…"
        if [ "$(os_release ID)" = rhel ]; then
          as_root dnf install -y "https://dl.fedoraproject.org/pub/epel/epel-release-latest-$major.noarch.rpm"
        else
          as_root dnf install -y epel-release
        fi
      fi
      file=$(fetch .rpm)
      say "Installing it (this asks for your password)…"
      as_root dnf install -y "$file"
    fi
    ;;
  4)
    file=$(fetch .rpm)
    say "Installing it (this asks for your password)…"
    as_root zypper --non-interactive install --allow-unsigned-rpm "$file"
    ;;
  5)
    file=$(fetch .pkg.tar.zst)
    say "Installing it (this asks for your password)…"
    as_root pacman -U --needed --noconfirm "$file"
    ;;
  6) install_appimage ;;
  *) fail "Please choose 1 to 6." ;;
esac

say ""
say "Tessera Launcher is installed: open it from your app menu. It updates itself."
