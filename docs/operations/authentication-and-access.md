---
title: Authentication and Access
sidebar_position: 2
---

Talon supports multiple gateway authentication modes.

## Gateway auth modes

The gateway can run in one of these modes:

- **open**: no auth configured
- **password**: Basic auth with a shared password
- **token**: bearer token auth
- **JWT**: bearer JWTs with optional namespace, agent, and session scoping

At startup, the server chooses auth from environment in this order:

1. `GATEWAY_JWT_SECRET`
2. `GATEWAY_TOKEN`
3. `GATEWAY_PASSWORD`
4. open mode

## JWT scoping

JWTs can restrict access to:

- a namespace
- an agent
- a session

That makes JWT mode the most expressive option for browser or delegated access.

## CLI auth

`talon-cli` supports:

- `--password`
- `--token`
- `--jwt-secret`

It can also target either:

- native gRPC by default
- REST-transcoded endpoints with `--rest`

## Browser and UI access

Browser-oriented access still terminates at the gateway. Sightline and similar clients are not a separate control plane.

## Read next

- [Runtime Surfaces](../reference/runtime-surfaces.md)
- [CLI](../reference/cli.md)
