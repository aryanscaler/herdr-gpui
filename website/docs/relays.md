---
title: Relays
blurb: Run your own relay, so phones reach your computer from anywhere.
status: experimental
platforms: macOS, Linux, Windows
---

# Relays

A relay joins your phone, or another Sheprd, to your computer when they have no direct path: different networks, no Tailscale, a hotel Wi-Fi in between. Everything that crosses it is end-to-end encrypted, so a relay sees only ciphertext, timing, and sizes; it can never read your chats or terminals, and it keeps nothing on disk.

> **Coming soon.** You can run a relay today. Sheprd, `sheprd-remote-control`, and the iPhone app gain relay support in a coming release; until then they keep reaching your computer [directly or over Tailscale](remote-control.md).

## Sheprd's relays and yours

Sheprd lists its own relays at [sheprd.org/relays.json](https://sheprd.org/relays.json). That list is empty for now: **no Sheprd relays are available for now**, so to use a relay, run your own. One small instance serves all your computers and phones, and you can run as many as you like, in different regions.

Your computers will then use the relays you name, and only those: with a private relay, Sheprd's relays are never contacted.

## Deploy in one click

[![Deploy to DigitalOcean](https://www.deploytodo.com/do-btn-blue.svg)](https://cloud.digitalocean.com/apps/new?repo=https://github.com/sheprd-labs/sheprd/tree/main)

The button creates a DigitalOcean App Platform app from the published image, `ghcr.io/sheprd-labs/sheprd-relay`, with nothing to build or set up. App Platform serves HTTPS but not UDP, so this relay offers only its WebSocket transport: it works from any network, but connects a little slower and does not survive a phone switching networks. For the faster QUIC transport too, use a droplet below.

Once it is running, your relay's address is the app's domain, such as `https://sheprd-relay-abc12.ondigitalocean.app`. To turn on its dashboard, add a `RELAY_DASHBOARD_TOKEN` of 24 or more characters as an encrypted environment variable in the app's settings, then open `/dashboard`.

## Deploy on a droplet

With [`doctl`](https://docs.digitalocean.com/reference/doctl/) signed in, from a checkout of the repository:

```sh
deploy/relay/create-droplet.sh fra1
```

Each region named gets a small droplet with only ports 22, 80, and 443 open, and a relay serving both QUIC and WebSockets at `https://IP-WITH-DASHES.sslip.io`, with a Let's Encrypt certificate and no DNS to set up. The dashboard's access token is in `/root/sheprd-relay-login.txt` on the droplet. Set `RELAY_DOMAIN_SUFFIX` to use your own names.

## Any host with Docker

```sh
cd deploy/relay
cp env.example .env
mkdir -p bans && touch bans/list
RELAY_DOMAIN=relay.example.org docker compose up -d
```

Point `relay.example.org` at the host first: Caddy gets its certificate on the first request. The image, `ghcr.io/sheprd-labs/sheprd-relay`, runs on amd64 and arm64.

## Keeping it safe

A relay needs no accounts and no setup to stay safe:

- Only devices a computer has paired can reach that computer; anyone else hears that it is offline.
- Connections, sessions, and bandwidth are limited per address and in total. Every limit is a `RELAY_*` setting with a safe default.
- Addresses that misbehave are banned automatically, for ten minutes, then an hour, then a day. To ban one yourself, add it to `bans/list`; the relay rereads it every 30 seconds.
- `RELAY_LISTEN_TOKEN` keeps a relay to your own computers.

The full list of settings is in [`deploy/relay/README.md`](https://github.com/sheprd-labs/sheprd/blob/main/deploy/relay/README.md), and the design in [`docs/relay-plan.md`](https://github.com/sheprd-labs/sheprd/blob/main/docs/relay-plan.md).
