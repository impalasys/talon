---
title: Coordination Model
sidebar:
  order: 4
  hidden: true
---

Talon has several durable coordination primitives. Choose one by the kind of work relationship you need, not by which API happens to be convenient.

## Choosing a primitive

| Primitive | Durable state | Use it for |
| --- | --- | --- |
| Session | One agent interaction | A conversational or agent-driven execution record |
| Schedule | A timed trigger | Starting work later or repeatedly |
| Channel | Shared message history | Public collaboration that routes into agent sessions |
| Workflow | A declared run graph | Predictable multi-step orchestration |
| Task | Owner/delegate responsibility | Accountable delegated work and review |

## Read in order

1. [Schedules and Background Work](/talon/docs/concepts/coordination-model/schedules-and-background-work)
2. [Channels and Subscriptions](/talon/docs/concepts/coordination-model/channels-and-subscriptions)
3. [Workflows and Runs](/talon/docs/concepts/coordination-model/workflows-and-runs)
4. [Tasks and Delegation](/talon/docs/concepts/coordination-model/tasks-and-delegation)

Use the table above to jump directly to the primitive that matches the coordination problem.
