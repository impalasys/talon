# Talon

Talon is the control plane for cloud native agents. It lets teams operate autonomous agent fleets with durable execution, declarative configuration, namespace isolation, and a browser-native fleet view.

Talon gives you the infrastructure long-lived agents were missing: a gateway API, worker runtime, persisted sessions, schedule wakeups, knowledge, MCP bindings, and Sightline for inspecting what is running.

## What ships in this repo

- `talon-server`: gateway process exposing the canonical gRPC API plus the browser-oriented UI HTTP surface
- `talon-worker`: worker runtime that consumes dispatch events and executes agent turns
- `talon-cli`: admin CLI for applying manifests, inspecting resources, and managing knowledge
- `ui/`: Next.js operator UI
- `proto/`: gateway, config, manifest, event, and model contracts
- `manifests/`: default resources plus end-to-end examples
- `docs/`: concepts, tutorials, operations guides, and generated API reference

## Runtime model

Talon is split into a few explicit roles:

- Gateway: accepts API calls, persists session state, and publishes work
- Worker: consumes work, resolves models and tools, executes turns, and emits step events
- Control plane: persistence, broker, and scheduler backing services
- Edge/UI: Envoy plus Sightline, the browser-native fleet view for operators

The important product behavior is durability. Threads survive crashes, deploys, and cold starts; schedules wake named agent workflows back up; and specs stay declarative as you iterate on prompts, tools, and policies.

In the checked-in local stack, the control plane uses Postgres and a Pub/Sub emulator. For same-host development, Talon can also run directly against SQLite plus a local socket broker.

## Repository layout

```text
src/                  Rust runtime, gateway, worker, control plane, CLI
src/bin/              talon-server, talon-worker, talon-cli entrypoints
ui/                   Next.js operator UI
packages/talon-chat/ React client package for Talon-backed chat surfaces
proto/                Protobuf service and schema definitions
manifests/            Default and example namespace/agent resources
dockerfiles/          Runtime, UI, and Envoy container builds
docs/                 Product and operator documentation
tests/                Python end-to-end tests
```

## Binaries

Cargo builds three binaries:

```bash
cargo build --locked --bins
```

- `target/debug/talon-server`
- `target/debug/talon-worker`
- `target/debug/talon-cli`

Release builds:

```bash
cargo build --locked --release --bins
```

## Quickstart

### Prerequisites

- Docker / Docker Compose
- Git
- Rust toolchain
- `protobuf-compiler`
- at least one real provider API key in `.env`

Clone the repository:

```bash
git clone https://github.com/impalasys/talon.git
cd talon
```

Create a local env file:

```bash
cp .env.example .env
```

Set the OpenAI provider key. The checked-in local stack includes the `openai` provider in `talon.compose.yaml`, so the shortest path is:

```bash
OPENAI_API_KEY=your-real-api-key
```

### Start the full local stack

```bash
docker compose up --build -d
```

This starts:

- gateway
- worker
- Postgres
- Pub/Sub emulator
- Envoy
- Sightline UI
- a local object store volume for multimodal session assets
- a manifest bootstrap step that applies `manifests/default_agent.yaml`

Useful local endpoints:

- UI: `http://localhost:3000`
- Envoy edge: `http://localhost:18789`
- native gRPC gateway: `http://localhost:50051`
- gateway UI HTTP surface: `http://localhost:50052`

The convenience wrapper in [`run.sh`](./run.sh) does the same startup and also loads provider keys from the macOS keychain when present.

