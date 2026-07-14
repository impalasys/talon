---
slug: /
title: Introduction
sidebar_position: 1
---

Talon is an agent control plane. It gives you durable runtime state, operational APIs, worker execution, schedules, knowledge, namespace-scoped MCP tools, and a browser UI for inspection.

If you are new to Talon, the fastest way to build the right mental model is:

1. understand how Talon works as a system
2. understand the core resources Talon manages
3. run the local stack and inspect it in Sightline
4. build an agent or client against the real contracts

## Start here

- [How Talon Works](./02-concepts/01-how-talon-works.md) for the end-to-end runtime flow
- [Runtime Topology](./02-concepts/02-runtime-topology.md) for the concrete local stack
- [Quickstart](./01-getting-started/01-quickstart.md) for the fastest local loop

## What Talon manages

- **Namespaces**: tenancy and grouping for agents, sessions, schedules, knowledge, and MCP servers
- **Agent templates and agents**: reusable specs and runtime instances
- **Sessions**: durable interaction state, persisted messages, and streamed execution steps
- **Schedules**: recurring or one-shot dispatch into the worker runtime
- **Knowledge and MCP servers**: runtime context and tool surfaces
- **Gateway and worker processes**: the control-plane API and execution engine
- **Sightline**: a browser-native operator surface over the same runtime system

## Reading order

1. [How Talon Works](./02-concepts/01-how-talon-works.md)
2. [Runtime Topology](./02-concepts/02-runtime-topology.md)
3. [Resource Model](./02-concepts/03-resource-model.md)
4. [Quickstart](./01-getting-started/01-quickstart.md)
5. [Build Your First Agent](./03-tutorials/01-first-agent.md)

## Choose your next path

- Building against Talon from code: [Build a Client](./03-tutorials/02-build-a-client.md)
- Operating Talon locally or in deployment: [Operate](./04-operations/01-local-development.md)
- Looking for exact request/response shapes: [Reference](./05-reference/00-overview.md)
