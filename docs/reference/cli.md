---
title: CLI
sidebar_position: 2
---

`talon-cli` is the administrative entry point for common control-plane tasks.

## Global flags

- `--gateway`: gRPC gateway address, default `http://localhost:50051`
- `--password`: basic-auth password
- `--token`: bearer token
- `--jwt-secret`: shared JWT secret for short-lived admin tokens
- `--grpc-web`: use gRPC-Web over HTTP/1.1 instead of native gRPC

## Commands

### `auth`

Authenticate to the gateway or mint JWTs for clients when the gateway is
running with `GATEWAY_JWT_SECRET`.

- `auth login`: sign in through Google OIDC and store a Talon access token
- `auth logout`: remove stored CLI auth
- `auth whoami`: show stored CLI auth
- `auth root-token`: unrestricted root token
- `auth agent-token --namespace <ns> --agent <agent>`: namespace and agent scoped token
- `auth session-token --namespace <ns> --agent <agent> --session <session-id>`: namespace, agent, and session scoped token
- `auth channel-token --namespace <ns> --channel <channel>`: namespace and channel scoped token

All token commands accept `--subject` and `--ttl-seconds`.

`auth login` accepts `--google-client-id` and `--google-client-secret`, with
environment fallbacks `TALON_GOOGLE_CLIENT_ID` and
`TALON_GOOGLE_CLIENT_SECRET`.

### `knowledge`

Manage namespace knowledge artifacts directly by path.

- `knowledge get`
- `knowledge set`
- `knowledge delete`
- `knowledge sync`

### `apply`

Apply a manifest file, optionally with template variables.

### `render`

Render a manifest file after template substitution in YAML or JSON.

### `get`

Fetch a resource by kind, name, and optional namespace.

### `delete`

Delete a resource by kind, name, and optional namespace.

### `gen`

Generate a TypeScript client SDK from manifest files.

## Notes

- The CLI is best thought of as an operator/admin tool, not the only integration surface.
- The CLI talks to the gateway RPC surface directly. Use native gRPC by default, and `--grpc-web` for Cloudflare-backed gateways that do not expose native gRPC.
- For service-to-service integrations, prefer the generated gateway contracts directly.
