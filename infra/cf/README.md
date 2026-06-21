# Talon on Cloudflare

Cloudflare-native deployment assets for Talon. This target runs Talon's Rust gateway and worker inside Cloudflare Containers, with a TypeScript Worker acting as the Cloudflare entrypoint and binding bridge.

## Layout

```text
infra/cf/
  README.md
  talon.yaml                         # production Talon runtime config, rendered into worker/wrangler.jsonc
  bindings/                          # @impalasys/talon-cf-bindings package
    src/
      d1.ts                          # generic D1 SQL execute bridge
      r2.ts                          # R2 object bridge
      queues.ts                      # Queue publisher/consumer bridge
      alarms.ts                      # Durable Object alarm scheduler bridge
  worker/                            # deployable Cloudflare Worker package
    src/index.ts                     # Worker entrypoint and Container classes
    wrangler.jsonc                   # production-oriented Wrangler config
  dev/                               # local development harness
    talon.yaml                       # local/mock Talon runtime config, rendered into dev/wrangler.jsonc
    Dockerfile                       # Node/Wrangler/Docker CLI tooling image
    docker-compose.yaml              # local E2E/dev stack
    wrangler.jsonc                   # local Wrangler config using Dockerfile-built containers
  tf/                                # reusable Terraform module for Cloudflare backing resources
```

The generic Talon runtime image is still built from `dockerfiles/oss-runtime.Dockerfile`. That image contains both `talon-server` and `talon-worker`. Cloudflare-specific Talon config is passed through `TALON_CONFIG_INLINE_YAML` rather than baked into the image filesystem.

## Architecture

Requests enter through the Cloudflare Worker in `worker/src/index.ts`.

```text
HTTP request
  -> Cloudflare Worker
    -> GatewayContainer / talon-server
      -> 50051 for Talon gRPC and gRPC-Web requests

Cloudflare Queue batch
  -> Worker queue() handler
    -> WorkerContainer / talon-worker /cloudflare/queues/dispatch

Scheduled wakeup
  -> Durable Object alarm
    -> WorkerContainer / talon-worker /schedules/fire
```

The Worker exposes internal virtual hostnames to the Rust containers:

```text
http://talon-d1.internal       -> D1 binding bridge
http://talon-r2.internal       -> R2 binding bridge
http://talon-queues.internal   -> Queue binding bridge
http://talon-alarms.internal   -> Durable Object alarm bridge
```

Rust owns Talon storage semantics. The TypeScript D1 binding is intentionally a small prepared-statement bridge rather than a duplicate KV implementation.

## Binding API Contracts

The Rust gateway/worker containers do not receive Cloudflare bindings directly. They call Worker outbound handlers through virtual hostnames, and the TypeScript bindings package translates those HTTP calls into D1, R2, Queues, or Durable Object APIs.

### D1 SQL Bridge

Rust backend: `D1KvStore`

Worker host: `http://talon-d1.internal`

```text
POST /execute
content-type: application/json
```

Request:

```json
{
  "mode": "run | all | first",
  "sql": "SELECT ... WHERE namespace = ?1",
  "params": [
    { "type": "text", "value": "default" },
    { "type": "bytes", "valueBase64": "..." },
    { "type": "number", "value": 1 },
    { "type": "bool", "value": true },
    { "type": "null" }
  ]
}
```

Responses:

```json
{ "meta": { "...": "D1 run metadata" } }
{ "row": { "column": { "type": "text", "value": "..." } } }
{ "results": [{ "column": { "type": "bytes", "valueBase64": "..." } }], "meta": {} }
```

The Worker does not build Talon SQL. Rust owns the schema, statements, parameter order, CAS behavior, pagination, and row decoding. The bridge only calls `env.TALON_D1.prepare(sql).bind(...params)` and encodes D1 values into tagged JSON cells.

### R2 Object Bridge

Rust backend: `R2ObjectStore`

Worker host: `http://talon-r2.internal`

```text
PUT /objects/{percent-encoded-key}
GET /objects/{percent-encoded-key}
DELETE /objects/{percent-encoded-key}
```

Headers:

```text
content-type: <object media type>
x-talon-object-metadata: <base64 JSON metadata envelope>
```

`PUT` stores bytes in R2 under the decoded key. The metadata header is stored in R2 `customMetadata.talon`, not as a second metadata object. `GET` returns the object body, content type, and metadata header. `DELETE` is idempotent from Rust's perspective.

### Queue Publish And Delivery Bridge

Rust publisher: `CfQueuesPublisher`

Worker host: `http://talon-queues.internal`

```text
POST /publish
content-type: application/json
```

Request:

```json
{
  "topic": "talon.session.dispatch | talon.resource.lifecycle | talon.session.control",
  "payloadBase64": "..."
}
```

The Worker maps Talon topic names to the configured Cloudflare Queue bindings and sends a queue body:

