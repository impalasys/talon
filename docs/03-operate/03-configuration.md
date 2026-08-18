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

## Layered configuration

A file can extend one or more local configuration files:

```yaml
extends: ./models.yaml
```

or:

```yaml
extends:
  - ./models.yaml
  - ./shared-providers.toml
```

YAML (`.yaml`/`.yml`), JSON, and TOML extension files are supported. Paths are
resolved relative to the file that declares them; absolute local paths are
also allowed. URLs and other remote includes are rejected. Each file expands
`${ENVIRONMENT_VARIABLE}` placeholders before it is parsed, so paths and
values in a parent are interpreted in the parent's own environment and
directory.

Parent layers are merged in declaration order, followed by the child. Maps
merge recursively, with child keys taking precedence; lists and scalar values
are replaced by the later layer. Duplicate extensions are allowed. Active
include cycles report their full chain, and nesting is limited to 32 levels.
The `providers`/`llmProviders` and `models`/`llmModels`/`modelLimits` aliases
are normalized before merging, so child overrides are deterministic. Relative
filesystem settings such as `workspace_dir`, database directories, and local
object-store paths are resolved relative to the file where they appear.

`TALON_CONFIG_INLINE_YAML` can also use `extends`. Absolute extension paths are
loaded directly, while relative paths are resolved from the process working
directory. This is useful for keeping a large shared catalog in a container
image and applying a small deployment-specific inline override. `extends` is a
loader directive and is not retained in the runtime configuration protobuf.

## Provider configuration

Provider config defines model backends and secrets. The config schema supports:

- OpenAI
- Anthropic
- Google
- generic OpenAI-compatible providers

Provider maps may be written as `providers` or `llmProviders`. If both are
present, Talon merges them before building the runtime config.

Native OpenAI providers use the Responses API by default. Set `api` to
`chat_completions` only for an explicit compatibility fallback:

```yaml
providers:
  openai:
    type: openai
    model: gpt-5.6-luna
    api: responses
```

The accepted values are `responses` and `chat_completions`. Generic
OpenAI-compatible providers continue to use Chat Completions.

## Model catalog

The optional `models` map records model metadata. This repository's
`models.yaml` is a shared curated catalog extended by both example deployment
configs. Model names are matched against the selected agent model;
provider-qualified keys such as `openai/gpt-5` are also supported when the same
model name is used by multiple providers. When multiple entries match, a
provider-qualified key takes precedence over the plain model key, and an entry
with an exact provider takes precedence over one without a provider.

The checked-in catalog includes major Chinese model families and routes,
including DeepSeek, Qwen, GLM/Zhipu, Kimi/Moonshot, MiniMax, Hunyuan, Xiaomi
MiMo, InclusionAI, and Meta Muse Spark, plus Novita, SiliconFlow, Volcengine,
Baichuan, and OpenRouter-qualified variants.

Catalog entries describe limits and pricing only. They do not activate a
provider, enable a connection, or resolve credentials. Keep provider
connections and secrets in the deployment config. The catalog is static and
is not refreshed from provider APIs during startup.

Costs are USD per one million tokens. `contextWindowTokens` is the complete
provider context window. When `maxOutputTokens` is present, compaction reserves
that many tokens for generation and uses the remainder as the history limit.
`longContextTokens` is an input-token pricing threshold. When it is set, the
corresponding `longContext*CostPerMillionTokens` fields describe the rates that
apply once a request exceeds that threshold. It does not change the model's
physical context window, but compaction uses the lower of that threshold and
the model's normal input budget to avoid crossing the higher pricing tier.

```yaml
models:
  openai/gpt-5:
    provider: openai
    contextWindowTokens: 400000
    maxOutputTokens: 128000
    inputCostPerMillionTokens: 1.25
    outputCostPerMillionTokens: 10.00
    cacheReadCostPerMillionTokens: 0.125
    cacheWriteCostPerMillionTokens: 1.25
    longContextTokens: 272000
    longContextInputCostPerMillionTokens: 2.50
    longContextOutputCostPerMillionTokens: 15.00
    longContextCacheReadCostPerMillionTokens: 0.25
```

The pricing fields are retained as model metadata for usage accounting and
future provider integrations. Compaction consumes only the context and output
limits; if no matching model entry exists, it keeps the existing
environment/default character budget.

## Deployment-wide capability gates

Use the top-level `capabilities` map to disable a native capability action for
every agent in a deployment:

```yaml
capabilities:
  code:
    run: false
```

