---
title: Tasks and Delegation
sidebar:
  order: 4
---

## In brief

A Task is durable responsibility for work owned by one party and delegated to another. It connects progress, artifacts, and review to an execution reference.

## Ownership

The Task records its title and description, owner, intended delegate, lifecycle phase, progress summary, artifacts, and any linked execution. Agent delegation uses declared internal A2A connections to create linked work without losing responsibility for the result.

## Lifecycle

A Task moves from queued work into active execution and can become blocked or need review before reaching a terminal state. Delegated work can create a child session, return progress or artifacts, and wake the owner to inspect failure or completion.

## Relationships

A Task is not generic human task tracking and is not a Workflow DAG. Use a Workflow for declared composition, a Session for an agent interaction, and a Task when ownership and delegation are the central concern.

## Read next

- [Workflows and Runs](./workflows-and-runs)
- [Agents and Runtimes](../execution-model/agents-and-runtimes)
