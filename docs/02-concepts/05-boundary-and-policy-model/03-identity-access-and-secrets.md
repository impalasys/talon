---
title: Identity, Access, and Secrets
sidebar:
  order: 3
---

## In brief

Identity establishes who is acting; access grants establish which Talon resources they may use; Secrets provide sensitive values without placing them in ordinary resource specifications.

## Ownership

Talon supports OIDC, JWT-backed access tokens, and API-key exchange at its gateway boundary. Grants can be scoped to Talon resources and browser origins. Secret resources and references keep sensitive configuration separate from normal manifests. MCP auth brokers serve a distinct tool-facing authentication boundary.

## Lifecycle

An external identity or API key is exchanged for Talon access. The gateway evaluates grants before accepting a request. Delegated access can narrow authority for downstream work. Secrets are resolved only by the runtime paths authorized to consume them.

## Relationships

Authorization answers whether work is allowed; [Usage and Limits](./usage-and-limits) answers whether an allowed request has remaining resource budget. OIDC configuration and key-management commands belong in [Operate](../../operate/authentication-and-access).

## Read next

- [Usage and Limits](./usage-and-limits)
- [Tools, MCP, and Sandboxes](../context-and-capability-model/tools-mcp-and-sandboxes)
