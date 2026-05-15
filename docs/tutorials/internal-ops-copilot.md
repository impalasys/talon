---
title: Build an Internal Ops Copilot
sidebar_position: 6
---

This tutorial shows how to build an internal assistant with tighter governance, bounded tools, and clearer operational controls.

## What you are building

You will build an internal copilot that helps operators inspect Talon resources and answer operational questions without exposing the full control plane broadly.

The system will include:

- a dedicated namespace
- an ops-oriented agent
- bounded MCP bindings
- explicit auth and access considerations
- inspection and debugging through Sightline

## Talon concepts used

- namespace
- agent template and agent
- knowledge
- MCP servers and bindings
- capability boundaries
- auth and scoping
- sessions and streamed steps

## Runtime surfaces used

- gateway CRUD for setup
- UI session surface for chat-like operator use
- MCP-backed ops tools
- Sightline for visibility

## Architecture

```text
Operator
  -> Browser UI or Sightline
    -> Talon gateway auth boundary
      -> Ops namespace
      -> Bounded MCP bindings
      -> Agent execution through worker
```

This system is intentionally narrower than the earlier tutorials. The point is to teach restraint: fewer tools, better scoping, cleaner auth, and more legible runtime behavior.

## Prerequisites

- the local Talon stack is running
- you have read [Authentication and Access](../operations/authentication-and-access.md)
- you understand [Namespaces, Knowledge, and MCP](../concepts/namespaces-knowledge-and-mcp.md)

## Apply the example assets

This tutorial ships with:

- `talon/manifests/examples/internal-ops-copilot/ops.yaml`
- `talon/manifests/examples/internal-ops-copilot/knowledge/runbook.md`

Apply the bundle:

```bash
cd talon
cargo run --bin talon-cli -- --rest apply -f manifests/examples/internal-ops-copilot/ops.yaml
```

The bundle models:

- an operations namespace
- one internal copilot agent
- a binding to the existing `talon-ops` MCP server
- a narrow allowlist of tools for inspection-oriented workflows

## Walk an end-to-end flow

Use prompts like:

- “List the schedules in this namespace and summarize anything risky”
- “Explain the last failed session in plain English”
- “What MCP bindings are available to this copilot?”

This is a good place to compare product surfaces:

- use the UI session API for an operator chat UI
- use gRPC or REST for explicit backend automation or admin tasks

## Inspect and debug in Sightline

Inspect:

- the namespace resources
- the active MCP bindings
- the session step stream
- whether the agent used only the tools you intended

If the agent feels too powerful:

- move more logic into a narrower template
- restrict `allowed_tool_names`
- split exploratory and mutating tooling into separate bindings or agents

## Why this structure works

Internal copilots fail when every tool is attached everywhere. Talon gives you better structure:

- namespace boundary for isolation
- explicit bindings for tool exposure
- auth modes and JWT scopes for access control
- Sightline for runtime observability

## Extend the system

Good next steps:

- add JWT-scoped access for a single namespace
- split read-only and write-capable tools
- add schedule-based audit summaries
- connect a custom operator frontend

## Production notes

Before deploying broadly:

- choose a real auth mode instead of open local access
- audit every MCP binding and allowlist
- keep mutating tools out of the widest user path

## What you learned

You used Talon as a governed internal assistant platform with explicit boundaries and observability.

## Read next

- [Runtime Surfaces](../reference/runtime-surfaces.md)
- [Authentication and Access](../operations/authentication-and-access.md)
- [Gateway API](../reference/generated/gateway-service.md)
