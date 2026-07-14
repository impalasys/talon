---
title: Using Sightline
sidebar:
  order: 6
---

Sightline is Talon’s browser-native operator UI.

## What Sightline is for

Use Sightline when you want to:

- inspect namespaces, agents, schedules, and sessions
- create or observe live sessions
- debug tool activity and streamed execution
- verify what the runtime is actually doing

## How it fits the system

Sightline is not a separate control plane. It is a UI over Talon’s real runtime surfaces.

In local development:

- the UI runs on `http://localhost:3000`
- it talks through the direct gateway on `http://localhost:50051`
- the gateway serves a browser-oriented session API behind that path

## Why it matters

Talon is designed to be observable. Sightline is the fastest way to see:

- persisted messages
- live execution steps
- tool starts and tool results
- whether schedules and sessions are behaving as expected

## Read next

- [Quickstart](../getting-started/quickstart)
- [Runtime Surfaces](../reference/runtime-surfaces)
