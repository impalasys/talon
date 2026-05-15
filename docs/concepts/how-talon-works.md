---
title: How Talon Works
sidebar_position: 1
---

Talon is easiest to understand as a control plane plus a worker runtime.

## The core loop

The usual Talon flow is:

1. define reusable agent behavior with templates or direct agent specs
2. create agents inside a namespace
3. create a session for an agent
4. send a message through the gateway
5. let the worker execute the turn, call tools, and emit step events
6. read the streamed steps and persisted session state later

## The runtime roles

### Gateway

The gateway is the API entry point. It owns:

- the canonical gRPC service
- REST-transcoded HTTP routes exposed through Envoy
- the browser-facing UI session surface used by Sightline-style clients
- CRUD for namespaces, agents, schedules, templates, knowledge, and MCP resources

### Worker

The worker consumes session dispatch and control events, executes agent turns, resolves tools, and persists the resulting state.

That split matters because Talon is not “just a chat server”. The gateway accepts and persists intent. The worker performs execution.

### Control plane

The control plane backing Talon provides:

- key-value style persistence over Postgres
- Pub/Sub-backed event delivery
- a scheduler backend for delayed or recurring dispatch

## What happens when you send a message

At a high level:

1. a client sends `SendMessage`
2. the gateway validates auth and appends the user message to the session
3. the gateway publishes a session-dispatch event
4. the worker consumes that event
5. the worker resolves the effective agent spec, models, tools, and context
6. the worker emits session step events as execution proceeds
7. the session returns to an idle state with persisted messages and steps

## Why the model is useful

This design gives Talon a durable operational surface:

- sessions can be resumed later
- tools and schedules are observable
- the UI can inspect the same state the API exposes
- auth and policy can be applied at the control-plane boundary

## Read next

- [Runtime Topology](./runtime-topology.md)
- [Resource Model](./resource-model.md)
- [Sessions and Streaming](./sessions-and-streaming.md)
