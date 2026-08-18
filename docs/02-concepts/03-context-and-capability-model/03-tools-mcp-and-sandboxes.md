---
title: Tools, MCP, and Sandboxes
sidebar:
  order: 3
---

## In brief

Tools give an agent actions; MCP servers expose external tool surfaces; sandboxes isolate execution environments. They are related capabilities with different ownership and policy boundaries.

## Ownership

An `McpServer` describes a namespaced endpoint, optional auth broker, and tool allowlist. Agent MCP references resolve by namespace ancestry, with the nearest same-named server winning. A SandboxClass identifies a provider, a SandboxPolicy selects the class and runtime template, and a Sandbox records the realized lease and processes.

## Lifecycle

Workers resolve permitted tools for a turn, then apply server policy and agent capability policy. Sandbox policies control how isolated runtimes are leased and run; ACP-backed agents use those policies and their permission configuration rather than treating a container as an ambient tool.

## Relationships

MCP is an external tool protocol, while a sandbox is execution isolation. [Identity, Access, and Secrets](../boundary-and-policy-model/identity-access-and-secrets) distinguishes Talon access from tool-specific authentication. Setup and provider details belong in [Operate](../../operate).

## Read next

- [Agents and Runtimes](../execution-model/agents-and-runtimes)
- [Identity, Access, and Secrets](../boundary-and-policy-model/identity-access-and-secrets)
