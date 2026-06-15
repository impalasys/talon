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

## Forwarded headers

The gateway uses `x-forwarded-proto` and `x-forwarded-host` when constructing public URLs in REST responses such as A2A Agent Cards. Production deployments must place the gateway behind a trusted reverse proxy or edge service that strips untrusted client-supplied `x-forwarded-*` headers and then sets the forwarded headers itself.

Do not directly expose the gateway UI HTTP surface to untrusted clients unless the surrounding infrastructure sanitizes these headers first.

## Read next

- [Runtime Topology](../concepts/runtime-topology.md)
- [Runtime Surfaces](../reference/runtime-surfaces.md)
