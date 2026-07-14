---
title: Configuration
sidebar:
  order: 4
---

Talon configuration covers providers, the control plane, and scheduler behavior.

Config files are loaded through a YAML compatibility layer and converted into
the runtime config proto. The proto-native shape uses `providers` and
`control_plane`; the checked-in local files also use aliases such as
`llmProviders`, `storage`, and `pubsub` where they are easier to read.

## Provider configuration

Provider config defines model backends and secrets. The config schema supports:

- OpenAI
- Anthropic
- Google
- generic OpenAI-compatible providers

Provider maps may be written as `providers` or `llmProviders`. If both are
present, Talon merges them before building the runtime config.

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
- optional object storage and document database backends

The examples below use the proto-native `control_plane` form.

### Local socket broker

For a single-host deployment, the control-plane message broker can use a local Unix socket:

```yaml
control_plane:
  database:
    driver: sqlite
    data_dir: ./var/talon
  message_broker:
    driver: local_socket
  object_store:
    driver: local
    path: ./var/talon/objects
```

The compose-oriented YAML in this repository uses the equivalent `storage` and
`pubsub` aliases:

```yaml
storage:
  control:
    driver: postgres
    url:
      source: env
      key: TALON_CONTROL_DATABASE_URL
  data:
    driver: postgres
    url:
      source: env
      key: TALON_DATA_DATABASE_URL
  documents:
    driver: postgres
    url:
      source: env
      key: TALON_DOCUMENT_DATABASE_URL
  objects:
    driver: local
    path: /data/talon/objects

pubsub:
  driver: gcp_pubsub
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

The local compose stack uses the `storage.control`, `storage.data`, and
`storage.documents` aliases to point all three stores at one local Postgres
instance.

### AWS control plane

AWS backends are compiled behind the `aws` crate feature so local-only builds do not pull in every AWS service client. The feature enables DynamoDB, SQS, and EventBridge Scheduler support together.

```yaml
control_plane:
  database:
    driver: dynamodb
    url:
      source: env
      key: TALON_DYNAMODB_TABLE
  message_broker:
    driver: sqs
  scheduler:
    driver: aws_eventbridge_scheduler
    group_name: talon
    queue_url: ${TALON_AWS_SCHEDULER_QUEUE_URL}
    execution_role_arn: ${TALON_AWS_SCHEDULER_EXECUTION_ROLE_ARN}
```

Notes:

- DynamoDB uses one shared table with namespace-isolated partition keys. Production deployments should provision this table in infra before Talon starts.
- `TALON_DYNAMODB_ENDPOINT_URL` and `TALON_SQS_ENDPOINT_URL` point the AWS SDK clients at local emulators such as DynamoDB Local or LocalStack.
- EventBridge Scheduler sends wakeups to SQS using `SendMessage`; workers consume those wakeups through the same SQS pull mode as other durable worker topics.
- `TALON_AWS_SCHEDULER_GROUP_NAME`, `TALON_AWS_SCHEDULER_QUEUE_URL`, `TALON_AWS_SCHEDULER_EXECUTION_ROLE_ARN`, `TALON_AWS_SCHEDULER_NAME_PREFIX`, `TALON_AWS_SCHEDULER_DLQ_ARN`, `TALON_AWS_SCHEDULER_MAX_EVENT_AGE_SECONDS`, `TALON_AWS_SCHEDULER_MAX_RETRY_ATTEMPTS`, and `TALON_AWS_SCHEDULER_ENDPOINT_URL` configure the AWS scheduler when env-based config is used.
- `TALON_SQS_QUEUE_NAME` defaults to `talon` and names the single SQS queue used for durable worker-delivered messages. `TALON_SQS_QUEUE_PREFIX` is still accepted as a compatibility fallback.
- `TALON_SQS_WAIT_TIME_SECONDS` is clamped to the SQS `0..=20` range, and `TALON_SQS_VISIBILITY_TIMEOUT_SECONDS` is clamped to `0..=43200`. Worker pull mode extends visibility while a dispatch is in flight.
- SQS provides durable work-queue semantics through worker pull mode. Talon writes worker messages to the same queue and stores the logical Talon routing key in SQS message attributes for dispatch routing. Messages are deleted only after the worker dispatch succeeds.
- The generic Talon `subscribe` stream is not available for SQS because it cannot acknowledge messages after handler completion. Live session parts and workflow events are delivered through the worker `FanoutService`, not through SQS topics.
- LocalStack can validate EventBridge Scheduler configuration and API wiring, but does not currently emulate timed Scheduler delivery to SQS.
- Playwright's helper stack can be started with `TALON_E2E_STACK=aws` to run Talon against LocalStack DynamoDB, SQS, and EventBridge Scheduler wiring. LocalStack still does not fire scheduled targets into SQS, so schedule delivery assertions should use Talon's local scheduler backends or opt-in real AWS smoke tests.

## Local environment

The local compose stack sets most runtime wiring automatically, including:

- Postgres URL
- Pub/Sub emulator host
- local object store path
- local scheduler driver
- worker pull mode

Provider credentials usually come from `.env`, environment variables, or another supported secret source.

## Common environment variables

Common runtime environment variables include:

- `TALON_CONFIG_PATH`
- `TALON_CONTROL_DATABASE_URL`
- `TALON_DATA_DATABASE_URL`
- `TALON_DOCUMENT_DATABASE_URL`
- `TALON_API_KEY`
- `GCP_PROJECT_ID`
- `GRPC_ADDR`
- `PORT`
- `PULL_MODE`
- `TALON_SCHEDULER_DRIVER`
- `TALON_LOCAL_SCHEDULER_TARGET_URL`
- `TALON_LOCAL_SCHEDULER_RUNNER`
- `TALON_JWT_PRIVATE_KEY_PEM`
- `TALON_JWT_ISSUER`

## Platform JWT and JWKS

Talon requires `TALON_JWT_PRIVATE_KEY_PEM` at startup for platform JWT signing,
gateway JWT verification, and JWKS publication. It publishes public key material
at `/.well-known/jwks.json` plus OAuth/OIDC metadata endpoints.

JWT `iss` defaults to `https://talon.impala.systems` and can be overridden with
`TALON_JWT_ISSUER`. JWKS proves a token was signed by Talon; gateway
authorization still requires `aud: "talon.impala.systems"`. MCP auth broker
assertions use `aud: "mcps.talon.impala.systems"` and are rejected by gateway
auth.

## Read next

- [Local Development](./local-development)
- [Config Schema](../reference/generated/config-schema)
