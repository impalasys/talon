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
- MCP servers

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

## MCP server

`McpServer` describes a namespace-local tool endpoint:

- transport, target URL, args, and headers
- optional auth broker configuration
- embedded tool policy

Agents reference MCP servers by simple name. Resolution follows namespace ancestry from the agent namespace toward its parents, and the first matching server wins. A child namespace can define the same server name to override a parent. `spec.policy.tools.allowlist` limits exposed MCP tools by exact tool name; an empty or missing allowlist exposes all tools.

## Read next

- [Agents and Templates](./04-agents-and-templates.md)
- [Namespaces, Knowledge, and MCP](./06-namespaces-knowledge-and-mcp.md)
- [Runtime Surfaces](../05-reference/02-runtime-surfaces.md)
