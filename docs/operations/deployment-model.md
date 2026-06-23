---
title: Deployment Model
sidebar_position: 2
---

Talon’s deployment surface in this repo is intentionally split:

- browser UI and docs surface
- gateway and worker runtime
- scheduler and control-plane dependencies

## Runtime deployment

The runtime is centered on:

- the gateway server
- the worker
- the control plane backing services

## Local deployment shape

The local stack in this repo uses Docker Compose to start:

- gateway
- worker
- Postgres
- Pub/Sub emulator
- UI
- manifest bootstrap

## Documentation source

Documentation source in this repository lives under `talon/docs`.

Generated reference pages are produced from the proto definitions and checked into `talon/docs/reference/generated` so API and schema changes remain reviewable in version control.

## Edge and API surfaces

In practice Talon exposes:

- native gRPC on the gateway
- gRPC-Web on the same gateway port for browser clients
- SDK clientsets over the named `talon.v1` services

## Forwarded headers

Production deployments that place the gateway behind a reverse proxy or edge service should strip untrusted client-supplied `x-forwarded-*` headers and then set trusted forwarded headers themselves.

## Read next

- [Runtime Topology](../concepts/runtime-topology.md)
- [Runtime Surfaces](../reference/runtime-surfaces.md)
