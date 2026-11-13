# The relay as a static binary on a distroless image: no shell, no package
# manager, a non-root user. Build from the repository root, which the
# workspace's members and patches need:
#
#   docker buildx build -f crates/sheprd-relay/Dockerfile \
#     --platform linux/amd64,linux/arm64 -t sheprd-relay .
#
# Both base images are pinned by digest, the tag kept for reading, so a
# moved tag cannot change what is built. Dependabot proposes new digests
# for the distroless image; move the Rust one by hand with the toolchain
# (`docker buildx imagetools inspect rust:TAG`).

ARG RUST_IMAGE=rust:1.99.0-alpine3.22@sha256:c3a5ad77ff2e5ec99fffaf62cb518e61a9dcbd9e1d7526d281a380db0be3bb3b

FROM ${RUST_IMAGE} AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p sheprd-relay \
 && cp target/release/sheprd-relay /sheprd-relay

FROM gcr.io/distroless/static-debian12:nonroot@sha256:afa5c872c891853ca7fcf1f12c3edb23f7eeef36189728842dd51042ff57f7ab
COPY --from=build /sheprd-relay /usr/local/bin/sheprd-relay
# Inside a container the relay listens on every interface; the host decides
# what is published (deploy/relay/compose.yml keeps /metrics private).
ENV RELAY_LISTEN=0.0.0.0:8080 \
    RELAY_METRICS=0.0.0.0:9090 \
    RELAY_QUIC=0.0.0.0:8443
EXPOSE 8080 9090 8443/udp
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/sheprd-relay"]
