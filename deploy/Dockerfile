FROM rust:slim AS builder
WORKDIR /src
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --uid 10001 --create-home hoth
COPY --from=builder /src/target/release/hoth /usr/local/bin/hoth
USER 10001
ENTRYPOINT ["/usr/local/bin/hoth"]
