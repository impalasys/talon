# Talon

Talon is the control plane for cloud native agents. It lets teams operate autonomous agent fleets with durable execution, declarative configuration, namespace isolation, and a browser-native fleet view.

Talon gives you the infrastructure long-lived agents were missing: a gateway API, worker runtime, persisted sessions, schedule wakeups, knowledge, namespace-scoped MCP tools, and Sightline for inspecting what is running.

## What ships in this repo

- `talon-server`: gateway process exposing the canonical gRPC and gRPC-Web API
- `talon-worker`: worker runtime that consumes dispatch events and executes agent turns
- `talon-cli`: admin CLI for applying manifests, inspecting resources, and managing knowledge
- `ui/`: Next.js operator UI
- `proto/`: versioned API, config, manifest, event, and model contracts
- `manifests/`: default resources plus end-to-end examples
- `docs/`: concepts, tutorials, operations guides, and generated API reference

## Runtime model

Talon is split into a few explicit roles:

- Gateway: accepts API calls, persists session state, and publishes work
- Worker: consumes work, resolves models and tools, executes turns, and emits step events
- Control plane: persistence, broker, and scheduler backing services
- UI: Sightline, the browser-native fleet view for operators

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
dockerfiles/          Runtime and UI container builds
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

Set the OpenAI provider key. The checked-in local stack includes the `openai` provider in `talon.docker-compose.yaml`, so the shortest path is:

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
- Sightline UI
- a local object store volume for multimodal session assets
- a manifest bootstrap step that applies `manifests/default_agent.yaml`

Useful local endpoints:

- UI: `http://localhost:3000`
- gateway gRPC and gRPC-Web: `http://localhost:50051`

The convenience wrapper in [`run.sh`](./run.sh) does the same startup and also loads provider keys from the macOS keychain when present.

### Apply a namespace and agent

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/chatgpt-app/namespace.yaml
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/chatgpt-app/support-docs-template.yaml
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/chatgpt-app/support-docs-agent.yaml
```

This is the core Talon loop in practice:

- define declarative agent specs
- bind tools and MCPs explicitly
- run durable named sessions
- wake work on schedule
- reuse knowledge across agents

### Send a streamed session prompt

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 session prompt \
  --namespace chatgpt-app \
  --agent support-docs \
  --stream \
  "Summarize the support docs in three bullets."
```

Inspect the durable session and streamed reply in Sightline.

## Running without Docker

For single-host development, Talon can run with:

- SQLite for control-plane storage
- `local_socket` for broker transport
- direct gRPC/gRPC-Web gateway access on `50051`

See [`docs/wiki/local-single-host-development.md`](./docs/wiki/local-single-host-development.md) for the full workflow. The short version is:

1. build `talon-server`, `talon-worker`, and `talon-cli`
2. point `TALON_CONFIG_PATH` at a config using `sqlite` and `local_socket`
3. run the gateway and worker as local processes
4. point Sightline and SDK clients at `http://localhost:50051`

## Configuration

Talon reads config from `TALON_CONFIG_PATH` or the default config loader. The checked-in [`talon.yaml`](./talon.yaml) and [`talon.docker-compose.yaml`](./talon.docker-compose.yaml) show the current supported shape:

- provider definitions under `providers`
- control-plane database driver and connection
- message broker driver
- optional object store configuration
- optional scheduler configuration
- required platform JWT private key and optional issuer override for asymmetric Talon-issued access tokens and JWKS

Common environment variables used by the runtime:

- `TALON_CONFIG_PATH`
- `POSTGRES_URL`
- `GCP_PROJECT_ID`
- `GRPC_ADDR`
- `PORT`
- `PULL_MODE`
- `TALON_SCHEDULER_DRIVER`
- `TALON_LOCAL_SCHEDULER_TARGET_URL`
- `TALON_LOCAL_SCHEDULER_RUNNER`
- `TALON_JWT_PRIVATE_KEY_PEM`
- `TALON_JWT_ISSUER`

Talon requires `TALON_JWT_PRIVATE_KEY_PEM` at startup for platform JWT signing,
gateway JWT verification, and JWKS publication. It publishes public key material
at `/.well-known/jwks.json` plus OAuth/OIDC metadata endpoints. JWT `iss`
defaults to `https://talon.impala.systems` and can be overridden with
`TALON_JWT_ISSUER`. JWKS proves a token was signed by Talon; gateway
authorization still requires `aud: "talon.impala.systems"`. MCP auth broker
assertions use `aud: "mcps.talon.impala.systems"` and are rejected by gateway
auth.

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
cargo run --bin talon-cli -- --gateway http://localhost:50051 get agent support-docs --namespace chatgpt-app
cargo run --bin talon-cli -- --gateway http://localhost:50051 knowledge sync --namespace chatgpt-app --manifest manifests/examples/chatgpt-app/support-docs-template.yaml
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

## CI artifacts

GitHub CI validates Cargo, runtime image, and UI builds. On pushes to `main`, CI publishes Docker images to GHCR:

- `ghcr.io/impalasys/talon-runtime:latest`
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

For macOS `arm64`, the latest CI-built CLI can be installed locally as `talon` with an ad-hoc signature:

```bash
scripts/install_latest_macos_cli.sh
```

The script uses the GitHub CLI to download the `talon-darwin-arm64-<sha>` artifact for the latest `main` commit, verifies `SHA256SUMS`, applies a local `codesign --sign -` signature, and installs the `talon-cli` binary to `/usr/local/bin/talon`. This is not Developer ID signing or notarization; it is a convenience path for local CLI installs until release assets are fully signed and notarized. If CI for the latest commit is not successful yet, use:

```bash
scripts/install_latest_macos_cli.sh --latest-successful
```

## Documentation

Start here:

- [`docs/intro.md`](./docs/intro.md)
- [`docs/getting-started/quickstart.md`](./docs/getting-started/quickstart.md)
- [`docs/concepts/how-talon-works.md`](./docs/concepts/how-talon-works.md)
- [`docs/operations/local-development.md`](./docs/operations/local-development.md)
- [`docs/reference/index.md`](./docs/reference/index.md)

## License

Source files in this repository are marked `AGPL-3.0-only`. See [`SECURITY.md`](./SECURITY.md) and the repository licensing metadata for additional project policy.