```json
{
  "eventType": "session_dispatch | resource_lifecycle | session_control",
  "payloadBase64": "..."
}
```

Queue consumption is Worker-owned. The Worker `queue()` handler forwards each message to a selected worker container:

```text
POST http://worker/cloudflare/queues/dispatch
authorization: Bearer <TALON_SCHEDULER_AUTH_TOKEN>
content-type: application/json
```

Body:

```json
{
  "eventType": "session_dispatch",
  "deliveryId": "<Cloudflare message id>",
  "payloadBase64": "..."
}
```

The Rust worker validates the bearer token with the same scheduler authenticator used by `/schedules/fire`. Non-2xx responses or per-message dispatch failures cause that individual queue message to be retried.

### Durable Object Alarm Bridge

Rust scheduler backend: `CfAlarmsSchedulerBackend`

Worker host: `http://talon-alarms.internal`

```text
POST /schedule
POST /cancel
GET /healthz
```

Schedule request:

```json
{
  "namespace": "default",
  "scheduleId": "...",
  "revision": 1,
  "fireAtMicros": 1760000000000000,
  "payloadBase64": "..."
}
```

Schedule response:

```json
{ "handle": "<opaque alarm handle>", "armed": true }
```

Cancel request:

```json
{ "handle": "<opaque alarm handle>" }
```

`ScheduleShard` stores active wakeups in Durable Object storage with a due-time index. When the DO alarm fires, it posts the decoded payload to:

```text
POST http://worker/schedules/fire
X-Talon-Scheduler-Token: <TALON_SCHEDULER_AUTH_TOKEN>
```

Successful delivery deletes the wakeup. Failed delivery is retried per wakeup with bounded backoff and a max retry count, so one failed schedule does not block the entire shard.

## Local Development

Start the Cloudflare dev stack:

```bash
docker compose -f infra/cf/dev/docker-compose.yaml up --build -d cloudflare-dev gateway worker mock-llm
```

The local gateway URL is:

```text
http://localhost:8787
```

`gateway`, `worker`, and `mock-llm` are private to the Compose network.
Only the Worker endpoint is published on the host. The Rust services reach the
Cloudflare binding bridge through internal aliases such as
`http://talon-d1.internal:8787`, and the mock LLM is available internally at
`http://mock-llm.internal:8000`.

Useful checks:

```bash
curl http://localhost:8787/healthz
docker compose -f infra/cf/dev/docker-compose.yaml logs -f cloudflare-dev
docker compose -f infra/cf/dev/docker-compose.yaml ps
cd infra/cf/worker && npm run typecheck && npm run test:bindings
```

Stop the stack:

```bash
docker compose -f infra/cf/dev/docker-compose.yaml down
```

Run the local E2E test container:

```bash
docker compose -f infra/cf/dev/docker-compose.yaml up --build --abort-on-container-exit --exit-code-from e2e-tests e2e-tests
```

Local development uses `wrangler dev` inside the `cloudflare-dev` service for
local D1/R2/Queue resources and Durable Object alarms. By default,
`dev/wrangler.jsonc` sets `TALON_CF_DEV_EXTERNAL_CONTAINERS=true`, so the Talon
gateway and worker run as ordinary private Compose services instead of
Wrangler-managed local Containers. This avoids Wrangler's current local
Containers path forcing `linux/amd64`, which is brittle on Apple Silicon.

Production still uses real Cloudflare Containers from `worker/wrangler.jsonc`.
If you want to try Wrangler-managed local Containers on an amd64 Docker host,
regenerate dev config with:

```bash
TALON_CF_DEV_EXTERNAL_CONTAINERS=false infra/cf/gen-wrangler.sh
```

The first startup can be slow because Compose builds the Talon runtime image.
Later runs should reuse Docker layers.

## Production Provisioning

`wrangler deploy` deploys the Worker and binds it to named resources, but it should not be treated as the source of truth for production D1/R2/Queue creation.

Provision backing resources with Terraform:

```hcl
module "talon_cf" {
  source = "github.com/impalasys/talon//infra/cf/tf?ref=main"

  account_id  = var.cloudflare_account_id
  name_prefix = "talon"
}
```

For production, pin `ref` to a release tag or commit SHA.

The module creates:

```text
D1 database: talon-control-plane
R2 bucket:   talon-objects
Queues:      talon-session-dispatch
             talon-resource-lifecycle
             talon-session-control
```

The module also outputs binding metadata in a Wrangler-friendly shape. Use those outputs to update or generate `worker/wrangler.jsonc`, especially the D1 `database_id`.

## Production Deploy

The production Worker package lives in `infra/cf/worker`.

```bash
cd infra/cf/worker
npm ci
npx wrangler secret put TALON_SCHEDULER_AUTH_TOKEN --config wrangler.jsonc
npx wrangler deploy --config wrangler.jsonc
```

