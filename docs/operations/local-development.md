---
title: Local Development
sidebar_position: 1
---

## Core loop

From `talon/`:

```bash
./run.sh
```

This starts the local compose stack and brings up:

- the gateway
- the worker
- Envoy
- Sightline UI
- Postgres
- the Pub/Sub emulator
- the default template bootstrap

## Useful endpoints

- Gateway edge: `http://localhost:18789`
- Sightline UI: `http://localhost:3000`

## Common tasks

- Inspect the [gateway service reference](../reference/generated/gateway-service.md) when adding or consuming API surface
- Use Sightline to verify sessions, schedules, namespaces, and tool activity
- Use the CLI for admin flows that are easier from the terminal than the UI

## Useful runtime ports

- `3000`: Sightline UI
- `18789`: Envoy edge surface
- `50051`: native gRPC gateway
- `50052`: gateway UI HTTP surface

## Docs workflow

- Hand-written docs live in `docs/`.
- Generated reference pages live in `docs/reference/generated/`.
- If you change the gateway or schema protos, regenerate the reference pages with `pnpm --filter @impalasys/talon-docs generate:reference`.
- Use the docs markdown itself as the source of truth for this open-source repository.
