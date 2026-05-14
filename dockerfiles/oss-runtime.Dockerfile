# syntax=docker/dockerfile:1.7

FROM rust:1.91.1-slim AS chef

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install cargo-chef --locked

WORKDIR /usr/src/talon

FROM chef AS planner

COPY Cargo.toml Cargo.lock build.rs ./
COPY third_party ./third_party
COPY proto ./proto
COPY src ./src
COPY talon.yaml ./talon.yaml
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

COPY --from=planner /usr/src/talon/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --release --recipe-path recipe.json

COPY Cargo.toml Cargo.lock build.rs ./
COPY third_party ./third_party
COPY proto ./proto
COPY src ./src
COPY talon.yaml ./talon.yaml
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --locked --bins && \
    mkdir -p /usr/src/talon/dist && \
    cp /usr/src/talon/target/release/talon-server /usr/src/talon/dist/talon-server && \
    cp /usr/src/talon/target/release/talon-worker /usr/src/talon/dist/talon-worker && \
    cp /usr/src/talon/target/release/talon-cli /usr/src/talon/dist/talon-cli

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/talon/dist/talon-server /usr/local/bin/talon-server
COPY --from=builder /usr/src/talon/dist/talon-worker /usr/local/bin/talon-worker
COPY --from=builder /usr/src/talon/dist/talon-cli /usr/local/bin/talon-cli
COPY --from=builder /usr/src/talon/talon.yaml /data/talon/talon.yaml

RUN mkdir -p /data/talon

ENV TALON_DATA_DIR=/data/talon
ENV RUST_LOG=info

WORKDIR /data/talon

CMD ["talon-server"]
