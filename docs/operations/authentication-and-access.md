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
- **JWT**: bearer JWTs with optional namespace, agent, session, and channel scoping

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
- a channel

JWTs without resource scope are root tokens and can access the gateway wherever JWT auth is accepted. Agent and session tokens include namespace scope; session tokens also include agent scope so a session id is not accidentally reusable across agents.

That makes JWT mode the most expressive option for browser or delegated access.

## CLI auth

`talon-cli` supports:

- `--password`
- `--token`
- `--jwt-secret`

Use the `auth` command to mint scoped tokens from `TALON_JWT_SECRET`, `GATEWAY_JWT_SECRET`, or `--jwt-secret`:

- `auth root-token`
- `auth agent-token --namespace <ns> --agent <agent>`
- `auth session-token --namespace <ns> --agent <agent> --session <session-id>`
- `auth channel-token --namespace <ns> --channel <channel>`

It can also target either:

- the gateway's native gRPC listener
- Envoy's native gRPC ingress

## Browser and UI access

Browser-oriented access still terminates at the gateway. Sightline and similar clients are not a separate control plane.

## Read next

- [Runtime Surfaces](../reference/runtime-surfaces.md)
- [CLI](../reference/cli.md)
