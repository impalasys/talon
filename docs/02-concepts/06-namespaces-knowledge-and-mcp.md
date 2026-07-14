---
title: Namespaces, Knowledge, and MCP
sidebar:
  order: 3
---

These three concepts define how Talon organizes data and tool access.

## Namespaces

Namespaces are the primary grouping and tenancy primitive. Most resources live under a namespace:

- agents
- sessions
- schedules
- namespace knowledge
- MCP servers

Namespaces also matter for auth and scoping. JWT-backed access can be restricted to a namespace, agent, or even session.

## Knowledge

Knowledge resources store durable content that an agent or namespace can use at runtime.

In practice this supports flows like:

- curated operating context
- product or account-specific notes
- playbooks and policies

Talon exposes both agent-oriented knowledge access and namespace knowledge CRUD. That split is useful when operators want to manage context centrally while agents consume it indirectly.

## MCP servers

`McpServer` is a namespace-scoped resource that describes both the server endpoint and the policy for exposing its tools.

Agents keep simple `mcpServerRefs` by name. At runtime, Talon resolves each ref through namespace ancestry from most specific to least specific. For an agent in `a:b:c`, Talon checks `a:b:c`, then `a:b`, then `a`. The first `McpServer` with that name wins, so a child namespace can override a parent by defining a local server with the same name. There is no fallback to `Sys` unless `Sys` is actually in that namespace ancestry.

Policy lives directly on the server. `spec.policy.tools.allowlist` is an exact MCP tool-name allowlist; when it is absent or empty, all tools from the server are exposed.

Lifecycle changes to MCP servers also affect the worker’s runtime registry, so these are not just static config objects. Sightline shows local and inherited MCP servers in namespace context and marks child overrides.

## Read next

- [Authentication and Access](../operations/authentication-and-access)
- [Resource Model](./resource-model)
