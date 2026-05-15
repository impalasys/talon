---
title: Architecture
sidebar_position: 2
---

Talon splits its runtime into a few clear roles.

## Core components

### Gateway

The gateway owns the external API surface:

- gRPC service definitions in `proto/gateway.proto`
- REST endpoints via `google.api.http` annotations
- browser-facing UI session endpoints for Sightline and AI SDK-compatible clients

### Worker

The worker processes agent execution:

- receives session work
- resolves models and tools
- executes turns
- emits session-step events and persisted session state

The gateway accepts and persists work. The worker executes it.

### Sightline UI

The UI is a browser-native operator surface for:

- exploring namespaces and agents
- inspecting sessions and schedules
- observing tool calls and streamed responses
- debugging local or remote Talon deployments

### Envoy / edge surface

Envoy provides a consistent edge interface over the gateway. It is the right place for browser-facing concerns like CORS and consistent URL routing.

## Resource model

The most important Talon resources are:

- namespaces
- agent templates
- agents
- sessions
- schedules
- namespace knowledge
- MCP servers and bindings

Read [Resource Model](../concepts/resource-model.md) for the durable control-plane view.

Talon is organized around a few resource types:

- **Namespaces**: tenancy and grouping
- **Agent templates**: reusable base definitions
- **Agents**: instantiated runtime entities
- **Sessions**: durable conversational/execution state
- **Knowledge**: namespace or agent knowledge assets
- **Schedules**: wakeups and recurring work
- **MCP servers / bindings**: tool connectivity and policy

## Transport model

- **gRPC** is the canonical service contract
- **REST** is derived from the same proto definitions through HTTP annotations
- **Browser streaming** is exposed for clients like Sightline and AI SDK-compatible frontends

## Where to go next

- [Agents and templates](../concepts/agents-and-templates.md)
- [Sessions and streaming](../concepts/sessions-and-streaming.md)
- [Gateway API reference](../reference/generated/gateway-service.md)
