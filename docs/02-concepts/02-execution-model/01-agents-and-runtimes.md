---
title: Agents and Runtimes
sidebar:
  order: 1
---

## In brief

An Agent is the runtime target for a session. Its specification declares the behavior and capabilities that a worker resolves when it performs a turn.

## Ownership

An Agent can declare model policy, prompt and feature configuration, capability policy, MCP references, A2A connections, and a runtime kind. Native agent execution and ACP-backed execution have different runtime requirements, but both run through Talon's durable session model.

## Lifecycle

The gateway stores the Agent resource. When a session needs work, the worker resolves the agent's effective configuration and allowed capabilities for that execution. Changes to an Agent affect later work; a persisted session remains the record of work that already occurred.

## Relationships

Agents are not derived from Templates. Templates and Deployments are declarative resource fan-out mechanisms. Agents use [Skills](../context-and-capability-model/skills) and [Tools, MCP, and Sandboxes](../context-and-capability-model/tools-mcp-and-sandboxes) to acquire runtime capability.

## Read next

- [Sessions and Submissions](./sessions-and-submissions)
- [Templates and Deployments](../system-model/templates-and-deployments)
