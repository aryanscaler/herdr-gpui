#!/usr/bin/env bash
# Creates one relay droplet per region with doctl, each zero-config:
#
#   deploy/relay/create-droplet.sh nyc3 fra1 sgp1
#
# Needs `doctl auth init` first. Optional environment:
#   RELAY_SIZE     droplet size (default s-1vcpu-1gb)
#   RELAY_SSH_KEYS comma-separated SSH key ids or fingerprints from
#                  `doctl compute ssh-key list` (default: none, console only)
#   RELAY_DOMAIN_SUFFIX  e.g. relay.example.org gives nyc3.relay.example.org;
#                  point those names at the droplets yourself. Without it,
#                  each relay is served at IP.sslip.io.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
if [ "$#" -eq 0 ]; then
  echo "usage: $0 REGION [REGION...]" >&2
  exit 2
fi
command -v doctl >/dev/null || { echo "doctl is not installed" >&2; exit 1; }

size=${RELAY_SIZE:-s-1vcpu-1gb}
compose=$(base64 < "$here/compose.yml" | tr -d '\n')
caddyfile=$(base64 < "$here/Caddyfile" | tr -d '\n')
data=$(mktemp)
trap 'rm -f "$data"' EXIT

for region in "$@"; do
  domain=""
  if [ -n "${RELAY_DOMAIN_SUFFIX:-}" ]; then
    domain="$region.$RELAY_DOMAIN_SUFFIX"
  fi
  sed -e "s|__COMPOSE_B64__|$compose|" \
      -e "s|__CADDYFILE_B64__|$caddyfile|" \
      -e "s|__RELAY_DOMAIN__|$domain|" \
      "$here/cloud-init.yml" > "$data"
  keys=()
  if [ -n "${RELAY_SSH_KEYS:-}" ]; then
    keys=(--ssh-keys "$RELAY_SSH_KEYS")
  fi
  doctl compute droplet create "sheprd-relay-$region" \
    --region "$region" \
    --size "$size" \
    --image ubuntu-24-04-x64 \
    --enable-monitoring \
    --tag-name sheprd-relay \
    --user-data-file "$data" \
    "${keys[@]}" \
    --wait \
    --format Name,PublicIPv4,Region
done

cat <<'NOTE'

Each droplet starts its relay within a few minutes. The dashboard login is in
/root/sheprd-relay-login.txt on each droplet (`doctl compute ssh NAME`, or the
web console). Without RELAY_DOMAIN_SUFFIX the address is IP-with-dashes.sslip.io.
NOTE
