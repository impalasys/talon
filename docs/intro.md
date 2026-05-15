---
slug: /
title: Introduction
sidebar_position: 1
---

Talon is an agent control plane. It gives you durable runtime state, operational APIs, worker execution, schedules, knowledge, MCP tool bindings, and a browser UI for inspection.

If you are new to Talon, the fastest way to build the right mental model is:

1. understand how Talon works as a system
2. understand the core resources Talon manages
3. run the local stack and inspect it in Sightline
4. build an agent or client against the real contracts

## Start here

- [How Talon Works](./concepts/how-talon-works.md) for the end-to-end runtime flow
- [Runtime Topology](./concepts/runtime-topology.md) for the concrete local stack
- [Quickstart](./getting-started/quickstart.md) for the fastest local loop

## What Talon manages

- **Namespaces**: tenancy and grouping for agents, sessions, schedules, knowledge, and MCP bindings
- **Agent templates and agents**: reusable specs and runtime instances
- **Sessions**: durable interaction state, persisted messages, and streamed execution steps
- **Schedules**: recurring or one-shot dispatch into the worker runtime
- **Knowledge and MCP bindings**: runtime context and tool surfaces
- **Gateway and worker processes**: the control-plane API and execution engine
- **Sightline**: a browser-native operator surface over the same runtime system

## Reading order

1. [How Talon Works](./concepts/how-talon-works.md)
2. [Runtime Topology](./concepts/runtime-topology.md)
3. [Resource Model](./concepts/resource-model.md)
4. [Quickstart](./getting-started/quickstart.md)
5. [Build Your First Agent](./tutorials/first-agent.md)

## Choose your next path

- Building against Talon from code: [Build a Client](./tutorials/build-a-client.md)
- Operating Talon locally or in deployment: [Operate](./operations/local-development.md)
- Looking for exact request/response shapes: [Reference](./reference/index.md)
