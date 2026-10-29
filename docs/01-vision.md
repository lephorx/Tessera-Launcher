# 01 Vision

## In one sentence

A launcher where you play modded Minecraft with your friends, and the server for it is either on your own computer (free) or hosted by us (paid), managed from the same place.

## Who it is for

| Person | Wants |
|---|---|
| **Player** | Join a friend's modded server without setting up mods by hand. |
| **Server owner, technical** | Run servers on their own hardware, manage them from anywhere, keep control. |
| **Server owner, not technical** | "I want a server for me and my friends." No ports, no Docker knowledge. |
| **Pack creator** | Build a modpack once, share it, have servers and players stay in sync. |

## Two ways to get a server

| | Do it yourself (free) | Hosted (paid) |
|---|---|---|
| Where it runs | Your own PC or server | Our machines |
| Setup | One installer + setup wizard | Click "Create server" |
| Public address | Your Playit account (free address), optionally your own domain via Cloudflare | `name.<our domain>` |
| Limits | Your hardware | The plan (RAM, CPU, disk) |
| Cost | Free | Monthly plan |
| Works without our cloud | **Yes, always** | No |

Both run the same CSCM software. A self-hosted CSCM never depends on the cloud: the account only adds remote access, the launcher integration and sharing.

## User journeys

### A. Self-host, beginner
1. On the website: "Host it yourself" → the page detects Windows/macOS/Linux → download.
2. The installer installs Docker and CSCM, shows progress, opens the dashboard.
3. The setup wizard: create the admin account → Playit account → Playit agent (CSCM runs it) → custom domain? "Skip, use the free Playit address".
4. A final check: "Your server can be reached from the internet ✓".
5. Optional: "Connect to your CSCM account" (pairing code) to manage it from the launcher and the web.

### B. Hosted
1. Sign in → "Create server" → pick a modpack or vanilla/Paper → pick a plan.
2. Server is created on our machines, gets `name.<our domain>`, sleeps when empty and wakes when someone joins.
3. Upgrade the plan any time.

### C. Play with friends (launcher)
1. Friend sends a link / invite to the server.
2. The launcher shows "Join <server>": installs the exact modpack (client mods only), adds the server, launches.
3. When the owner updates the pack, the launcher updates before the next start.

### D. Pack creator
1. Builds a pack in the launcher (Modrinth search) or imports a `.mrpack`.
2. "Create server from this pack": CSCM installs only the mods the server needs.
3. Shares the pack publicly or with selected users.

## Principles

- Self-hosting is a full product, not a demo of the paid one.
- One codebase for self-hosted and hosted machines.
- Beginners never see Docker, ports or YAML unless they want to.
- Modrinth first (open API, `.mrpack`), CurseForge later if at all.
