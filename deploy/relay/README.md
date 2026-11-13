# Running a Sheprd relay

`sheprd-relay` joins Sheprd peers that have no direct path. It carries only
end-to-end encrypted bytes, keeps no state, and needs no settings.
`docs/relay-plan.md` has the design.

## The image

CI (`.github/workflows/relay-image.yml`) builds the image natively for amd64
and arm64, checks that each starts and answers, and on every change to `main`
publishes one multi-architecture image, signed with cosign and with build
provenance:

```text
ghcr.io/sheprd-labs/sheprd-relay:latest
ghcr.io/sheprd-labs/sheprd-relay:sha-COMMIT
```

Docker picks the right architecture, so the same name works on regular and
ARM droplets. Before the first deploy, make the package public once (GitHub >
the organization > Packages > sheprd-relay > Package settings > Change
visibility), or droplets and App Platform cannot pull it without
credentials.

A tag can be pushed again; a digest cannot. To check an image by hand,
verify its signature against the exact workflow that signs it, on `main`
(the workflow's summary prints this for each image it publishes):

```sh
cosign verify ghcr.io/sheprd-labs/sheprd-relay:latest \
  --certificate-identity https://github.com/sheprd-labs/sheprd/.github/workflows/relay-image.yml@refs/heads/main \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  | jq -r '.[0].critical.image."docker-manifest-digest"'
```

It prints the `sha256:` digest that signature covers. Deploy
`ghcr.io/sheprd-labs/sheprd-relay@sha256:…` with that digest, never the
tag you verified: the tag may move between the check and the pull.
Droplets do this on their own (below); App Platform cannot.

`scripts/relay-image.sh amd64` builds and checks it locally, as CI does.

### Pinned images

Everything else the relay is built or served with is pinned by digest, with
its tag kept beside it for people:

| Image | Where | Kept current by |
| --- | --- | --- |
| `caddy:2-alpine@sha256:…` | `compose.yml` | Dependabot (`docker-compose`) |
| `moby/buildkit:v0.34.2@sha256:…` | `.github/workflows/relay-image.yml` | by hand: Dependabot does not read `run:` lines |
| cosign v2.6.5, by SHA-256 per architecture | `cloud-init.yml`'s `start.sh` | by hand, from the release's signed `cosign_checksums.txt`, in step with the version `relay-image.yml` signs with |
| `gcr.io/distroless/static-debian12:nonroot@sha256:…` | `crates/sheprd-relay/Dockerfile` | Dependabot (`docker`) |
| `rust:1.99.0-alpine3.22@sha256:…` | `crates/sheprd-relay/Dockerfile`, the `RUST_IMAGE` default | by hand, with the toolchain: Dependabot does not read `ARG` defaults |

To move cosign, take the two hashes from the release's checksums after
checking that file's own signature:

```sh
v=v2.6.5
base=https://github.com/sigstore/cosign/releases/download/$v
curl -fsSLO "$base/cosign_checksums.txt" \
  -O "$base/cosign_checksums.txt-keyless.pem" -O "$base/cosign_checksums.txt-keyless.sig"
cosign verify-blob cosign_checksums.txt \
  --certificate cosign_checksums.txt-keyless.pem --signature cosign_checksums.txt-keyless.sig \
  --certificate-identity keyless@projectsigstore.iam.gserviceaccount.com \
  --certificate-oidc-issuer https://accounts.google.com
grep -E ' cosign-linux-(amd64|arm64)$' cosign_checksums.txt
```

## DigitalOcean droplets, one command (QUIC and WebSockets)

```sh
doctl auth init
deploy/relay/create-droplet.sh nyc3 fra1 sgp1
```

Each region gets a `s-1vcpu-1gb` droplet tagged `sheprd-relay`, with only
ports 22, 80, and 443 open. A few minutes later the relay answers at
`https://IP-WITH-DASHES.sslip.io` (for example `203-0-113-7.sslip.io`), with a
Let's Encrypt certificate. The dashboard's access token is in
`/root/sheprd-relay-login.txt` on the droplet.

To use your own names, set `RELAY_DOMAIN_SUFFIX=relay.example.org`, then point
`nyc3.relay.example.org` and the rest at the droplets. Set `RELAY_SSH_KEYS` to
your key ids from `doctl compute ssh-key list` to reach them over SSH.

### What a droplet runs

`/opt/sheprd-relay/start.sh`, which the user data writes and runs at first
boot, never runs a tag:

1. It installs cosign v2.6.5 as `/usr/local/bin/cosign-v2.6.5`, and only if
   the download matches the SHA-256 pinned for the droplet's architecture.
2. It runs `cosign verify` on `ghcr.io/sheprd-labs/sheprd-relay:latest`,
   accepting only a keyless signature whose certificate names
   `https://github.com/sheprd-labs/sheprd/.github/workflows/relay-image.yml@refs/heads/main`,
   issued by `https://token.actions.githubusercontent.com`.
3. It writes the digest that signature covers to `compose.env` as
   `RELAY_IMAGE=ghcr.io/sheprd-labs/sheprd-relay@sha256:…` and starts
   compose with it.

If the signature is missing or from anyone else, or names more than one
digest, `start.sh` changes nothing, so the relay already running keeps
running, and exits non-zero with `sheprd-relay: … the running relay is
unchanged`. At first boot that message is in `/var/log/cloud-init-output.log`.

To run another tag, verified the same way, or a digest you verified
yourself, set `RELAY_IMAGE` (a digest, `…@sha256:…`, is used as given):

```sh
RELAY_IMAGE=ghcr.io/sheprd-labs/sheprd-relay:sha-0123456789ab /opt/sheprd-relay/start.sh
RELAY_IMAGE=ghcr.io/sheprd-labs/sheprd-relay@sha256:… /opt/sheprd-relay/start.sh
```

## DigitalOcean App Platform (WebSockets only)

No server to manage, in one click:

[![Deploy to DigitalOcean](https://www.deploytodo.com/do-btn-blue.svg)](https://cloud.digitalocean.com/apps/new?repo=https://github.com/sheprd-labs/sheprd/tree/main)

The button reads `.do/deploy.template.yaml`. Or, with `doctl`:

```sh
doctl apps create --spec deploy/relay/app.yaml
```

App Platform carries HTTP but not UDP, so the relay runs there without QUIC,
and clients use its WebSocket fallback: slower to connect and with no
connection migration, but it works from any network. The spec wires up what
the platform provides (below); add `RELAY_DASHBOARD_TOKEN` as an encrypted
variable in the app's settings for the dashboard.

**App Platform deploys are not verified.** App Platform pulls the image
itself and cannot run cosign, so both specs name `tag: latest` and run
whatever that tag points at when the app deploys. To pin, verify a digest by
hand (above), then in the spec replace `tag: latest` with
`digest: sha256:…` (App Platform accepts one or the other, not both) and
apply it with `doctl apps update APP_ID --spec deploy/relay/app.yaml`. A
pinned app stays on that digest until you change it again. The Deploy
button always starts from `.do/deploy.template.yaml` on `main`, so it is
unverified unless that file is pinned; keep it and `app.yaml` the same.

## Ports and names from outside

A container cannot see which ports the host publishes it on, so the relay is
told what the outside looks like. Each deploy sets these for you:

| Variable | What it says | Droplet (compose) | App Platform |
| --- | --- | --- | --- |
| `PORT` | The HTTP port to listen on, when the platform picks it; `RELAY_LISTEN` wins | not set (8080) | set by the platform |
| `RELAY_QUIC_PORT` | The UDP port clients reach QUIC on, when a mapping changes it | `443` (published to the container's 8443) | QUIC is off |
| `RELAY_QUIC_IDENTITY_DIR` | Where QUIC's certificate is kept, so its pin survives restarts and can be listed in Sheprd's signed relay list | `/quic`, from `/opt/sheprd-relay/quic` | QUIC is off |
| `RELAY_PUBLIC_HOST` | The name clients use, which WebSocket proofs must cover | `RELAY_DOMAIN` | `${APP_DOMAIN}` |
| `RELAY_BEHIND_PROXY` | Take callers' addresses from the last entry of the last `X-Forwarded-For` line; only behind a proxy that appends to that header or replaces it, as Caddy does | `1` (Caddy) | not set |
| `RELAY_CLIENT_IP_HEADER` | Take them from this header instead | not set | `do-connecting-ip` |

Callers' real addresses matter: limits are per address, so behind a proxy
without either setting, every caller would share the proxy's.

## Any host with Docker

```sh
cd deploy/relay
cp env.example .env    # set RELAY_DASHBOARD_TOKEN for the dashboard
mkdir -p bans && touch bans/list   # ranges to ban by hand, one per line
RELAY_DOMAIN=relay.example.org \
  RELAY_IMAGE=ghcr.io/sheprd-labs/sheprd-relay@sha256:… docker compose up -d
```

Set `RELAY_IMAGE` to a digest you verified (see "The image"). Without it,
compose pulls `:latest` unverified.

Or just the image, with no proxy and no TLS, for a trial:

```sh
docker run --rm -p 8080:8080 -p 8443:8443/udp \
  -e RELAY_DASHBOARD_TOKEN="$(openssl rand -base64 32)" \
  ghcr.io/sheprd-labs/sheprd-relay:latest
```

The dashboard cookie is `Secure`, so outside `localhost` sign in over HTTPS.

That trial serves QUIC only. The WebSocket fallback, for networks that block
UDP, needs the relay's public name, which proofs are checked against:
`-e RELAY_PUBLIC_HOST=relay.example.org`. The compose file sets it from
`RELAY_DOMAIN`.

## Limits

Defaults suit a 1 GB droplet. Change them in `.env`; `sheprd-relay --help`
lists every one.

| Variable | Default |
| --- | --- |
| `RELAY_MAX_CONNECTIONS` | 10000 |
| `RELAY_MAX_CONNECTIONS_PER_IP` | 64 |
| `RELAY_MAX_LISTENERS` | 5000 |
| `RELAY_MAX_SESSIONS` | 5000 |
| `RELAY_MAX_SESSIONS_PER_ROUTE` | 32 |
| `RELAY_SESSION_BANDWIDTH` | 4MiB (per second, each way) |
| `RELAY_TOTAL_BANDWIDTH` | 0 (unlimited) |
| `RELAY_IDLE_TIMEOUT` | 5m |
| `RELAY_MAX_SESSION_DURATION` | 24h |
| `RELAY_LISTEN_TOKEN` | unset: any computer may listen |

## Who may use it

There is nothing to authorize. Each computer tells the relay which of its
paired devices may dial it, and only while it is connected; everyone else
hears "offline". When a computer disconnects, its keys go with it, and the
relay keeps nothing on disk.

To keep a relay to your own computers, you may set `RELAY_LISTEN_TOKEN`
(16+ characters) and give it to them. It is off by default, and dialers
never need it. To give each person or machine its own, so one can be
revoked alone, put them in a file instead, one per line, and point
`RELAY_LISTEN_TOKEN_FILE` at it:

```text
# NAME: TOKEN, or the token alone
alice-laptop: 0a1b2c...   # openssl rand -base64 32
bob-desktop: 9f8e7d...
```

The relay re-reads it every 30 seconds; delete a line and that token's
computers stop listening within half a minute. Emptying or deleting the
file revokes every token. A file it cannot parse (a token under 16
characters, say) is ignored and the last good one kept.

## Toxic clients

Addresses that misbehave are banned automatically, with nothing to set up:
hitting limits again and again, forged proofs, malformed messages, probing,
or sending more than four times `RELAY_MAX_TRANSFER_PER_HOUR` (20 GiB) in
an hour; past one budget, an address's sessions are only slowed. Bans last
10 minutes, then an hour, then a day, and show on the dashboard. A ban for
hitting limits alone refuses new connections but keeps the address's open
sessions and listeners, since a household or a carrier's NAT can hit them
together. Wrong dashboard tokens block that address from the dashboard for
15 minutes, never from the relay. Private and loopback addresses are never
banned automatically.

To ban someone by hand, without a restart, list their addresses or ranges
in a file and point the relay at it:

```sh
echo '203.0.113.0/24' >> /opt/sheprd-relay/bans/list    # takes effect within 30 s
```

`deploy/relay`'s compose file mounts the `./bans` directory read-only and
sets `RELAY_BAN_FILE=/bans/list`, and the droplet's user data creates it.
Elsewhere, mount a directory and point `RELAY_BAN_FILE` at a file in it, or
set `RELAY_BAN=203.0.113.7,2001:db8::/32` and restart. `RELAY_AUTO_BAN=off`
turns the automatic bans off.

## Listing a relay as one of Sheprd's

Sheprd's own relays are whatever `https://sheprd.org/relays.json` lists,
from `website/site/relays.json` in this repository. Clients take the list
only when it is signed by the maintainer's OpenPGP key or the site
workflow's Ed25519 key (`docs/relay-plan.md`), so never edit it by hand.
Once a relay answers `https://DOMAIN/v1/info`:

```sh
just relays-add https://fra1.relay.sheprd.org fra1 Frankfurt   # reads its QUIC pin, signs (a key tap)
just relays-remove https://fra1.relay.sheprd.org               # retire one
just relays-sign                                               # re-sign before the list expires (a year)
```

Each one stamps a new serial and expiry, signs with `gpg`, and checks the
result as clients will. Compare the QUIC pin `relays-add` prints with the
`pin=` the relay logged at start. Commit `relays.json` and
`relays.json.asc` together; the site deploy adds `relays.json.sig`.

Remove an entry to retire a relay, or empty the list to take them all out
of service: Sheprd then shows "No Sheprd relays are available for now", and
computers stay reachable on their own networks and private relays. Clients
re-read the list every hour and keep the last good copy. The endpoint must
be `https`; the site test and the relay crate's tests check the file.

## Using your own relay from Sheprd

Once the daemon side ships (`docs/relay-plan.md`, phase 3), add the relay in
Settings > Remote control > Relays, or in the config:

```toml
[remote]
relays = ["https://relay.example.org"]   # yours only; add "default" to keep ours too
```

Any of `relay.example.org`, `https://relay.example.org:8443`, or
`wss://example.org/relay` (behind a proxy that strips `/relay`) works. Plain
`http://` is accepted only for a relay on this computer or a private network.
Sheprd checks `https://DOMAIN/v1/info` first and asks for the listen token if
the relay requires one. Paired phones learn the relay from the computer.

## Monitoring

- `/metrics` is on port 9090, published on the droplet's `127.0.0.1` only.
  Scrape it from the droplet, through `ssh -L 9090:127.0.0.1:9090`, or over
  a private network with `RELAY_METRICS_TOKEN` set and the port published
  there.
- `https://DOMAIN/dashboard` shows sessions, listeners, throughput, and
  refusals, live and read-only, when `RELAY_DASHBOARD_TOKEN` is set. Sign in
  with the token; three wrong tries block that address for 15 minutes.
- `https://DOMAIN/healthz` answers `ok` for an uptime check.

Logs hold no addresses, routes, or payloads.

## Updating

On a droplet, run the start script again: it verifies `:latest` and moves
to its digest only if the signature checks out (see "What a droplet runs").

```sh
/opt/sheprd-relay/start.sh
```

Run compose there only with `--env-file compose.env`, which holds the digest
last verified; without it, compose falls back to `:latest`, unverified.

Restarting drops open sessions. Computers listen again within seconds and
clients reconnect; nothing is lost because nothing is stored.
