# Relay Server Deployment

The relay server helps peers connect when direct connections aren't possible (e.g., behind strict NAT). It is **stateless** and **cannot read content** — all traffic is end-to-end encrypted by iroh.

## Quick Start

```bash
RELAY_HOSTNAME=relay.yourdomain.com docker compose up -d
```

## Requirements

- A VPS with a public IP
- A domain name pointing to the VPS
- Ports 80, 443 (TCP) and 3478 (UDP) open

## Notes

- The relay automatically obtains TLS certificates via Let's Encrypt
- Certificates are persisted in a Docker volume across restarts
- For Phase 1-2 development, the N0 public relays work fine — no need to self-host yet
- Deploy 2+ instances in different regions for production redundancy
