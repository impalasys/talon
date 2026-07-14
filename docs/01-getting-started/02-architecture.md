---
title: Architecture
sidebar_position: 2
---

Talon splits its runtime into a few clear roles.

## Core components

### Gateway

The gateway owns the external API surface:

- versioned gRPC service definitions under `proto/talon/v1`
- native gRPC and gRPC-Web on the same gateway port
- named services for namespaces, resources, sessions, channels, workflows, knowledge, and auth

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

## Resource model

The most important Talon resources are:

- namespaces
- agent templates
- agents
- sessions
- schedules
- namespace knowledge
- MCP servers

Read [Resource Model](../02-concepts/03-resource-model.md) for the durable control-plane view.

Talon is organized around a few resource types:

- **Namespaces**: tenancy and grouping
- **Agent templates**: reusable base definitions
- **Agents**: instantiated runtime entities
- **Sessions**: durable conversational/execution state
- **Knowledge**: namespace or agent knowledge assets
- **Schedules**: wakeups and recurring work
- **MCP servers**: namespace-scoped tool connectivity and policy

## Transport model

- **gRPC** is the canonical service contract for backend integrations
- **gRPC-Web** is the browser contract for clients like Sightline and AI SDK-compatible frontends
- SDKs expose one Talon clientset with accessors for the named services

## Where to go next

- [Agents and templates](../02-concepts/04-agents-and-templates.md)
- [Sessions and streaming](../02-concepts/05-sessions-and-streaming.md)
