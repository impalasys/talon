---
title: Build an Internal Ops Copilot
sidebar_position: 6
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
- a namespace binding to the built-in `talon-ops` MCP server
- a small read-oriented tool allowlist

## 1. Apply the example manifests

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 apply -f manifests/examples/internal-ops-copilot/namespace.yaml
cargo run --bin talon-cli -- --gateway http://localhost:18789 apply -f manifests/examples/internal-ops-copilot/ops-copilot-template.yaml
cargo run --bin talon-cli -- --gateway http://localhost:18789 apply -f manifests/examples/internal-ops-copilot/ops-copilot.yaml
cargo run --bin talon-cli -- --gateway http://localhost:18789 apply -f manifests/examples/internal-ops-copilot/ops-tools.binding.yaml
```

This creates:

- the `internal-ops` namespace
- the `ops-copilot-template` template
- the `ops-copilot` agent
- the `ops-tools` MCP binding

## 2. Load the runbook

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 knowledge sync \
  --namespace internal-ops \
  --dir manifests/examples/internal-ops-copilot/knowledge
```

## 3. Verify the MCP binding

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 get mcpserverbinding ops-tools --namespace internal-ops
```

The binding should point at `talon-ops` and allow only:

- `list_schedules`
- `get_schedule`
- `list_channels`
- `get_channel`
- `list_channel_messages`
- `get_channel_message`
- `list_mcp_bindings`
- `get_mcp_binding`

## 4. Start an operator session

Create a session:

```bash
curl -sS http://localhost:18789/v1/ns/internal-ops/agents/ops-copilot/sessions \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"ns":"internal-ops","agent":"ops-copilot"}'
```

Then ask an ops question:

```bash
curl -sS http://localhost:18789/v1/ui/ns/internal-ops/agents/ops-copilot/sessions/<session-id> \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"messages":[{"content":"List the schedules in this namespace and summarize anything unusual."}]}'
```

## 5. Inspect tool use in Sightline

In Sightline, open the session and confirm:

- the agent stayed inside the intended namespace
- only the allowed `talon-ops` tools were exposed
- the session transcript makes the tool usage legible

That is the core ops-copilot pattern in Talon: narrow bindings, explicit visibility, and no broad ambient tool access.

## Why this tutorial matters

Internal assistants get risky when every tool is globally available. Talon’s namespace bindings let you keep the exposure legible and reviewable.

## Read next

- [Runtime Surfaces](../reference/runtime-surfaces.md)
- [Authentication and Access](../operations/authentication-and-access.md)
