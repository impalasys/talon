---
title: Deployment Model
sidebar:
  order: 2
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
- Cloud Run Worker Pools do not have a load-balanced URL. To register each instance's ephemeral private VPC address, opt in with `TALON_WORKER_ENDPOINT_DISCOVERY=cloud_run_worker_pool`. Talon reads the instance IPv4 address from the Google metadata server and registers `http://<private-ip>:<PORT>`; `PORT` defaults to `8081`.
- `TALON_WORKER_ENDPOINT_URL` always takes precedence. `TALON_WORKER_PUBLIC_URL` and `TALON_WORKER_URL` remain compatible explicit overrides. With Cloud Run Worker Pool discovery enabled, the metadata address takes precedence over `CLOUD_RUN_SERVICE_URL`; a Cloud Run service URL is not a per-worker endpoint.

Cloud Run Worker Pool requirements:

- Configure Direct VPC ingress so the worker has a private VPC address and can receive traffic on its listening port.
- The Talon gateway must have VPC reachability to the Worker Pool subnet and routes for the worker addresses.
- Allow the gateway's source range or identity in the worker firewall rules for the worker listener port (normally `8081`, or the configured `PORT`). Do not expose the worker listener publicly for this discovery mode.
- Worker Pool addresses are ephemeral and can change whenever an instance restarts. Each new instance registers its current address; heartbeat TTL expiry removes stale registrations.

Minimal worker configuration:

```bash
TALON_WORKER_ENDPOINT_DISCOVERY=cloud_run_worker_pool
PORT=8081
```

Deploy the worker with Cloud Run Worker Pool Direct VPC ingress, and deploy the gateway on a network that can route to the Worker's VPC addresses. Do not set `CLOUD_RUN_SERVICE_URL` as the worker endpoint. Set `TALON_WORKER_ENDPOINT_URL` only if it intentionally names another reachable, per-worker endpoint.

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

Generated reference pages are produced from the proto definitions and checked into `talon/docs/05-reference/generated` so API and schema changes remain reviewable in version control.

## Edge and API surfaces

In practice Talon exposes:

- native gRPC on the gateway
- gRPC-Web on the same gateway port for browser clients
- SDK clientsets over the named `talon.v1` services

## Forwarded headers

Production deployments that place the gateway behind a reverse proxy or edge service should strip untrusted client-supplied `x-forwarded-*` headers and then set trusted forwarded headers themselves.

## Read next

- [Runtime Topology](../concepts/runtime-topology)
- [Runtime Surfaces](../reference/runtime-surfaces)
