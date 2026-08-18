---
title: Namespaces and Resolution
sidebar:
  order: 3
---

## In brief

A namespace is Talon's primary tenancy and grouping boundary. Agents, sessions, schedules, files, tools, and other namespaced resources live within it. A namespace answers where a resource belongs and which nearby definitions may be used when a name is resolved; it does not, by itself, answer who may use the resource.

Namespace structure is part of the system model because reusable context is often intentionally shared. A child namespace can use a capability or package defined above it while retaining the ability to replace that definition locally. That gives a team a controlled default without making every application copy a common resource.

## What it owns

Namespaces scope namespaced resources and provide the ancestry used by inheritable name resolution. They also provide a target for selectors: a Deployment can select namespaces for placement, and a UsagePolicy can apply to a namespace or its descendants.

They do **not** grant access merely by existing. Talon's identity and grants model decides whether a caller may act on a scoped resource. Keeping hierarchy separate from authorization lets an organization reuse a parent definition without implicitly giving every child identity broad permission; see [Identity, Access, and Secrets](../boundary-and-policy-model/identity-access-and-secrets).

## Resolution rules

Talon supports hierarchical namespace names. When an inheritable resource is referred to by name, resolution begins in the most specific namespace and walks through its ancestors. A child resource with the same name shadows its parent. This makes the effective choice predictable: the closest definition wins.

Resolution is useful for shared `McpServer` definitions, Skills, and sandbox policy. It is not a license to leave critical dependencies ambiguous. An explicit `ResourceRef` identifies a concrete resource and should be used when a caller needs a fixed target rather than the effective definition for its current namespace.

The hierarchy and a Namespace's declared parent relationship serve related purposes, but docs should not present the internal ancestry implementation as an extra public policy. The portable rule is the one consumers rely on: a more-specific namespace takes precedence over its ancestors for inheritable names.

## Relationships and tradeoffs

Inheritance reduces repeated configuration and makes a shared platform usable across many namespaces. Shadowing supplies local control, but it also means a same-named child resource changes the effective behavior of work in that child. Inspect the resolved resource when debugging a surprising tool, skill, or sandbox choice.

Namespace selectors answer a different question: they let a higher-level declaration apply to many namespaces. They are used for placement and policy, not as an implicit lookup mechanism. [Templates and Deployments](./templates-and-deployments) covers placement; [Tools, MCP, and Sandboxes](../context-and-capability-model/tools-mcp-and-sandboxes) covers the capability resources that commonly resolve through ancestry.

## Read next

- [Templates and Deployments](./templates-and-deployments)
- [Tools, MCP, and Sandboxes](../context-and-capability-model/tools-mcp-and-sandboxes)
