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

## Worker endpoint registration

Workers publish a `Worker` resource in the Talon system namespace and refresh its status on a heartbeat. When the worker has an address that another Talon component can call, it includes that address in `status.endpoints`.

Endpoint discovery is override-first. Set `TALON_WORKER_ENDPOINT_URL` only when it names the specific worker instance that is registering. Optional `TALON_WORKER_ENDPOINT_AUDIENCE` and `TALON_WORKER_ENDPOINT_PROTOCOL` values are copied into the registered endpoint.

Platform behavior:

- AWS ECS tasks with `ECS_CONTAINER_METADATA_URI_V4` register the task's first IPv4 address and the worker `PORT`.
- Cloud Run worker pools do not have a load-balanced endpoint. Use `TALON_WORKER_ENDPOINT_URL` only when you intentionally front the worker with another reachable service.

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
