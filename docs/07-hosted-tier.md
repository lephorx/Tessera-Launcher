# 07 Hosted tier (paid)

## Plans (draft, numbers to validate)

| Plan | RAM | CPU | Disk | Servers | Sleep when empty |
|---|---|---|---|---|---|
| Starter | 2 GB | 1 vCPU share | 10 GB | 1 | after 10 min |
| Standard | 4 GB | 2 vCPU | 25 GB | 2 | after 30 min |
| Plus | 8 GB | 3 vCPU | 50 GB | 3 | optional |
| Always-on add-on | — | — | — | — | no sleep |

A free hosted tier only works with aggressive sleeping and limits; start with **self-host = free option** and paid hosting only (see 09).

## How it works

- Hosting machines run the normal CSCM agent with `CSCM_MODE=hosted`.
- **Placement:** the cloud picks a machine with enough free RAM/disk for the plan; servers can be moved (stop → copy data → start elsewhere).
- **Routing:** players join `name.<our domain>`. A Velocity proxy (or custom TCP router) reads the hostname from the handshake and forwards to the right machine/port. Bedrock (UDP) needs per-server ports or a separate approach.
- **Sleep / wake:** the agent stops a server with no players for N minutes. The proxy answers status pings for sleeping servers ("Sleeping — join to start") and starts the server on a join attempt; players get "Starting, try again in ~30 s" (or are held in a lobby).
- **Limits:** memory limit (exists), CPU quota (`nano_cpus`), disk quota per server (XFS project quotas or a size-limited volume), outbound network rate limits, no privileged containers.
- **Backups:** daily, kept N days, restorable from the dashboard (existing backup system + off-machine copy).

## Abuse & safety

- E-mail verification, one free/trial server per payment method or verified account.
- Detect crypto mining (sustained 100 % CPU), outbound flood; automatic suspension + review.
- DDoS: proxy in front, provider DDoS protection; hosting machines never expose servers directly.

## Billing

- Stripe Checkout + Customer Portal; monthly; prorated upgrades; suspension on failed payment after grace period; data kept 14 days after cancellation.
- Usage metering only if pay-per-use plans are added later.

## Legal (Germany / EU) — checklist

- Impressum, AGB (terms), Datenschutzerklärung (GDPR privacy policy), AV-Vertrag where needed
- Withdrawal right (Widerrufsbelehrung) for consumer contracts
- VAT / Kleinunternehmerregelung decision, invoices
- Minecraft EULA & commercial usage guidelines: hosting is allowed; no selling of in-game advantages; "Minecraft" not in the product name
- Content moderation contact and abuse reporting
