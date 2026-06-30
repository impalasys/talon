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
- one or more browser origins

JWTs without resource scope are root tokens and can access the gateway wherever JWT auth is accepted. Namespace tokens can access one namespace and its child namespaces. Agent and session tokens include namespace scope; session tokens also include agent scope so a session id is not accidentally reusable across agents.

That makes JWT mode the most expressive option for browser or delegated access.

Talon-issued JWTs can also carry structured `grants`. New tokens use the
`grants` claim, and Talon continues to accept legacy `talon:grants`. Grants
express `read` or `readwrite` access with optional namespace, agent, session, or
channel selectors. When grants are present, Talon treats them as the
authoritative authorization scope.

## API keys

API keys are long-lived credentials for trusted server-side clients. They are
not accepted on normal gateway APIs. Instead, clients exchange an API key with
`AuthService.ExchangeApiKey` and receive a short-lived Talon JWT whose grant is
the same as, or narrower than, the API key's stored grants.

API key lifecycle management is root-only. A root JWT can create, list, and
revoke keys through `AuthService`; scoped JWTs, OIDC grant JWTs, and Basic auth
cannot manage API keys. Revocation stops future exchanges immediately, while
already-issued JWTs remain valid until their short expiry.

Use API keys only in trusted backends, CLIs, workers, or CI jobs. Browser clients
should receive short-lived, origin-scoped JWTs from a backend instead of holding
API keys.

Origin-scoped JWTs include `talon:origins`, for example
`["https://app.example.com"]`. When present, browser-facing gateway paths
require the request to include a matching `Origin` header. Talon enforces this
for A2A REST and gRPC-Web requests; native gRPC ignores the claim because
non-browser clients can set arbitrary origin metadata. This is intended to make
browser-exposed tokens harder to reuse from another website; keep using short
TTLs and resource scopes.

## CLI auth

`talon-cli` supports:

- `--password`
- `--token`
- `--jwt-secret`

Use the `auth` command to mint scoped tokens from `TALON_JWT_SECRET`, `GATEWAY_JWT_SECRET`, or `--jwt-secret`:

- `auth root-token`
- `auth namespace-token --namespace <ns>`
- `auth agent-token --namespace <ns> --agent <agent>`
- `auth session-token --namespace <ns> --agent <agent> --session <session-id>`
- `auth channel-token --namespace <ns> --channel <channel>`
- `auth api-key create --name <name> --grant readwrite,namespace=<ns>`
- `auth api-key list`
- `auth api-key revoke <id>`

All token commands accept `--ttl <duration>` and repeatable `--origin <origin>`
flags. The default TTL is `5min`; examples include `1wk`, `3mo`, and `1yr`.
`--ttl-seconds <seconds>` remains available for scripts. Origin flags add
`talon:origins` to the minted JWT.

Normal CLI commands also accept `--api-key` or `TALON_API_KEY`; the CLI exchanges
the key for a cached short-lived JWT before calling the gateway.

The CLI targets the gateway RPC surface directly. It uses native gRPC by default; pass `--grpc-web` only when an HTTP proxy exposes the gRPC-Web gateway path. Browser-oriented clients should use the gRPC-Web-compatible gateway path.

## Browser and UI access

Browser-oriented access still terminates at the gateway. Sightline and similar clients are not a separate control plane.

## OIDC trust grants

Talon can declare trusted OIDC issuers in `talon.yaml` and map accepted
identities to Talon grants:

```yaml
trust:
  oidc:
    - name: google-admins
      issuer: https://accounts.google.com
      audiences:
        - talon-google-web-client-id.apps.googleusercontent.com
        - talon-google-desktop-client-id.apps.googleusercontent.com
      allowedDomains:
        - impala.systems
      allowedEmails: []
      clockSkewSeconds: 60
      grants:
        - kind: readwrite
```

Grant kinds are:

- `read`: allows read-style gateway operations such as get, list, stream, and search.
- `readwrite`: allows read plus mutating operations such as create, delete, send, append, stop, resume, and cancel.

Selectors narrow a grant:

- no selectors: global gateway access
- `namespace`: everything in one namespace
- `namespace` + `agent`: one agent surface
- `namespace` + `agent` + `session`: one session surface
- `namespace` + `channel`: one channel surface

OIDC trust entries do not contain OAuth client secrets. Sightline web SSO uses
`TALON_GOOGLE_WEB_CLIENT_ID` and `TALON_GOOGLE_WEB_CLIENT_SECRET` at the gateway
to enable the Google sign-in button.

The CLI uses a Google Desktop OAuth client with loopback redirect and PKCE.
Official release builds can inject Talon's default Desktop OAuth client at build
time. Source builds should provide `TALON_GOOGLE_CLIENT_ID` or
`talon-cli auth login --google-client-id ...`.

Google Desktop OAuth clients are native-app clients and cannot keep secrets in
the OAuth security sense, but Google's token endpoint can still require the
Desktop client secret issued with that client. Official release builds can
inject the matching Desktop client secret at build time; it is not an
authorization boundary. Source builds should provide the matching Desktop client
secret with `TALON_GOOGLE_CLIENT_SECRET`, `TALON_GOOGLE_CLI_CLIENT_SECRET`, or
`talon-cli auth login --google-client-secret ...`. Do not use a Google Web OAuth
client secret for CLI login.

## Read next

- [Runtime Surfaces](../reference/runtime-surfaces.md)
- [CLI](../reference/cli.md)
