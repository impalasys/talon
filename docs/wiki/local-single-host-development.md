# Local Single-Host Development Without Docker

This document covers the smallest local Talon stack that still works with the current Sightline UI.

This mode runs:

- gateway as a local process
- worker as a local process
- SQLite for control-plane storage
- Unix socket broker for same-host event delivery
- Envoy as a host-native browser-facing edge

This mode does not use:

- `docker compose`
- Postgres
- Pub/Sub emulator

It is intended for one machine running both gateway and worker.

## What each component does

- `SQLite`: durable control-plane state
- `local_socket`: same-host broker transport between gateway and worker
- `Envoy`: browser-facing edge for the current Sightline UI
- `scheduler backend`: delayed/background job storage and delivery, separate from the broker

Important separation:

- switching to `local_socket` does not remove scheduler storage
- `TALON_SCHEDULER_DRIVER=local_sqlite` will still create scheduler tables in SQLite
- if you do not want scheduler tables, do not enable `local_sqlite`

## Current UI requirement

The current Sightline UI expects a single edge URL that supports:

- gRPC-Web
- REST-transcoded `/v1/...` routes
- `/v1/ui/...` session routes

That means the current UI should connect to Envoy, not directly to the raw gateway gRPC port.

Use:

- Sightline UI: `http://localhost:3000`
- Envoy edge: `http://127.0.0.1:18789`

Do not point the UI at:

- `http://127.0.0.1:51051`

That port is the raw gateway gRPC/gRPC-Web surface, not the full browser edge expected by the current UI.

## Prerequisites

- Rust toolchain
- Envoy installed on the host
- repository checked out locally

If `envoy` is not already installed:

```bash
brew install envoy
```

## 1. Build the binaries

From the repository root:

```bash
cargo build --offline --bin talon-server --bin talon-worker --bin talon-cli
```

If you are not using offline mode locally:

```bash
cargo build --bin talon-server --bin talon-worker --bin talon-cli
```

## 2. Create a temporary runtime directory

```bash
ROOT="$(mktemp -d /tmp/talon-local-XXXXXX)"
mkdir -p "$ROOT/data" "$ROOT/manifests"
```

This directory will hold:

- the config file
- the SQLite database
- the Unix socket broker
- any temporary manifests

## 3. Write the local config

Create `$ROOT/config.yaml`:

```yaml
providers: {}
default_provider: ""
workspace_dir: .

control_plane:
  database:
    driver: sqlite
    data_dir: ./data
  message_broker:
    driver: local_socket
```

`data_dir` is resolved relative to the config file directory.

With the config above, the live runtime artifacts will be created under:

- `$ROOT/data/talon-control-plane.db`
- `$ROOT/data/talon-broker.sock`

## 4. Start the gateway

From the repository root:

```bash
env \
  TALON_CONFIG_PATH="$ROOT/config.yaml" \
  GRPC_ADDR=127.0.0.1:51051 \
  GATEWAY_UI_ADDR=127.0.0.1:51052 \
  RUST_LOG=info \
  ./target/debug/talon-server
```

Expected startup lines include:

```text
Connecting to SqliteKvStore at sqlite://$ROOT/data/talon-control-plane.db...
Initializing LocalSocketMessagePublisher at $ROOT/data/talon-broker.sock...
gRPC Gateway listening on: 127.0.0.1:51051
```

## 5. Start the worker

In a second terminal:

```bash
env \
  TALON_CONFIG_PATH="$ROOT/config.yaml" \
  PORT=18081 \
  PULL_MODE=1 \
  RUST_LOG=info \
  ./target/debug/talon-worker
```

Expected startup lines include:

```text
Connecting to SqliteKvStore at sqlite://$ROOT/data/talon-control-plane.db...
Initializing LocalSocketMessagePublisher at $ROOT/data/talon-broker.sock...
Starting worker background subscriptions transport="local_socket"
```

If you explicitly want durable local scheduler storage too, add:

```bash
TALON_SCHEDULER_DRIVER=local_sqlite
TALON_LOCAL_SCHEDULER_RUNNER=1
```

Only add those when you want the scheduler tables and runner behavior.

## 6. Start Envoy for the UI edge

The repository `envoy.yaml` targets compose service names, so for this direct local mode use a host-targeted Envoy config.

Write `$ROOT/envoy.yaml`:

