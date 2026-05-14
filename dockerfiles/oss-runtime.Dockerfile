# syntax=docker/dockerfile:1.7

FROM rust:1.88-slim AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/talon

COPY Cargo.toml Cargo.lock build.rs ./
COPY proto ./proto
COPY src ./src
COPY third_party ./third_party
COPY talon.yaml ./talon.yaml

RUN cargo build --release --locked --bins

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/talon/target/release/talon-server /usr/local/bin/talon-server
COPY --from=builder /usr/src/talon/target/release/talon-worker /usr/local/bin/talon-worker
COPY --from=builder /usr/src/talon/target/release/talon-cli /usr/local/bin/talon-cli
COPY --from=builder /usr/src/talon/talon.yaml /data/talon/talon.yaml

RUN mkdir -p /data/talon

ENV TALON_DATA_DIR=/data/talon
ENV RUST_LOG=info

WORKDIR /data/talon

CMD ["talon-server"]
