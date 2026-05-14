# Stage 1: Build
FROM rust:1.88-slim AS builder

# Install system dependencies for building
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/talon
COPY . .

# Build the Talon binary
RUN cargo build --release -p talon --bin talon

# Stage 2: Final
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary from builder
COPY --from=builder /usr/src/talon/target/release/talon /usr/local/bin/talon

# Create directory for persistent memory
RUN mkdir -p /data/talon

# Environment variables
ENV TALON_DATA_DIR=/data/talon
ENV RUST_LOG=info

WORKDIR /data/talon

ENTRYPOINT ["talon"]
