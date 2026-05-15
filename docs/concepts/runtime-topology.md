---
title: Runtime Topology
sidebar_position: 2
---

This page explains the local stack Talon starts and how the pieces fit together.

## Local stack

Running `./run.sh` inside `talon/` starts:

- `ui` on `http://localhost:3000`
- `envoy` on `http://localhost:18789`
- `gateway` gRPC on `http://localhost:50051`
- `gateway` UI HTTP on `http://localhost:50052`
- `worker`
- `postgres`
- `pubsub` emulator
- `init-manifests` bootstrap for the default template

## Traffic paths

### Browser / operator path

The browser typically talks to:

- the Next.js Sightline UI on `3000`
- the Envoy edge on `18789`

Envoy then routes:

- REST-transcoded gateway routes
- browser-facing UI session routes

### Native integration path

Backend or service integrations can talk directly to the gateway gRPC server on `50051`.

That is the canonical contract.

### UI session path

Sightline-style session clients use the gateway UI HTTP surface on `50052`, which exposes browser-oriented session interactions and streams.

## Execution path

When a session message is submitted:

1. the gateway persists the request and publishes a dispatch event
2. the worker receives the event from Pub/Sub
3. the worker executes the turn and publishes step events
4. the gateway and UI can read the resulting session state

## Bootstrap behavior

The local compose stack also applies `manifests/default_agent.yaml` after the gateway becomes healthy. That means the local environment starts with at least one usable agent template already loaded.

## Read next

- [Resource Model](./resource-model.md)
- [Quickstart](../getting-started/quickstart.md)
- [Scheduling and Background Work](../operations/scheduling-and-background-work.md)