```yaml
static_resources:
  listeners:
  - name: listener_0
    address:
      socket_address: { address: 127.0.0.1, port_value: 18789 }
    filter_chains:
    - filters:
      - name: envoy.filters.network.http_connection_manager
        typed_config:
          "@type": type.googleapis.com/envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager
          stat_prefix: ingress_http
          codec_type: AUTO
          route_config:
            name: local_route
            virtual_hosts:
            - name: local_service
              domains: ["*"]
              routes:
              - match: { prefix: "/v1/ui/" }
                route:
                  cluster: talon_ui_http_service
                  timeout: 0s
              - match: { prefix: "/" }
                route:
                  cluster: talon_grpc_service
                  timeout: 60s
              cors:
                allow_origin_string_match:
                - safe_regex:
                    google_re2: {}
                    regex: ".*"
                allow_methods: GET, PUT, DELETE, POST, OPTIONS
                allow_headers: keep-alive,user-agent,cache-control,content-type,content-transfer-encoding,x-accept-content-transfer-encoding,x-accept-response-streaming,x-user-agent,x-grpc-web,grpc-timeout,authorization
                max_age: "1728000"
                expose_headers: grpc-status,grpc-message
          http_filters:
          - name: envoy.filters.http.grpc_web
            typed_config:
              "@type": type.googleapis.com/envoy.extensions.filters.http.grpc_web.v3.GrpcWeb
          - name: envoy.filters.http.cors
            typed_config:
              "@type": type.googleapis.com/envoy.extensions.filters.http.cors.v3.Cors
          - name: envoy.filters.http.grpc_json_transcoder
            typed_config:
              "@type": type.googleapis.com/envoy.extensions.filters.http.grpc_json_transcoder.v3.GrpcJsonTranscoder
              proto_descriptor: "/etc/envoy/talon_gateway_proto-descriptor-set.proto.bin"
              services: ["talon.gateway.GatewayService"]
              max_response_body_size: 33554432
              print_options:
                add_whitespace: true
                always_print_primitive_fields: true
                always_print_enums_as_ints: false
                preserve_proto_field_names: false
          - name: envoy.filters.http.router
            typed_config:
              "@type": type.googleapis.com/envoy.extensions.filters.http.router.v3.Router
  clusters:
  - name: talon_grpc_service
    connect_timeout: 0.25s
    type: LOGICAL_DNS
    typed_extension_protocol_options:
      envoy.extensions.upstreams.http.v3.HttpProtocolOptions:
        "@type": type.googleapis.com/envoy.extensions.upstreams.http.v3.HttpProtocolOptions
        explicit_http_config:
          http2_protocol_options: {}
    lb_policy: ROUND_ROBIN
    load_assignment:
      cluster_name: talon_grpc_service
      endpoints:
      - lb_endpoints:
        - endpoint:
            address:
              socket_address:
                address: 127.0.0.1
                port_value: 51051
  - name: talon_ui_http_service
    connect_timeout: 0.25s
    type: LOGICAL_DNS
    lb_policy: ROUND_ROBIN
    load_assignment:
      cluster_name: talon_ui_http_service
      endpoints:
      - lb_endpoints:
        - endpoint:
            address:
              socket_address:
                address: 127.0.0.1
                port_value: 51052
```

Then start Envoy:

```bash
envoy -c "$ROOT/envoy.yaml"
```

Run Envoy in its own terminal so you can see config or routing errors directly.

## 7. Verify listeners

```bash
lsof -iTCP:51051 -iTCP:51052 -iTCP:18081 -iTCP:18789 -sTCP:LISTEN
```

You should see:

- gateway on `51051`
- gateway UI HTTP on `51052`
- worker on `18081`
- Envoy on `18789`

## 8. Connect the UI

In Sightline, connect to:

```text
http://127.0.0.1:18789
```

Do not connect the UI to `51051`.

## 9. Apply resources

Create the namespace:

```bash
./target/debug/talon-cli --rest \
  --gateway http://127.0.0.1:18789 \
  apply --file "$ROOT/manifests/pretzel.namespace.yaml"
```

Create the agent:

```bash
./target/debug/talon-cli \
  --gateway http://127.0.0.1:51051 \
  apply --file "$ROOT/manifests/dj.agent.yaml"
```

Verify through the edge:

```bash
curl -sS http://127.0.0.1:18789/v1/ns/pretzel/agents/dj
```

## Runtime artifacts

After startup, the runtime directory should contain:

- `$ROOT/data/talon-control-plane.db`
- `$ROOT/data/talon-broker.sock`

Depending on SQLite activity, you may also see:

- `$ROOT/data/talon-control-plane.db-wal`
- `$ROOT/data/talon-control-plane.db-shm`

If `local_sqlite` scheduler is enabled, you should also expect scheduler tables inside the same SQLite database.

## Cleanup

Stop the Envoy, worker, and gateway processes, then remove the temp directory:

```bash
rm -rf "$ROOT"
```
