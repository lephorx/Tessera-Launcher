# CSCM Launcher & Cloud

Concept for the next stage of [CSCM](https://github.com/lephorx/CSCM): one account for players and server owners, a Minecraft launcher with modpacks that sync to your servers, and two ways to get a server:

- **Do it yourself (free):** install CSCM on your own computer with one installer and a step-by-step setup, Playit only or with your own domain.
- **Hosted (paid):** click "Create server" and get one on our machines, with a small plan to start and more performance when you need it.

> Status: **concept**. Nothing here is built yet. CSCM itself is at 1.9.3.

## Documents

| | |
|---|---|
| [01 Vision](docs/01-vision.md) | What it is, for whom, the two options, user journeys |
| [02 Architecture](docs/02-architecture.md) | CSCM Cloud, agents on each machine, pairing, security |
| [03 Launcher](docs/03-launcher.md) | The desktop app: play, modpacks, server sync, hosting on this device |
| [04 Installer & setup](docs/04-installer-and-setup.md) | One-click install per OS, beginner setup wizard, Playit-only |
| [05 Changes to CSCM](docs/05-cscm-changes.md) | Everything that has to change in the CSCM repo |
| [06 Dependencies](docs/06-dependencies.md) | Tech stack, libraries, external APIs and services, approvals |
| [07 Hosted tier](docs/07-hosted-tier.md) | Plans, sleeping servers, routing, abuse, billing, legal |
| [08 Roadmap](docs/08-roadmap.md) | Phases and milestones |
| [09 Open questions](docs/09-open-questions.md) | Decisions to make before building |

## Repositories

| Repo | What |
|---|---|
| `lephorx/CSCM` | The server manager (agent + local dashboard). Stays usable on its own. |
| `lephorx/CSCM-Launcher` | This repo: concept now, later the launcher (and possibly the cloud). |
