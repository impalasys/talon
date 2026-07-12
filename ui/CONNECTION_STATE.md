# Sightline Connection State

Sightline keeps connection state split across three separate concerns. Do not merge these concepts.

## Local Storage

Only durable session preferences belong in localStorage:

- `talon_gateway_url`: persisted gateway URL, written with a short debounce.
- `talon_auth_token`: runtime bearer token returned by sign-in or API-key exchange.

Advanced Options are not durable preferences and must not be stored in localStorage:

- Namespace / connection root
- Manual JWT draft
- API key draft

The old `talon_manual_jwt` and `talon_connection_namespace` keys are removed on load for cleanup.

## URL Params

Sightline URL params have separate ownership:

- `root`: connection namespace prefill. This is the only URL param that may hydrate the Advanced Options namespace field.
- `ns`, `type`, `agent`, `channel`, `session`, `name`: explorer selection state. These params select resources in the explorer and must not hydrate the connection namespace field.
- `connected`: requested connection state.
- `historyPageSize`: chat history page size.

When the app rewrites the URL for explorer selection, it must preserve `root` if present.

## Connection Probe

On connect, Sightline uses the submitted Advanced Options namespace as the scoped root for API-key exchange/probing. If that field is empty, Sightline may infer a single namespace from the resulting JWT and use it for the connection probe and initial explorer selection. That inferred namespace must not be persisted as an Advanced Options draft.
