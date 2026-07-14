---
title: Build an Internal Ops Copilot
sidebar:
  order: 6
---

This tutorial shows a narrower, safer Talon pattern: an internal agent with bounded operational tools.

Before you begin, clone the repository, create `.env` from `.env.example`, and set `OPENAI_API_KEY` so the example agent uses a real model provider:

```bash
git clone https://github.com/impalasys/talon.git
cd talon
cp .env.example .env
```

Start the local stack if it is not already running:

```bash
docker compose up --build -d
```

## What you are building

You will create:

- an `internal-ops` namespace
- an `ops-copilot` agent
- a namespace-local `talon-ops` MCP server
- a small read-oriented tool allowlist

## 1. Apply the example manifests

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/internal-ops-copilot/namespace.yaml
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/internal-ops-copilot/ops-copilot-template.yaml
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/internal-ops-copilot/ops-copilot.yaml
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/internal-ops-copilot/talon-ops.mcp-server.yaml
```

This creates:

- the `internal-ops` namespace
- the `ops-copilot-template` template
- the `ops-copilot` agent
- the namespace-local `talon-ops` MCP server

## 2. Load the runbook

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 knowledge sync \
  --namespace internal-ops \
  --dir manifests/examples/internal-ops-copilot/knowledge
```

## 3. Verify the MCP server

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 get mcpserver talon-ops --namespace internal-ops
```

The server should expose only:

- `list_schedules`
- `get_schedule`
- `list_channels`
- `get_channel`
- `list_channel_messages`
- `get_channel_message`
- `list_mcp_servers`
- `get_mcp_server`

## 4. Start an operator session

Create a session and ask an ops question:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 session prompt \
  --namespace internal-ops \
  --agent ops-copilot \
  --stream \
  "List the schedules in this namespace and summarize anything unusual."
```

## 5. Inspect tool use in Sightline

In Sightline, open the session and confirm:

- the agent stayed inside the intended namespace
- only the allowed `talon-ops` tools were exposed
- the session transcript makes the tool usage legible

That is the core ops-copilot pattern in Talon: narrow namespace-local MCP servers, explicit visibility, and no broad ambient tool access.

## Why this tutorial matters

Internal assistants get risky when every tool is globally available. Talon’s namespace-scoped MCP servers and embedded policy let you keep the exposure legible and reviewable.

## Read next

- [Runtime Surfaces](../reference/runtime-surfaces)
- [Authentication and Access](../operations/authentication-and-access)
