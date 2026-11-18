# syntax=docker/dockerfile:1
FROM rust:slim-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY apps/ apps/
COPY crates/ crates/
RUN cargo fetch --locked
RUN --network=none cargo build -p hoth --release --locked --offline

FROM debian:bookworm-slim
RUN groupadd --gid 10001 hoth && useradd --uid 10001 --gid hoth --create-home hoth
COPY --from=builder /src/target/release/hoth /usr/local/bin/hoth
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/hoth"]
