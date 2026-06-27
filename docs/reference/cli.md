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

All token commands accept `--subject`, `--ttl <duration>`, repeatable `--origin
<origin>` flags, and `--ttl-seconds <seconds>` retained for scripts. The
default token TTL is `5min`; examples include `1wk`, `3mo`, and `1yr`. Origins
are serialized into the `talon:origins` claim and require A2A REST or gRPC-Web
browser requests to carry a matching `Origin` header. Native gRPC ignores the
claim.

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
- The CLI talks to the gateway RPC surface directly. Use native gRPC by default, and `--grpc-web` only when an HTTP proxy exposes the gRPC-Web gateway path.
- For service-to-service integrations, prefer the generated gateway contracts directly.
