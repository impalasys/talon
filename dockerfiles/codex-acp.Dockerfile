# syntax=docker/dockerfile:1.7

FROM node:22-bookworm-slim

ARG CODEX_CLI_VERSION=latest
ARG CODEX_ACP_VERSION=0.16.0
ARG CLAUDE_CODE_VERSION=latest
ARG CLAUDE_CODE_ACP_VERSION=0.16.2
ARG OPENCODE_VERSION=latest

LABEL org.opencontainers.image.title="Talon ACP Harness Sandbox"
LABEL org.opencontainers.image.description="ACP-compatible coding harnesses for Talon Docker sandboxes"
LABEL org.opencontainers.image.source="https://github.com/impala-systems/talon"

ENV NODE_ENV=production
ENV NPM_CONFIG_AUDIT=false
ENV NPM_CONFIG_FUND=false
ENV NPM_CONFIG_UPDATE_NOTIFIER=false
ENV PIP_BREAK_SYSTEM_PACKAGES=1
ENV CODEX_HOME=/home/codex/.codex
ENV CLAUDE_CONFIG_DIR=/home/codex/.claude
ENV OPENCODE_CONFIG_DIR=/home/codex/.config/opencode

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
        "@anthropic-ai/claude-code@${CLAUDE_CODE_VERSION}" \
        "@zed-industries/claude-code-acp@${CLAUDE_CODE_ACP_VERSION}" \
        "opencode-ai@${OPENCODE_VERSION}" \
    && npm cache clean --force

RUN useradd --create-home --shell /bin/bash codex \
    && mkdir -p /workspace "${CODEX_HOME}" "${CLAUDE_CONFIG_DIR}" "${OPENCODE_CONFIG_DIR}" \
    && chown -R codex:codex /workspace /home/codex

USER codex
WORKDIR /workspace

CMD ["codex-acp"]
