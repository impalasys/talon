# Talon

Talon is a Rust-based agent runtime and control plane. It provides a gateway API, a worker runtime, durable session state, namespace-scoped resources, manifests, and a browser UI for inspection and operations.

This repository is the open-source home for Talon runtime, CLI, manifests, and UI code.

## What is in this repo

- Rust gateway, worker, and CLI binaries in `src/`
- Protocol buffer contracts in `proto/`
- Example manifests in `manifests/`
- Operator UI in `ui/`
- Builder and operator docs in `docs/`
- Reference-generation helpers in `docs-site/`

## Architecture at a glance

- `gateway`: the canonical API surface over gRPC plus HTTP-transcoded routes
- `worker`: executes agent turns and background work
- `envoy`: browser-facing edge surface for local and deployed environments
- `ui`: the Next.js operator UI for inspecting agents, sessions, schedules, knowledge, and MCP bindings
- `postgres` and `pubsub`: control-plane backing services used by the local stack

For the system model, start with [docs/intro.md](docs/intro.md), [docs/concepts/how-talon-works.md](docs/concepts/how-talon-works.md), and [docs/concepts/runtime-topology.md](docs/concepts/runtime-topology.md).

## Quickstart

From the repository root:

```bash
cp .env.example .env
```

Set a real provider key in `.env`:

```bash
OPENAI_API_KEY=your-real-api-key
```

Then start the stack directly with Docker Compose:

```bash
docker compose up --build -d
```

This starts the local Talon stack, including:

- Sightline UI on `http://localhost:3000`
- Envoy edge on `http://localhost:18789`
- Native gRPC gateway on `http://localhost:50051`
- Gateway UI HTTP surface on `http://localhost:50052`

The fastest next step is [docs/getting-started/quickstart.md](docs/getting-started/quickstart.md).

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

### Generated reference docs

If you change `proto/gateway.proto`, `proto/config.proto`, or `proto/manifests.proto`, regenerate the checked-in reference pages:

```bash
pnpm --filter @impalasys/talon-docs generate:reference
```

### Docker validation

```bash
docker build -f dockerfiles/oss-runtime.Dockerfile .
docker build -f dockerfiles/oss-ui.Dockerfile .
```

To validate the Envoy image:

```bash
protoc -I. -Iproto -Ithird_party/googleapis \
  --include_imports \
  --include_source_info \
  --experimental_allow_proto3_optional \
  --descriptor_set_out=talon_gateway_proto-descriptor-set.proto.bin \
  proto/gateway.proto

docker build -f dockerfiles/envoy-cloudrun.Dockerfile .
```

## Documentation map

- [docs/intro.md](docs/intro.md): starting point
- [docs/getting-started/quickstart.md](docs/getting-started/quickstart.md): local bring-up
- [docs/tutorials/first-agent.md](docs/tutorials/first-agent.md): first end-to-end agent flow
- [docs/reference/index.md](docs/reference/index.md): API and schema reference
- [docs/operations/local-development.md](docs/operations/local-development.md): operator-focused local workflow
- [docs/contributing/docs-system.md](docs/contributing/docs-system.md): how docs are maintained

## License

This repository is licensed under the GNU Affero General Public License v3.0. See `LICENSE`.
