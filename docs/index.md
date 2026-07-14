---
title: Introduction
sidebar:
  order: 1
---

Talon is an agent control plane. It gives you durable runtime state, operational APIs, worker execution, schedules, knowledge, namespace-scoped MCP tools, and a browser UI for inspection.

If you are new to Talon, the fastest way to build the right mental model is:

1. understand how Talon works as a system
2. understand the core resources Talon manages
3. run the local stack and inspect it in Sightline
4. build an agent or client against the real contracts

## Start here

- [How Talon Works](./concepts/how-talon-works) for the end-to-end runtime flow
- [Runtime Topology](./concepts/runtime-topology) for the concrete local stack
- [Quickstart](./getting-started/quickstart) for the fastest local loop

## What Talon manages

- **Namespaces**: tenancy and grouping for agents, sessions, schedules, knowledge, and MCP servers
- **Agent templates and agents**: reusable specs and runtime instances
- **Sessions**: durable interaction state, persisted messages, and streamed execution steps
- **Schedules**: recurring or one-shot dispatch into the worker runtime
- **Knowledge and MCP servers**: runtime context and tool surfaces
- **Gateway and worker processes**: the control-plane API and execution engine
- **Sightline**: a browser-native operator surface over the same runtime system

## Reading order

1. [How Talon Works](./concepts/how-talon-works)
2. [Runtime Topology](./concepts/runtime-topology)
3. [Resource Model](./concepts/resource-model)
4. [Quickstart](./getting-started/quickstart)
5. [Build Your First Agent](./tutorials/first-agent)

## Choose your next path

- Building against Talon from code: [Build a Client](./tutorials/build-a-client)
- Operating Talon locally or in deployment: [Operate](./operations/local-development)
- Looking for exact request/response shapes: [Reference](./reference)
