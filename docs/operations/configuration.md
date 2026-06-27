---
title: Configuration
sidebar_position: 4
---

Talon configuration covers providers, the control plane, and scheduler behavior.

## Provider configuration

Provider config defines model backends and secrets. The config schema supports:

- OpenAI
- Anthropic
- Google
- generic OpenAI-compatible providers

## Secret sources

Secrets can be sourced from:

- plain inline values
- environment variables
- GCP Secret Manager
- local keychain
- AWS or Azure secret references

## Control plane configuration

The control plane config defines:

- database driver and URL
- message broker driver
- scheduler backend configuration

### Local socket broker

For a single-host deployment, the control-plane message broker can use a local Unix socket:

```yaml
control_plane:
  database:
    driver: sqlite
    data_dir: ./var/talon
  message_broker:
    driver: local_socket
```

Notes:

- This mode is intended for one host running the gateway and one or more workers locally.
- The broker socket defaults to `talon-broker.sock` under the SQLite `data_dir` when one is available.
- Override the socket path with `TALON_LOCAL_SOCKET_PATH=/absolute/path/talon-broker.sock`.
- `local_socket` is lightweight and non-durable. It is best for same-host dispatch where queued events do not need to survive process restarts.

### SQLite control plane

For a single-host deployment, the control plane database can use SQLite:

```yaml
control_plane:
  database:
    driver: sqlite
    data_dir: ./var/talon
  message_broker:
    driver: gcp_pubsub
```

Notes:

- Talon will create `talon-control-plane.db` under `data_dir`.
- You can also set `control_plane.database.url` directly to a SQLite URL such as `sqlite:///absolute/path/talon.db`.
- SQLite is intended for same-host access. Keep the database on a local filesystem, not a network filesystem.
- For local schedule delivery with the same SQLite file, set `TALON_SCHEDULER_DRIVER=local_sqlite`.

### RocksDB control plane

For single-process embedded deployments, the control plane database can use RocksDB:

```yaml
control_plane:
  database:
    driver: rocksdb
    data_dir: ./var/talon
  message_broker:
    driver: local_socket
```

Notes:

- Talon will create `talon-control-plane.rocksdb` under `data_dir`.
- You can also set `control_plane.database.url` directly to a RocksDB path such as `rocksdb:///absolute/path/talon-control-plane.rocksdb`.
- RocksDB is embedded and cannot be opened read/write by separate gateway and worker processes. Start `talon-node` instead of separate `talon-server` and `talon-worker` processes so gateway and worker subscriptions share one control plane.
- Runtime tuning is exposed through environment variables: `TALON_ROCKSDB_COMPRESSION=none|lz4`, `TALON_ROCKSDB_WRITE_BUFFER_SIZE_MB`, `TALON_ROCKSDB_MAX_WRITE_BUFFER_NUMBER`, `TALON_ROCKSDB_BLOCK_CACHE_SIZE_MB`, and `TALON_ROCKSDB_MAX_BACKGROUND_JOBS`.
- `TALON_ROCKSDB_DISABLE_WAL=true` skips the write-ahead log and can improve benchmark throughput, but writes can be lost after a crash. Keep it disabled for durable deployments.

### Postgres control plane

For multi-service or existing Postgres-backed deployments:

```yaml
control_plane:
  database:
    driver: postgres
    url:
      source: env
      key: TALON_DATABASE_URL
  message_broker:
    driver: gcp_pubsub
```

### AWS control plane

AWS backends are compiled behind crate features so local-only builds do not pull in every AWS service client:

- `dynamodb` enables `control_plane.database.driver: dynamodb`
- `sqs` enables `control_plane.message_broker.driver: sqs`
- `aws-local-e2e` enables both and compiles the Docker-backed AWS local integration tests

```yaml
control_plane:
  database:
    driver: dynamodb
    url:
      source: env
      key: TALON_DYNAMODB_TABLE
  message_broker:
    driver: sqs
```

Notes:

- DynamoDB uses one shared table with namespace-isolated partition keys. Production deployments should provision this table in infra before Talon starts.
- `TALON_DYNAMODB_ENDPOINT_URL` and `TALON_SQS_ENDPOINT_URL` point the AWS SDK clients at local emulators such as DynamoDB Local or LocalStack.
- `TALON_SQS_QUEUE_NAME` defaults to `talon` and names the single SQS queue used for durable worker-delivered messages. `TALON_SQS_QUEUE_PREFIX` is still accepted as a compatibility fallback.
- `TALON_SQS_WAIT_TIME_SECONDS` is clamped to the SQS `0..=20` range, and `TALON_SQS_VISIBILITY_TIMEOUT_SECONDS` is clamped to `0..=43200`. Worker pull mode extends visibility while a dispatch is in flight.
- SQS provides durable work-queue semantics through worker pull mode. Talon writes all worker-delivered topics to the same queue and stores the logical Talon topic in SQS message attributes for dispatch routing. Messages are deleted only after the worker dispatch succeeds.
- The generic Talon `subscribe` stream is not available for SQS because it cannot acknowledge messages after handler completion. Live `talon.session.parts.*` token streams still require Pub/Sub, `local_socket`, or direct worker streaming.

## Local environment

The local compose stack sets most runtime wiring automatically, including:

- Postgres URL
- Pub/Sub emulator host
- local object store path
- local scheduler driver
- worker pull mode

Provider credentials usually come from `.env`, environment variables, or another supported secret source.

## Read next

- [Local Development](./local-development.md)
- [Config Schema](../reference/generated/config-schema.md)
