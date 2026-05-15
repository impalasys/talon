---
title: Namespaces, Knowledge, and MCP
sidebar_position: 3
---

These three concepts define how Talon organizes data and tool access.

## Namespaces

Namespaces are the primary grouping and tenancy primitive. Most resources live under a namespace:

- agents
- sessions
- schedules
- namespace knowledge
- MCP bindings

Namespaces also matter for auth and scoping. JWT-backed access can be restricted to a namespace, agent, or even session.

## Knowledge

Knowledge resources store durable content that an agent or namespace can use at runtime.

In practice this supports flows like:

- curated operating context
- product or account-specific notes
- playbooks and policies

Talon exposes both agent-oriented knowledge access and namespace knowledge CRUD. That split is useful when operators want to manage context centrally while agents consume it indirectly.

## MCP servers and bindings

Talon separates the definition of an MCP server from how it is bound into a namespace.

- **McpServer** describes the server endpoint/transport
- **McpServerBinding** applies it inside a namespace with arguments, headers, policy, and tool allowlists

This lets operators apply the same tool surface differently across environments or tenants.

Lifecycle changes to MCP servers and bindings also affect the worker’s runtime registry, so these are not just static config objects.

## Read next

- [Authentication and Access](../operations/authentication-and-access.md)
- [Resource Model](./resource-model.md)
