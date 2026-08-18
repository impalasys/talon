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

## Relationships

Channels can route a public message into an agent-owned Session. Schedules can target a Session, and delegated Tasks can point to child execution. Those mechanisms initiate or relate work; they do not replace the session's transcript.

## Read next

- [Execution and Recovery](./execution-and-recovery)
- [Channels and Subscriptions](../coordination-model/channels-and-subscriptions)
