---
title: Sessions and Execution
sidebar_position: 2
---

Sessions are the durable runtime unit for interaction with an agent.

## Sessions

A session tracks:

- the target namespace and agent
- lifecycle state
- timestamps and labels
- accumulated messages and execution history

Persisted messages are not the whole story. A session also accumulates execution steps, which are what make Talon observable.

## Messaging flow

The common flow is:

1. create a session
2. send a message
3. observe streamed steps while the worker executes
4. fetch or resume session state later

Under the hood, the gateway persists intent and the worker performs execution.

## Streaming surfaces

Talon supports multiple consumption patterns:

- native gRPC streaming via `SessionService.StreamParts`
- browser gRPC-Web streaming via `SessionService.SubmitTurn`
- session history reads via `SessionService.ListMessages`

The important distinction is:

- gRPC is the system-of-record contract
- gRPC-Web is the same contract adapted for frontend clients

## Tool visibility

Session streams are not only text streams. They also expose:

- tool call starts
- tool results
- persisted execution steps

This is what makes Sightline useful as an operator/debugging surface rather than a plain chat UI.

## Session state vs stream state

It helps to separate two views of the same interaction:

- **persisted session state** is what you can fetch later with `SessionService.Get`
- **stream state** is what you observe live while execution is still in progress

Talon gives you both because durable auditability and live UX are different needs.

## Read next

- [Using Sightline](./using-sightline.md)
- [Runtime Surfaces](../reference/runtime-surfaces.md)
- [Events and Models](../reference/events-and-models.md)
