#!/usr/bin/env bash
# Builds the relay's container image for one architecture, checks that it
# starts and answers, and with --push publishes it by digest and prints the
# digest. CI runs it once per architecture on native runners; it runs the
# same way locally:
#
#   scripts/relay-image.sh amd64                     # build and check
#   scripts/relay-image.sh arm64 ghcr.io/o/sheprd-relay --push
#
# A registry IMAGE also turns on its layer cache (IMAGE:buildcache-ARCH).
set -euo pipefail

arch=${1:?usage: relay-image.sh amd64|arm64 [IMAGE] [--push]}
image=${2:-sheprd-relay}
push=${3:-}
case $arch in amd64 | arm64) ;; *) echo "unknown architecture: $arch" >&2; exit 2 ;; esac

build=(--platform "linux/$arch" -f crates/sheprd-relay/Dockerfile --provenance=false)
# A mirror of the Rust base image, when Docker Hub is out of reach.
if [[ -n ${RUST_IMAGE:-} ]]; then
  build+=(--build-arg "RUST_IMAGE=$RUST_IMAGE")
fi
if [[ $image == */* ]]; then
  cache="type=registry,ref=$image:buildcache-$arch"
  build+=(--cache-from "$cache")
  if [[ $push == --push ]]; then
    build+=(--cache-to "$cache,mode=max")
  fi
fi

check="$image:check-$arch"
docker buildx build "${build[@]}" --load -t "$check" . >&2

# The image runs as the deploy files run it: read-only, no capabilities.
docker run --rm "$check" --help >/dev/null
container=$(docker run -d --read-only --cap-drop ALL \
  -e RELAY_PUBLIC_HOST=relay.test -p 127.0.0.1::8080 "$check")
trap 'docker rm -f "$container" >/dev/null' EXIT
port=$(docker port "$container" 8080/tcp | head -n 1 | sed 's/.*://')
for _ in $(seq 1 30); do
  curl -fsS "http://127.0.0.1:$port/healthz" >/dev/null 2>&1 && break
  sleep 1
done
curl -fsS "http://127.0.0.1:$port/v1/info" \
  | jq -e '.protocol == 1 and .websocket and .quic.port > 0' >/dev/null
echo "$arch image starts and answers" >&2

if [[ $push == --push ]]; then
  metadata=$(mktemp)
  # Every layer is cached from the build above, so this only pushes.
  docker buildx build "${build[@]}" --metadata-file "$metadata" \
    --output "type=image,name=$image,push-by-digest=true,name-canonical=true,push=true" . >&2
  jq -r '."containerimage.digest"' "$metadata"
fi