An explicit `false` is a global deny. An explicit `true`, or an omitted gate,
does not grant access by itself: the agent still needs the matching action in
its manifest, such as `spec.capabilities.code: [run]`. Missing gates remain
allowed for backwards compatibility, so this setting is suitable for an
operator-controlled emergency shutoff without changing agent manifests.

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
- `TALON_WORKER_ENDPOINT_URL`
- `TALON_WORKER_PUBLIC_URL`
- `TALON_WORKER_URL`
- `TALON_WORKER_ENDPOINT_PROTOCOL`
- `TALON_WORKER_ENDPOINT_AUDIENCE`
- `TALON_SCHEDULER_DRIVER`
- `TALON_LOCAL_SCHEDULER_TARGET_URL`
- `TALON_LOCAL_SCHEDULER_RUNNER`
- `TALON_JWT_PRIVATE_KEY_PEM`
- `TALON_JWT_ISSUER`

### Serverless code execution

`run_python_code` uses Monty's per-invocation resource limits and a separate,
process-local admission budget. Configure this budget on every worker that can
run agents with the `code:run` capability. The limiter is intentionally local:
it prevents one Cloud Run container from being overcommitted, but it is not a
cross-instance tenant quota.

For a `1 vCPU` / `512 MiB` Cloud Run Worker Pool instance, use these initial
values:

```bash
TALON_CODE_MAX_CONCURRENT_RUNS=1
TALON_CODE_MEMORY_BUDGET_BYTES=268435456
TALON_CODE_QUEUE_TIMEOUT_MS=30000
TALON_CODE_MAX_QUEUED_RUNS=8
TALON_CODE_MAX_TOOL_CALLS=16
```

The reservation for a run includes its requested Monty heap limit, `64 MiB` of
runtime and artifact-serialization overhead, bounded `1 MiB` stdout/stderr
capture buffers, and the maximum `25 MiB` input and `25 MiB` output staging
budgets. A request which cannot fit is rejected before file hydration or a
Monty subprocess is started. The `timeout_ms` value is a hard wall-clock
deadline covering hydration, execution, tool bridges, and output persistence.
Calls wait for capacity for at most the configured queue timeout; a full queue
or elapsed timeout returns a retryable capacity error. Inline `code` plus
`inputs` are capped at `1 MiB`; inputs are also limited to 64 nesting levels and
100,000 values before they are converted for Monty. These limits protect host
side argument conversion, but they do not yet bound every allocation made while
the Monty protocol returns a large value.

Cancellation and timeout cleanup currently relies on `monty-pool`'s
kill-on-drop behavior. The Talon reservation is released when the execution
future is dropped, which can be before the child process has finished exiting.
This is an accepted availability risk for this release and is tracked for an
upstream `monty-pool` abort-and-reap API. Deployments requiring strict
serverless isolation should keep code execution restricted until that upstream
fix is available.

The Talon host binary embeds the Monty worker. Set `TALON_MONTY_BIN` only when
an external runtime is intentionally required. Keep the worker listener private
and avoid granting the worker network egress solely for code execution; Monty
code does not receive host networking, environment variables, or unrestricted
host filesystem access.

Monitor Monty execution outcomes, queue occupancy and capacity rejections,
worker-process count and RSS, cancellation and timeout rates, and container
restarts or OOM kills. A one-run admission limit does not guarantee that a
recently cancelled child has already disappeared.

### Cloud Run Worker Pool endpoint discovery

Cloud Run automatically sets `CLOUD_RUN_WORKER_POOL` inside Worker Pool
containers. Talon uses that runtime marker to enable per-instance endpoint
registration. The worker requests its private IPv4 address from:

```text
http://metadata.google.internal/computeMetadata/v1/instance/network-interfaces/0/ip
```

and registers it with the worker port as an HTTP endpoint. `PORT` is used when
set, with `8081` as the default. The metadata server request includes
`Metadata-Flavor: Google` and the worker does not become ready if the response
is missing, invalid, loopback, link-local, multicast, broadcast, or otherwise
not a usable VPC IPv4 address. VPC IPv4 ranges outside RFC1918, such as
`100.64.0.0/10`, are accepted when they are routable from the gateway.

This requires Cloud Run Worker Pool Direct VPC ingress, VPC routes from the
Talon gateway to the worker subnet, and worker firewall rules that allow the
gateway to reach the configured listener port. Worker Pool IPs are ephemeral;
they may change on restart, so the worker re-registers its current address and
the existing heartbeat TTL removes stale registrations. Worker Pools do not
have a load-balanced URL; `CLOUD_RUN_SERVICE_URL` is not used as a per-instance
endpoint.

```bash
# CLOUD_RUN_WORKER_POOL is injected by Cloud Run Worker Pools.
PORT=8081
```

Explicit `TALON_WORKER_ENDPOINT_URL` takes precedence over discovery. The
existing `TALON_WORKER_PUBLIC_URL` and `TALON_WORKER_URL` overrides remain
supported. When discovery is unset, Talon preserves its existing endpoint
behavior.

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

- [Local Stack](./local-stack)
- [Config Schema](../reference/generated/config-schema)
