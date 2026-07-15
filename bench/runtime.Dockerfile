FROM rust:1.91.1-slim AS builder

RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    clang \
    g++ \
    libclang-dev \
    pkg-config \
    make \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/talon
ARG CARGO_FEATURES="rocksdb"

COPY Cargo.toml Cargo.lock build.rs ./
COPY third_party ./third_party
COPY proto ./proto
COPY sdk/rust/talon-client ./sdk/rust/talon-client
COPY tools/install-hooks ./tools/install-hooks
COPY src ./src
COPY talon.yaml ./talon.yaml

RUN if [ -n "$CARGO_FEATURES" ]; then \
        cargo build --release --locked --features "$CARGO_FEATURES" \
          --bin talon-node; \
    else \
        cargo build --release --locked \
          --bin talon-node; \
    fi && \
    mkdir -p /usr/src/talon/dist && \
    cp /usr/src/talon/target/release/talon-node /usr/src/talon/dist/talon-node

FROM debian:trixie-slim

RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libssl3 \
    libstdc++6 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/talon/dist/talon-node /usr/local/bin/talon-node

RUN mkdir -p /data/talon

ENV TALON_DATA_DIR=/data/talon
ENV RUST_LOG=info

WORKDIR /data/talon

CMD ["talon-node"]
