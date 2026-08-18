---
title: Usage and Limits
sidebar:
  order: 4
---

## In brief

A UsagePolicy applies hard, rolling resource limits to matching Talon work. It protects runtime capacity; it is not an authorization or billing system.

## Ownership

A policy selects traffic by namespace scope and can narrow by agent, provider, model, or identity. Limits cover configured metrics such as LLM requests and tokens, successful session creation, and tool calls. Status reports the active window, usage, remaining capacity, reset time, and whether a limit has been exceeded.

## Lifecycle

Before admitting matching work, Talon evaluates the applicable policies and atomically charges usage. Limits can apply to one namespace or recursively to descendants, and can be shared across traffic or partitioned by identity.

## Relationships

UsagePolicy constrains consumption after [Identity, Access, and Secrets](./identity-access-and-secrets) permits an action. It is separate from an Agent model policy, which chooses model behavior rather than enforcing a quota.

## Read next

- [Identity, Access, and Secrets](./identity-access-and-secrets)
- [Resources and Reconciliation](../system-model/resources-and-reconciliation)