### Apply a namespace and agent

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 apply -f manifests/examples/chatgpt-app/namespace.yaml
cargo run --bin talon-cli -- --gateway http://localhost:18789 apply -f manifests/examples/chatgpt-app/support-docs-template.yaml
cargo run --bin talon-cli -- --gateway http://localhost:18789 apply -f manifests/examples/chatgpt-app/support-docs-agent.yaml
```

This is the core Talon loop in practice:

- define declarative agent specs
- bind tools and MCPs explicitly
- run durable named sessions
- wake work on schedule
- reuse knowledge across agents

### Send a browser-style session request

```bash
curl -sS http://localhost:18789/v1/ns/chatgpt-app/agents/support-docs/sessions \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"ns":"chatgpt-app","agent":"support-docs"}'
```

Use the returned `sessionId` against `/v1/ui/...` to drive a session, or inspect it in the UI.

## Running without Docker

For single-host development, Talon can run with:

- SQLite for control-plane storage
- `local_socket` for broker transport
- host-native Envoy for the browser edge

See [`docs/wiki/local-single-host-development.md`](./docs/wiki/local-single-host-development.md) for the full workflow. The short version is:

1. build `talon-server`, `talon-worker`, and `talon-cli`
2. point `TALON_CONFIG_PATH` at a config using `sqlite` and `local_socket`
3. run the gateway and worker as local processes
4. front them with Envoy for UI compatibility

## Configuration

Talon reads config from `TALON_CONFIG_PATH` or the default config loader. The checked-in [`talon.yaml`](./talon.yaml) and [`talon.compose.yaml`](./talon.compose.yaml) show the current supported shape:

- provider definitions under `providers`
- control-plane database driver and connection
- message broker driver
- optional object store configuration
- optional scheduler configuration

Common environment variables used by the runtime:

- `TALON_CONFIG_PATH`
- `POSTGRES_URL`
- `GCP_PROJECT_ID`
- `GRPC_ADDR`
- `GATEWAY_UI_ADDR`
- `PORT`
- `PULL_MODE`
- `TALON_SCHEDULER_DRIVER`
- `TALON_LOCAL_SCHEDULER_TARGET_URL`
- `TALON_LOCAL_SCHEDULER_RUNNER`
- `GATEWAY_PASSWORD`, `GATEWAY_TOKEN`, or `GATEWAY_JWT_SECRET`

## CLI

`talon-cli` is the operator entry point for common admin flows:

- `apply`
- `render`
- `get`
- `delete`
- `knowledge get|set|delete|sync`
- `gen`

Examples:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 get agent support-docs --namespace chatgpt-app
cargo run --bin talon-cli -- --gateway http://localhost:18789 knowledge sync --namespace chatgpt-app --manifest manifests/examples/chatgpt-app/support-docs-template.yaml
```

## Development

### Rust

```bash
cargo metadata --locked
cargo build --locked --bins
cargo test --locked
```

### UI

```bash
cd ui
pnpm install --frozen-lockfile
pnpm build
```

### Docker validation

```bash
docker build -f dockerfiles/oss-runtime.Dockerfile .
docker build -f dockerfiles/oss-ui.Dockerfile .
```

To validate the Envoy image:

```bash
docker build -f dockerfiles/envoy.Dockerfile .
```

## CI artifacts

GitHub CI validates Cargo, runtime image, Envoy image, and UI builds. On pushes to `main`, CI publishes Docker images to GHCR:

- `ghcr.io/impalasys/talon-runtime:latest`
- `ghcr.io/impalasys/talon-envoy:latest`
- `ghcr.io/impalasys/talon-ui:latest`

Each image is also tagged as `sha-<commit>` for immutable references from downstream projects.

On pushes to `main`, CI also packages release binaries for:

- Linux `x86_64`
- macOS `arm64`

Each workflow artifact bundle contains:

- `talon-server`
- `talon-worker`
- `talon-cli`
- `talon.yaml`
- `SHA256SUMS`

That artifact is intended for single-host runtime validation or downstream packaging. It is not a full deployment bundle by itself because Talon still expects a backing config and control-plane dependencies.

## Documentation

Start here:

- [`docs/intro.md`](./docs/intro.md)
- [`docs/getting-started/quickstart.md`](./docs/getting-started/quickstart.md)
- [`docs/concepts/how-talon-works.md`](./docs/concepts/how-talon-works.md)
- [`docs/operations/local-development.md`](./docs/operations/local-development.md)
- [`docs/reference/index.md`](./docs/reference/index.md)

## License

Source files in this repository are marked `AGPL-3.0-only`. See [`SECURITY.md`](./SECURITY.md) and the repository licensing metadata for additional project policy.
