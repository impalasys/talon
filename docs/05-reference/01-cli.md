---
title: CLI
sidebar_position: 2
---

`talon-cli` is the administrative entry point for common control-plane tasks.

## Global flags

- `--gateway`: gRPC gateway address, default `http://localhost:50051`
- `--token`: bearer token
- `--api-key`: Talon API key to exchange for a short-lived bearer token
- `--grpc-web`: use gRPC-Web over HTTP/1.1 instead of native gRPC

## Commands

### `auth`

Authenticate to the gateway or, for local environments only, mint a
platform-signed bootstrap token from a private PEM file. API key and OIDC
exchanges return platform-signed Talon access tokens when
the required `TALON_JWT_PRIVATE_KEY_PEM` platform signing key is present. JWT
`iss` defaults to `https://talon.impala.systems` and can be overridden with
`TALON_JWT_ISSUER`.

- `auth login`: sign in through Google OIDC and store a Talon access token
- `auth logout`: remove stored CLI auth
- `auth whoami`: show stored CLI auth
- `auth local-token --private-key-pem-file <path>`: local-only RS256 platform access token
- `auth api-key create --name <name> --grant <grant>`: create an API key
- `auth api-key list`: list API keys
- `auth api-key revoke <id>`: revoke an API key

`auth local-token` accepts `--subject`, `--ttl <duration>`, repeatable
`--origin <origin>` flags, optional namespace/agent/session/channel scopes,
optional `--grant` entries, and `--ttl-seconds <seconds>` retained for scripts.
The default token TTL is `5min`; examples include `1wk`, `3mo`, and `1yr`.
Origins are serialized into the `talon:origins` claim and require A2A REST or
gRPC-Web browser requests to carry a matching `Origin` header. Native gRPC
ignores the claim.

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

## Examples

Inspect an agent:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 get agent main --namespace default
```

Apply the default local manifest:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/default
```

## Notes

- The CLI is best thought of as an operator/admin tool, not the only integration surface.
- The CLI talks to the gateway RPC surface directly. Use native gRPC by default, and `--grpc-web` only when an HTTP proxy exposes the gRPC-Web gateway path.
- For service-to-service integrations, prefer the generated gateway contracts directly.
