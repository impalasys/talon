---
title: Agents and Templates
sidebar_position: 1
---

Talon separates reusable agent design from runtime instances so behavior can be standardized without losing runtime flexibility.

## Agent templates

Agent templates describe a reusable base:

- system prompt
- feature set
- model policy
- MCP server references
- capability policy

Templates let you standardize a class of agent and then apply targeted overrides later.

In practice, templates are where platform or product teams encode the safe default shape of an agent.

## Agents

Agents are the runtime-facing resources created inside a namespace. An agent can be:

- fully custom
- derived from a template with deltas

The effective agent spec is what ultimately drives runtime behavior.

That effective spec is what the worker sees when it executes a turn.

## Template deltas

When an agent is templated, it can still override parts of the base template:

- model policy
- system prompt
- features
- MCP server references
- capabilities

The important point is that Talon treats these overrides as structured deltas rather than unstructured prompt editing.

## Why this split matters

This gives Talon a real control-plane model rather than “just prompt text”:

- templates can be curated centrally
- agents can inherit and override safely
- policies become explicit and reviewable

## Where these show up operationally

- templates are often bootstrapped or curated globally
- agents are created inside namespaces
- sessions are always created against an agent, never directly against a template

See the generated [resource schema reference](../05-reference/generated/resource-schemas.md) for the exact message shapes.

## Read next

- [Resource Model](./03-resource-model.md)
- [Sessions and Streaming](./05-sessions-and-streaming.md)
