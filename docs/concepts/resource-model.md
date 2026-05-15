---
title: Resource Model
sidebar_position: 3
---

Talon manages durable resources, not just prompts and chat turns.

## Namespace

A namespace is the primary grouping and tenancy boundary.

Most runtime resources live under a namespace:

- agents
- sessions
- schedules
- namespace knowledge
- MCP bindings

## Agent template

An agent template defines a reusable agent base:

- system prompt
- model policy
- features
- MCP server references
- capabilities

Templates are global resources. They let operators standardize behavior before creating namespace-specific runtime agents.

## Agent

An agent is the runtime-facing resource you actually create sessions against.

An agent can be:

- fully custom
- derived from a template with deltas

The effective spec is what the worker ultimately executes.

## Session

A session is the durable interaction unit for an agent. It stores:

- identity
- lifecycle state
- timestamps and labels
- persisted messages
- execution steps

## Schedule

A schedule is a durable instruction to trigger agent work later or repeatedly.

Schedules target either:

- a new session flow
- or a specific existing session

## Knowledge

Knowledge stores durable content that an agent or namespace can use at runtime.

In practice, this is where you keep:

- operating context
- product or tenant notes
- policies and playbooks

## MCP server and MCP binding

Talon splits tool infrastructure into two resources:

- `McpServer`: the server definition itself
- `McpServerBinding`: how that server is applied inside a namespace

That lets the same server surface be reused with different args, headers, auth broker settings, or tool allowlists.

## Read next

- [Agents and Templates](./agents-and-templates.md)
- [Namespaces, Knowledge, and MCP](./namespaces-knowledge-and-mcp.md)
- [Runtime Surfaces](../reference/runtime-surfaces.md)
