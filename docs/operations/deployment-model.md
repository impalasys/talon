---
title: Deployment Model
sidebar_position: 2
---

Talon’s deployment surface in this repo is intentionally split:

- browser UI and docs surface
- gateway and worker runtime
- edge routing through Envoy
- scheduler and control-plane dependencies

## Runtime deployment

The runtime is centered on:

- the gateway server
- the worker
- the control plane backing services
- edge routing through Envoy

## Local deployment shape

The local stack in this repo uses Docker Compose to start:

- gateway
- worker
- Postgres
- Pub/Sub emulator
- Envoy
- UI
- manifest bootstrap

## Documentation source

Documentation source in this repository lives under `talon/docs`.

Generated reference pages are produced from the proto definitions and checked into `talon/docs/reference/generated` so API and schema changes remain reviewable in version control.

## Edge and API surfaces

In practice Talon exposes:

- native gRPC on the gateway
- REST-transcoded HTTP through Envoy
- browser-oriented UI session routes for Sightline-style clients

## Read next

- [Runtime Topology](../concepts/runtime-topology.md)
- [Runtime Surfaces](../reference/runtime-surfaces.md)
