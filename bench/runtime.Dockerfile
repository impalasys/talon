FROM rust:1.91.1-slim AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    clang \
    g++ \
    libclang-dev \
    pkg-config \
    make \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/talon
ARG CARGO_FEATURES=""

COPY Cargo.toml Cargo.lock build.rs ./
COPY third_party ./third_party
COPY proto ./proto
COPY src ./src
COPY talon.yaml ./talon.yaml

RUN if [ -n "$CARGO_FEATURES" ]; then \
        cargo build --release --locked --bins --features "$CARGO_FEATURES"; \
    else \
        cargo build --release --locked --bins; \
    fi && \
    mkdir -p /usr/src/talon/dist && \
    cp /usr/src/talon/target/release/talon-server /usr/src/talon/dist/talon-server && \
    cp /usr/src/talon/target/release/talon-worker /usr/src/talon/dist/talon-worker && \
    cp /usr/src/talon/target/release/talon-cli /usr/src/talon/dist/talon-cli && \
    cp /usr/src/talon/target/release/talon-node /usr/src/talon/dist/talon-node

FROM debian:trixie-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/talon/dist/talon-server /usr/local/bin/talon-server
COPY --from=builder /usr/src/talon/dist/talon-worker /usr/local/bin/talon-worker
COPY --from=builder /usr/src/talon/dist/talon-cli /usr/local/bin/talon-cli
COPY --from=builder /usr/src/talon/dist/talon-node /usr/local/bin/talon-node

RUN mkdir -p /data/talon

ENV TALON_DATA_DIR=/data/talon
ENV RUST_LOG=info

WORKDIR /data/talon

CMD ["talon-server"]
