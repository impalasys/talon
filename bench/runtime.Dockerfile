# syntax=docker/dockerfile:1.7
FROM gcr.io/bazel-public/bazel:9.3.2 AS builder
USER root
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev clang libclang-dev cmake && rm -rf /var/lib/apt/lists/*
WORKDIR /workspace
COPY . .
RUN --mount=type=cache,target=/root/.cache/bazel \
    bazel --output_user_root=/root/.cache/bazel build //dockerfiles:runtime_context \
    && cp -rL bazel-bin/dockerfiles/runtime_context /out
FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl libssl3 libstdc++6 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /out/bin/talon-node /out/bin/talon-cli /usr/local/bin/
COPY models.yaml /data/talon/
COPY models/ /data/talon/models/
ENV TALON_DATA_DIR=/data/talon RUST_LOG=info
WORKDIR /data/talon
CMD ["talon-node"]
