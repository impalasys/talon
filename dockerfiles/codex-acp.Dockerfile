# syntax=docker/dockerfile:1.7

FROM rust:1.91.1-slim-bookworm AS talon-adapter-builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        clang \
        g++ \
        libclang-dev \
        libssl-dev \
        make \
        pkg-config \
        protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/talon

COPY Cargo.toml Cargo.lock build.rs ./
COPY third_party ./third_party
COPY proto ./proto
COPY src ./src

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --locked --bin talon-codex-acp

FROM node:22-bookworm-slim

ARG CODEX_CLI_VERSION=latest
ARG CODEX_ACP_VERSION=0.16.0

LABEL org.opencontainers.image.title="Talon Codex ACP Sandbox"
LABEL org.opencontainers.image.description="Codex CLI plus the Zed Codex ACP adapter for Talon Docker sandboxes"
LABEL org.opencontainers.image.source="https://github.com/impala-systems/talon"

ENV NODE_ENV=production
ENV NPM_CONFIG_AUDIT=false
ENV NPM_CONFIG_FUND=false
ENV NPM_CONFIG_UPDATE_NOTIFIER=false
ENV PIP_BREAK_SYSTEM_PACKAGES=1
ENV CODEX_HOME=/home/codex/.codex

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        build-essential \
        ca-certificates \
        curl \
        git \
        jq \
        openssh-client \
        procps \
        python3 \
        python3-pip \
        ripgrep \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g \
        "@openai/codex@${CODEX_CLI_VERSION}" \
        "@zed-industries/codex-acp@${CODEX_ACP_VERSION}" \
    && npm cache clean --force

COPY --from=talon-adapter-builder /usr/src/talon/target/release/talon-codex-acp /usr/local/bin/talon-codex-acp

RUN useradd --create-home --shell /bin/bash codex \
    && mkdir -p /workspace "${CODEX_HOME}" \
    && chown -R codex:codex /workspace /home/codex

USER codex
WORKDIR /workspace

CMD ["codex-acp"]
