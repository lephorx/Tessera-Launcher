# 04 Installer & setup ("do it yourself")

Goal: someone who has never heard of Docker gets a working, publicly reachable server by clicking through.

## Today (CSCM 1.9.3)

- `curl -fsSL https://lephor.com/cscm/install.sh | sh` — macOS/Linux, **needs Docker and Git already installed**; Windows only through WSL.
- First-run wizard in the dashboard: access mode → Playit account → Playit agent → custom domain (optional).
- **Playit-only already works**: without Cloudflare, players use the Playit address (e.g. `name.tun.ply.gg`); the domain step can be skipped.

## Target flow

1. **Download page** detects the OS, one button: "Install CSCM on this computer".
2. **Installer** (per OS, below) shows plain steps: "Checking your computer… Installing Docker (2/4)… Starting CSCM…", then opens the dashboard.
3. **Setup wizard** (rewritten for beginners):
   1. Create your admin account (+ authenticator app, with help).
   2. "Who should be able to join?" → Internet (Playit) / Only my network.
   3. Playit account: link to sign-up, email/password fields, ✓ check that the login works.
   4. Playit agent: CSCM creates/starts it; ✓ "Agent connected".
   5. Own domain? Big **"Skip — use the free Playit address"**, or Cloudflare with a short guide (token permissions, zone ID, domain) and the existing verification.
   6. Create your first server (preset "Vanilla for friends", 2–4 GB).
   7. ✓ Reachability test **from outside** (cloud/test service connects to the address and does a Minecraft status ping) → "Your friends can join with …" + copy button.
   8. Optional: "Connect to your CSCM account" (pairing code).
4. Every step: one task, short sentences, a screenshot or 10-second clip, a "What is this?" expander, and a link to the matching wiki page.

## Installers per OS

| OS | Package | Does |
|---|---|---|
| **Windows 10/11** | `.exe` (Inno Setup or Tauri bundle / the launcher itself) | Check virtualisation → enable WSL2 (`wsl --install`, may need restart, installer resumes after reboot) → install Docker Desktop silently (`winget` or bundled installer) → enable Docker Desktop host networking → install CSCM (compose files + `.env`) → start → autostart |
| **macOS 12+** | `.dmg` app, notarised | Install Docker Desktop (or offer OrbStack) → host networking → install CSCM → LaunchAgent for autostart |
| **Linux** | existing `install.sh`, extended | Install Docker Engine via `get.docker.com` if missing (Debian/Ubuntu/Fedora/Arch), add user to `docker` group, install CSCM, systemd unit |

Common to all:
- Detects an existing installation → offers update/repair instead.
- Generates `.env` with sane defaults (paths, port 3000 or the next free one).
- Writes an uninstall entry (the existing `uninstall.sh` logic).
- Logs to a file shown on failure with "Copy log / Open help".

## Hard spots

- **Windows**: admin rights, WSL restart, virtualisation disabled in BIOS → detect and explain with pictures.
- **Playit agent on macOS/Windows** relies on Docker Desktop host networking (4.34+). Fallback: run the native Playit agent (it has Windows/macOS builds) outside Docker, managed by the installer/tray app.
- **Code signing** needed to avoid "unknown publisher" / "can't be opened" warnings.
- **Docker Desktop licence**: free for personal use and small businesses (<250 employees, <$10M revenue).

## Launcher integration

"Host on this device" in the launcher runs the same installer steps; afterwards the launcher acts as the tray app (start/stop, autostart, open dashboard, updates) and pairs the machine to the account automatically.