Production containers use CI-published GHCR images, not local Dockerfile
builds. The checked-in `worker/wrangler.jsonc` defaults to `latest` for the
simplest manual deploy path:

```text
ghcr.io/impalasys/talon-runtime:latest
```

For repeatable production deploys, set `TALON_CF_PROD_IMAGE_TAG`,
or `TALON_CF_PROD_RUNTIME_IMAGE`, then run `infra/cf/gen-wrangler.sh`.
Prefer immutable `sha-*` tags or image digests for promotions, set
`TALON_SCHEDULER_AUTH_TOKEN` with `wrangler secret put`, and
ensure the D1 database ID matches the Terraform-created D1 database.

Dry-run a deploy without publishing:

```bash
cd infra/cf/worker
npm ci
npx wrangler deploy --dry-run --config wrangler.jsonc --containers-rollout=none
```

## Scaling Model

The current Worker code starts one logical instance each:

```text
GatewayContainer
WorkerContainer
```

Cloudflare Containers are backed by Durable Objects. Production does not require a 1:1 mapping between gateway and worker containers, but scaling is manual today: the Worker must address multiple container IDs.

The Worker supports these count variables:

```text
TALON_GATEWAY_CONTAINER_COUNT=1
TALON_WORKER_CONTAINER_COUNT=1
```

When the count is `1`, the Worker uses the stable instance name `default`. When a count is greater than `1`, it routes to bounded instance names such as:

```text
gateway-0, gateway-1
worker-0, worker-1
```

HTTP gateway requests are spread across their configured pool. Queue and alarm delivery use stable hashing to select a worker instance.

Future scaling work should make gateway and worker counts independent, for example:

```text
gateway_count = 2
worker_count  = 8
```

Queue delivery can then shard across worker container IDs separately from HTTP gateway traffic.

## Config

Talon reads Cloudflare runtime config from `TALON_CONFIG_INLINE_YAML`, which is declared in Wrangler `vars` and passed into the gateway/worker containers by `worker/src/index.ts`.

```text
infra/cf/dev/wrangler.jsonc
infra/cf/worker/wrangler.jsonc
```

`infra/cf/talon.yaml` is the production source config. It defaults to OpenAI's API URL and reads `OPENAI_API_KEY` through Talon's env secret loader.

`infra/cf/dev/talon.yaml` is the local/E2E source config. It points at the Docker Compose `mock-llm` service on `mock-llm.internal:8000` and reads `MOCK_LLM_API_KEY`, which `gen-wrangler.sh` fills with a local placeholder in `dev/wrangler.jsonc`.

Regenerate both Wrangler configs after editing either file:

```bash
infra/cf/gen-wrangler.sh
```

The script also accepts a single config path, which renders both dev and production from the same YAML:

```bash
infra/cf/gen-wrangler.sh infra/cf/talon.yaml
```

To override only one side, set `TALON_CF_DEV_CONFIG_YAML` or `TALON_CF_PROD_CONFIG_YAML`.

The generated configs should not be hand-edited for values owned by the script. Override generation defaults with environment variables, for example:

```bash
TALON_CF_D1_DATABASE_ID="<real-d1-id>" \
TALON_CF_PROD_RUNTIME_IMAGE="ghcr.io/impalasys/talon-runtime:sha-<commit>" \
TALON_CF_GATEWAY_CONTAINER_COUNT=2 \
TALON_CF_WORKER_CONTAINER_COUNT=8 \
infra/cf/gen-wrangler.sh
```

The Worker sets:

```text
TALON_CONFIG_INLINE_YAML=<Cloudflare Talon YAML>
TALON_SCHEDULER_DRIVER=cf_alarms
```

Secrets should remain environment-backed. Any string Worker variables and secrets are forwarded into the Talon containers, while Cloudflare control variables such as container counts and inline config remain Worker-owned. For production, configure the OpenAI key referenced by `infra/cf/talon.yaml`:

```bash
cd infra/cf/worker
npx wrangler secret put OPENAI_API_KEY --config wrangler.jsonc
```

## Validation

Useful checks while editing this folder:

```bash
cd infra/cf/worker
npm ci
npm run typecheck

cd ../../..
infra/cf/gen-wrangler.sh
docker compose -f infra/cf/dev/docker-compose.yaml config
infra/cf/worker/node_modules/.bin/wrangler deploy --dry-run --config infra/cf/dev/wrangler.jsonc --containers-rollout=none
infra/cf/worker/node_modules/.bin/wrangler deploy --dry-run --config infra/cf/worker/wrangler.jsonc --containers-rollout=none

cd infra/cf/tf
terraform fmt -check
terraform init -backend=false
terraform validate
```

Remove generated local artifacts before committing:

```bash
rm -rf infra/cf/worker/node_modules infra/cf/tf/.terraform infra/cf/tf/.terraform.lock.hcl
```
