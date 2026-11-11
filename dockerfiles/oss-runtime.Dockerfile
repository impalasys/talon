# syntax=docker/dockerfile:1.7
# CI supplies a Bazel-built context and selects runtime-from-bazel. A direct
# docker build uses the same Bazel targets inside the Linux builder below.
FROM docker:27-cli AS docker-cli

FROM debian:trixie-slim AS runtime-base
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl python3 python3-venv libssl3 libstdc++6 \
    && rm -rf /var/lib/apt/lists/*
COPY tools/bazel/requirements.lock /tmp/talon-requirements.lock
RUN python3 -m venv /opt/talon-e2e-venv \
    && /opt/talon-e2e-venv/bin/pip install --no-cache-dir --require-hashes -r /tmp/talon-requirements.lock \
    && rm /tmp/talon-requirements.lock
RUN python3 -m venv /opt/talon-monty-venv \
    && /opt/talon-monty-venv/bin/pip install --no-cache-dir pydantic-monty-runtime==0.0.21 \
    && ln -s /opt/talon-monty-venv/bin/monty /usr/local/bin/monty
COPY --from=docker-cli /usr/local/bin/docker /usr/local/bin/docker
ENV TALON_DATA_DIR=/data/talon RUST_LOG=info
WORKDIR /data/talon
EXPOSE 50051 8081
CMD ["talon-server"]

FROM runtime-base AS runtime-from-bazel
COPY bin/ /usr/local/bin/
COPY talon.yaml models.yaml /data/talon/
COPY models/ /data/talon/models/

FROM debian:trixie-slim AS benchmark-base
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl libssl3 libstdc++6 && rm -rf /var/lib/apt/lists/*
ENV TALON_DATA_DIR=/data/talon RUST_LOG=info
WORKDIR /data/talon
CMD ["talon-node"]

FROM benchmark-base AS benchmark-from-bazel
COPY bin/talon-node bin/talon-cli /usr/local/bin/
COPY models.yaml /data/talon/
COPY models/ /data/talon/models/

FROM gcr.io/bazel-public/bazel:9.3.2 AS bazel-builder
USER root
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev clang libclang-dev cmake && rm -rf /var/lib/apt/lists/*
WORKDIR /workspace
COPY . .
RUN --mount=type=cache,target=/root/.cache/bazel \
    bazel --output_user_root=/root/.cache/bazel build //dockerfiles:runtime_context \
    && cp -rL bazel-bin/dockerfiles/runtime_context /out

FROM benchmark-base AS benchmark-runtime
COPY --from=bazel-builder /out/bin/talon-node /out/bin/talon-cli /usr/local/bin/
COPY models.yaml /data/talon/
COPY models/ /data/talon/models/

FROM runtime-base AS runtime
COPY --from=bazel-builder /out/bin/ /usr/local/bin/
COPY talon.yaml models.yaml /data/talon/
COPY models/ /data/talon/models/
