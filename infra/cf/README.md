# Talon on Cloudflare

Cloudflare-native deployment assets for Talon. This target runs Talon's Rust gateway and worker inside Cloudflare Containers, with a TypeScript Worker acting as the Cloudflare entrypoint and binding bridge.

## Layout

```text
infra/cf/
  README.md
  talon.yaml                         # readable source for the Cloudflare Talon runtime config
  envoy.yaml                         # Envoy config for the Cloudflare Envoy container
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
    Dockerfile                       # Node/Wrangler/Docker CLI tooling image
    docker-compose.yaml              # local E2E/dev stack
    wrangler.jsonc                   # local Wrangler config using Dockerfile-built containers
  dockerfiles/
    cloudflare-envoy.Dockerfile      # Cloudflare-specific Envoy image
  tf/                                # reusable Terraform module for Cloudflare backing resources
```

The generic Talon runtime image is still built from `dockerfiles/oss-runtime.Dockerfile`. That image contains both `talon-server` and `talon-worker`. Cloudflare-specific Talon config is passed through `TALON_CONFIG_INLINE_YAML` rather than baked into the image filesystem.

## Architecture

Requests enter through the Cloudflare Worker in `worker/src/index.ts`.

```text
HTTP request
  -> Cloudflare Worker
    -> EnvoyContainer for /v1/*
      -> GatewayContainer / talon-server
    -> GatewayContainer directly for non-/v1 requests

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

## Local Development

Start the Cloudflare dev stack:

```bash
docker compose -f infra/cf/dev/docker-compose.yaml up --build -d cloudflare-dev mock-llm
```

The local gateway URL is:

```text
http://localhost:8787
```

Useful checks:

```bash
curl http://localhost:8787/healthz
docker compose -f infra/cf/dev/docker-compose.yaml logs -f cloudflare-dev
docker compose -f infra/cf/dev/docker-compose.yaml ps
```

Stop the stack:

```bash
docker compose -f infra/cf/dev/docker-compose.yaml down
```

Run the local E2E test container:

```bash
docker compose -f infra/cf/dev/docker-compose.yaml up --build --abort-on-container-exit e2e-tests
```

Local development uses `wrangler dev` inside the `cloudflare-dev` service. The service mounts the repo and Docker socket so Wrangler can build and run Cloudflare Containers locally. Wrangler/Miniflare creates local-only D1/R2/Queue resources from the bindings; it does not touch production resources.

The first startup can be slow because Wrangler builds the Talon runtime image. Later runs should reuse Docker layers.

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
npx wrangler deploy --config wrangler.jsonc
```

Production containers should use pinned images from CI, not local Dockerfile builds. The checked-in `worker/wrangler.jsonc` currently uses floating GHCR tags as placeholders:

```text
ghcr.io/impalasys/talon-runtime:latest
ghcr.io/impalasys/talon-envoy-cloudflare:latest
```

Before production deploys, replace these with immutable `sha-*` tags or image digests, and ensure the D1 database ID matches the Terraform-created D1 database.

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
EnvoyContainer
```

Cloudflare Containers are backed by Durable Objects. Production does not require a 1:1 mapping between gateway and worker containers, but scaling is manual today: the Worker must address multiple container IDs.

The Worker supports these count variables:

```text
TALON_GATEWAY_CONTAINER_COUNT=1
TALON_WORKER_CONTAINER_COUNT=1
TALON_ENVOY_CONTAINER_COUNT=1
```

When the count is `1`, the Worker uses the stable instance name `default`. When a count is greater than `1`, it routes to bounded instance names such as:

```text
gateway-0, gateway-1
worker-0, worker-1
envoy-0, envoy-1
```

HTTP gateway and Envoy requests are spread across their configured pools. Queue and alarm delivery use stable hashing to select a worker instance.

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

`infra/cf/talon.yaml` is the readable source for that YAML. Regenerate both Wrangler configs after editing it:

```bash
infra/cf/gen-wrangler.sh
```

The script also accepts a config path:

```bash
infra/cf/gen-wrangler.sh infra/cf/talon.yaml
```

The generated configs should not be hand-edited for values owned by the script. Override generation defaults with environment variables, for example:

```bash
TALON_CF_D1_DATABASE_ID="<real-d1-id>" \
TALON_CF_PROD_RUNTIME_IMAGE="ghcr.io/impalasys/talon-runtime:sha-<commit>" \
TALON_CF_PROD_ENVOY_IMAGE="ghcr.io/impalasys/talon-envoy-cloudflare:sha-<commit>" \
TALON_CF_GATEWAY_CONTAINER_COUNT=2 \
TALON_CF_WORKER_CONTAINER_COUNT=8 \
infra/cf/gen-wrangler.sh
```

The Worker sets:

```text
TALON_CONFIG_INLINE_YAML=<Cloudflare Talon YAML>
TALON_SCHEDULER_DRIVER=cloudflare_alarms
```

Secrets should remain environment-backed. The checked-in `talon.yaml` references `NOVITA_API_KEY` through Talon's existing env secret loader.

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
