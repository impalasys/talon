---
title: Sessions and Submissions
sidebar:
  order: 2
---

## In brief

A Session is the durable interaction record for one agent. A Submission is a durable request for the session to perform work, such as a user turn or maintenance action.

## Ownership

Sessions retain identity, lifecycle state, messages, execution history, and labels. Submissions retain the requested action and allow Talon to claim, execute, and commit work safely. The session owns an agent interaction; it is not a shared Channel, a WorkflowRun, or a delegated Task.

## Lifecycle

The gateway persists a submitted turn before dispatching it. A worker claims runnable work, produces journal and message-part output, then commits canonical messages and session state. Idempotent submission and durable state let work be inspected or resumed after a client disconnects.

## Message storage limits

Workers cap each encoded assistant SessionMessage at 350 KiB by default, leaving
headroom for DynamoDB's item metadata. `TALON_SESSION_MESSAGE_MAX_BYTES` overrides
the positive byte limit and is captured once when a submission's sink is created.
Choose a limit compatible with the storage backend; zero or invalid values use
the default.

If a streaming projection, semantic boundary, or final message exceeds the limit,
the worker stops accepting further sink output, stores a compact FAILED error
projection, and marks the submission failed. Final persistence failures also
release the session as ERROR instead of reporting successful completion. If the
backend rejects even the compact error, the worker still attempts to mark the
submission failed and reports an error to connected clients. A failed post-commit
projection update preserves the already committed recovery journal.

## Relationships

Channels can route a public message into an agent-owned Session. Schedules can target a Session, and delegated Tasks can point to child execution. Those mechanisms initiate or relate work; they do not replace the session's transcript.

## Read next

- [Execution and Recovery](./execution-and-recovery)
- [Channels and Subscriptions](../coordination-model/channels-and-subscriptions)
