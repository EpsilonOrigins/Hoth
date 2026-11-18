# syntax=docker/dockerfile:1
# Supply a pre-fetched Cargo registry with --build-context cargo-registry=PATH.
FROM scratch AS cargo-registry

FROM rust:1.100.0-slim-bookworm AS builder
ARG CARGO_BUILD_JOBS=2
RUN --mount=type=bind,source=.,target=/src \
    --mount=type=bind,from=cargo-registry,target=/prefetched-registry \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build-target \
    --mount=type=secret,id=cargo_ca \
    set -eu; \
    cd /src; \
    export CARGO_TARGET_DIR=/build-target RUSTUP_TOOLCHAIN=1.100.0; \
    if [ -d /prefetched-registry/index ]; then \
        export CARGO_HOME=/tmp/prefetched-cargo CARGO_NET_OFFLINE=true; \
        mkdir -p "$CARGO_HOME"; \
        ln -s /prefetched-registry "$CARGO_HOME/registry"; \
    elif [ -f /run/secrets/cargo_ca ]; then \
        export CARGO_HTTP_SSL_CAINFO=/run/secrets/cargo_ca; \
    fi; \
    cargo fetch --locked; \
    sh deploy/build-package.sh

FROM busybox:1.37.0-musl AS web-server

FROM debian:bookworm-slim AS runtime
RUN groupadd --gid 10001 hoth && useradd --uid 10001 --gid hoth --create-home hoth \
    && install -d -o 10001 -g 10001 /data
USER 10001:10001

FROM runtime AS production
COPY --from=builder /package/usr/local/bin/hoth /usr/local/bin/hoth
ENTRYPOINT ["/usr/local/bin/hoth"]

FROM runtime AS package
COPY --from=builder /package/ /
COPY --from=web-server /bin/busybox /usr/local/bin/busybox
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/hoth-container"]
CMD ["serve-globe"]

FROM package AS integration
