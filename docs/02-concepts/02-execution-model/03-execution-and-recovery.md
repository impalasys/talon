---
title: Execution and Recovery
sidebar:
  order: 3
---

## In brief

Talon makes execution durable before it makes it live. Streaming is a projection of work in progress; persisted session state is the record that remains afterward.

## Ownership

The gateway validates and persists intent. Workers claim session work, resolve the agent and context, execute model and tool steps, and write journal entries and message parts. The worker owns execution, while the gateway owns the public service boundary.

## Lifecycle

The usual sequence is gateway persistence, dispatch, worker claim, execution and journal writes, then an atomic canonical commit or terminal failure. A lease and durable journal let Talon recover or reclaim interrupted work. Session compaction and steering are durable session operations, not client-only stream behavior.

## Relationships

`SessionMessagePartEvent` exposes live deltas, completion, and errors. Persisted messages and session state can be read later even when a stream was missed. Exact stream methods and event shapes belong in [Runtime Surfaces](../../reference/runtime-surfaces) and [Events and Models](../../reference/events-and-models).

## Read next

- [Gateway and Clients](../boundary-and-policy-model/gateway-and-clients)
- [Sessions and Submissions](./sessions-and-submissions)
