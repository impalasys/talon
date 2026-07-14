---
title: Runtime Topology
sidebar_position: 2
---

This page explains the local stack Talon starts and how the pieces fit together.

## Local stack

Running `docker compose up --build -d` from the repository root starts:

- `ui` on `http://localhost:3000`
- `gateway` gRPC and gRPC-Web on `http://localhost:50051`
- `worker`
- `postgres`
- `pubsub` emulator
- `init-manifests` bootstrap for the default namespace and agent

This default topology is Postgres-backed. A smaller same-host deployment can also run Talon with a local SQLite control-plane database instead of Postgres.

## Traffic paths

### Browser / operator path

The browser typically talks to:

- the Next.js Sightline UI on `3000`
- the gateway gRPC-Web endpoint on `50051`

### Native integration path

Backend or service integrations can talk directly to the gateway gRPC server on `50051`.

That is the canonical contract.

## Execution path

When a session message is submitted:

1. the gateway persists the request and publishes a dispatch event
2. the worker receives the event from Pub/Sub
3. the worker executes the turn and publishes step events
4. the gateway and UI can read the resulting session state

## Bootstrap behavior

The local compose stack also applies `manifests/default` after the gateway becomes healthy. That means the local environment starts with `Namespace/default`, `Template/default`, and `Agent/default/main` already loaded.

## Read next

- [Resource Model](./03-resource-model.md)
- [Quickstart](../01-getting-started/01-quickstart.md)
- [Scheduling and Background Work](../04-operations/05-scheduling-and-background-work.md)
