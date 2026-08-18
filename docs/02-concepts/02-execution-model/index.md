---
title: Execution Model
sidebar:
  order: 2
  hidden: true
---

Talon separates an agent's declared behavior from its durable interaction state and live execution.

## Relationship map

An Agent supplies runtime behavior. A Session supplies durable interaction state. A Submission records requested work, and a worker claims it, writes a journal, and commits canonical session state while clients observe live parts.

## Read in order

1. [Agents and Runtimes](/talon/docs/concepts/execution-model/agents-and-runtimes)
2. [Sessions and Submissions](/talon/docs/concepts/execution-model/sessions-and-submissions)
3. [Execution and Recovery](/talon/docs/concepts/execution-model/execution-and-recovery)

Start with Sessions and Submissions when you need to distinguish a session from a channel, workflow run, or task.
