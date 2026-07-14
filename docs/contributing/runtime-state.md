---
title: Runtime State Design
sidebar_position: 2
---

Talon has several persistence layers. New runtime features should choose the
smallest layer that matches the product semantics, and pull requests should make
that choice visible.

## Session replay and side effects

Model tool calls are replayed through the session submission journal. A native
tool should not add its own idempotency keys, claim records, or deterministic
resource names just to survive worker redelivery.

Use the journal when the question is:

- Did this submitted LLM tool call already produce a tool result?
- Can recovery rebuild a committed message without executing the tool again?
- Can a stale worker be fenced from appending more results?

Use the domain resource itself when the question is:

- What durable object did the user or agent create?
- What state should an operator, CLI, workflow, or another agent inspect?
- What events should resource watchers observe?

For example, `delegate_task` creates a normal `Task` and child `Session`. If a
worker crashes after the tool result is journaled, recovery should reuse the
journaled result. The `Task` layer should not maintain a separate
tool-call-to-task mapping.

## Session inbox limitation

Today, sending a message to a session is also a request to drive that session.
If the target session is already `PROCESSING`, `send_message` rejects instead of
durably appending the message to a pending inbox.

That means runtime code cannot currently say: "append this message now, process
it after the current turn commits." Until Talon has that primitive, features
that need to notify a busy session must either fail visibly or arrange a retry
from an existing durable transition.

Delegated Task owner wakeups use that second option as a stopgap:

- the delegate session finishes and updates the `Task` to `NEEDS_REVIEW` or
  `FAILED`
- Talon tries to send a wake message to the owner session
- if the owner session is busy, the `Task` records `OwnerWake=False`
- when any session releases its lock, Talon checks whether that released
  session is the owner for delegated Tasks with `OwnerWake=False`
- matching Tasks retry the owner wake from the release path

This is intentionally not a general queue, not a hidden KV side table, and not
the long-term inbox design. The long-term fix is a session inbox where
`AppendMessage` can persist an inbound message and pending submission while a
session is busy, and the worker claims the next pending submission when the
current turn releases.

## Hidden KV state

Avoid hidden KV records when a first-class resource, child record, label, or
session journal entry can express the relationship. Hidden KV writes are harder
to discover, harder to clean up, and easy to miss in code review.

If a feature really needs hidden KV state, the pull request should explain:

- the exact key shape and owner
- why a resource, label, or journal entry is insufficient
- how stale records expire or are deleted
- how tests prove replay, retry, and cleanup behavior

## Resource names and indexes

Resource names should be stable, readable enough for operators, and compatible
with the resource API. Do not encode private lookup indexes into resource names
unless the user-facing identity truly requires it.

If a feature needs efficient lookup by a second dimension, prefer one of:

- labels on the resource when eventual list-and-filter behavior is acceptable
- a documented index record owned by that resource type
- a storage-level query capability added deliberately to the resource store

Do not add ad hoc hash names or side tables only to make one tool call easier.
