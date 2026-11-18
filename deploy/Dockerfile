# syntax=docker/dockerfile:1
FROM rust:1.100.0-slim-bookworm AS builder
ARG CARGO_BUILD_JOBS=2
RUN --mount=type=bind,source=.,target=/src \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build-target \
    --mount=type=secret,id=cargo_ca \
    set -eu; \
    cd /src; \
    export CARGO_TARGET_DIR=/build-target RUSTUP_TOOLCHAIN=1.100.0; \
    if [ -f /run/secrets/cargo_ca ]; then export CARGO_HTTP_SSL_CAINFO=/run/secrets/cargo_ca; fi; \
    cargo fetch --locked; \
    cargo build --release --locked --offline -p hoth --bin hoth --example verify; \
    install -D /build-target/release/hoth /binaries/hoth; \
    install -D /build-target/release/examples/verify /binaries/verify

FROM debian:bookworm-slim AS runtime
RUN groupadd --gid 10001 hoth && useradd --uid 10001 --gid hoth --create-home hoth
COPY --from=builder /binaries/hoth /usr/local/bin/hoth
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/hoth"]

FROM runtime AS integration
COPY --from=builder /binaries/verify /usr/local/bin/hoth-verify
COPY --chmod=u=rwX,go=rX tests/fixtures/ /fixtures/
COPY --chmod=0755 deploy/run-fixture.sh /usr/local/bin/run-fixture

FROM runtime AS production
