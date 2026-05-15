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

## Docs preview

To preview the landing site and docs together:

```bash
cd talon/site
pnpm build
```

The published `/docs` routes are rendered by the Astro site from `talon/docs`.
