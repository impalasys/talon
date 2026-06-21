# Local Single-Host Development Without Docker

This document covers the smallest local Talon stack that still works with the current Sightline UI.

This mode runs:

- gateway as a local process
- worker as a local process
- SQLite for control-plane storage
- Unix socket broker for same-host event delivery

This mode does not use:

- `docker compose`
- Postgres
- Pub/Sub emulator
- a separate browser edge proxy

It is intended for one machine running both gateway and worker.

## What each component does

- `SQLite`: durable control-plane state
- `local_socket`: same-host broker transport between gateway and worker
- `scheduler backend`: delayed/background job storage and delivery, separate from the broker

Important separation:

- switching to `local_socket` does not remove scheduler storage
- `TALON_SCHEDULER_DRIVER=local_sqlite` will still create scheduler tables in SQLite
- if you do not want scheduler tables, do not enable `local_sqlite`

## UI gateway endpoint

Sightline connects directly to the gateway, which serves both native gRPC and gRPC-Web on one port.

Use:

- Sightline UI: `http://localhost:3000`
- gateway: `http://127.0.0.1:50051`

## Prerequisites

- Rust toolchain
- repository checked out locally

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
  GRPC_ADDR=127.0.0.1:50051 \
  RUST_LOG=info \
  ./target/debug/talon-server
```

Expected startup lines include:

```text
Connecting to SqliteKvStore at sqlite://$ROOT/data/talon-control-plane.db...
Initializing LocalSocketMessagePublisher at $ROOT/data/talon-broker.sock...
gRPC Gateway listening on: 127.0.0.1:50051
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

## 6. Verify listeners

```bash
lsof -iTCP:50051 -iTCP:18081 -sTCP:LISTEN
```

You should see:

- gateway gRPC and gRPC-Web on `50051`
- worker on `18081`

## 7. Connect the UI

In Sightline, connect to:

```text
http://127.0.0.1:50051
```

## 8. Apply resources

Create the namespace:

```bash
./target/debug/talon-cli \
  --gateway http://127.0.0.1:50051 \
  apply --file "$ROOT/manifests/pretzel.namespace.yaml"
```

Create the agent:

```bash
./target/debug/talon-cli \
  --gateway http://127.0.0.1:50051 \
  apply --file "$ROOT/manifests/dj.agent.yaml"
```

Verify through the edge:

```bash
./target/debug/talon-cli \
  --gateway http://127.0.0.1:50051 \
  get agents --namespace pretzel
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

Stop the worker and gateway processes, then remove the temp directory:

```bash
rm -rf "$ROOT"
```
