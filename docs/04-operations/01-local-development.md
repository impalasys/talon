---
title: Local Development
sidebar_position: 1
---

## Docker Compose variant

Use Docker Compose when you want the full local stack, including Sightline, Postgres, the Pub/Sub emulator, and bootstrap jobs.

### Prerequisites

- **[Docker](https://www.docker.com/)** and Docker Compose
- **[Git](https://git-scm.com/)**
- **[Rust](https://www.rust-lang.org/tools/install)** toolchain for host-side `talon-cli`
- **[OpenAI](https://platform.openai.com/api-keys)** API key

### 1. Configure the provider

From the repository root:

```bash
cp .env.example .env
```

Edit `.env` and set:

```bash
OPENAI_API_KEY=your-real-api-key
```

### 2. Start the stack

From the repository root:

```bash
docker compose up --build -d
```

This starts the local compose stack and brings up:

- the gateway
- the worker
- Sightline UI
- Postgres
- the Pub/Sub emulator
- a shared `talon-objects` volume for local object storage
- the default namespace and agent bootstrap

### 3. Export the local API key

```bash
export TALON_API_KEY="$(docker compose run --rm --no-deps init-api-key 'cat /data/talon/auth/api-key')"
```

`talon-cli` reads `TALON_API_KEY` automatically. For shorter commands:

```bash
talon() {
  cargo run --bin talon-cli -- --gateway http://localhost:50051 "$@"
}
```

### 4. Verify the default agent

The `init-manifests` job applies `manifests/default`, which creates `Namespace/default`, `Template/default`, and `Agent/default/main`.

```bash
talon get agent main --namespace default
```

If you need to reapply the bootstrap resources:

```bash
talon apply -f manifests/default
```

### 5. Send a prompt

```bash
talon session prompt \
  --namespace default \
  --agent main \
  --stream \
  "Explain what Talon is in two bullets."
```

### 6. Inspect in Sightline

Open `http://localhost:3000` and connect to `http://localhost:50051`.

Use the API key from the bootstrap step when the auth modal asks for credentials.

## SQLite and `talon-node` development

The Docker Compose stack uses Postgres and the Pub/Sub emulator. For the smallest local runtime, use `talon-node` with SQLite and a local Unix socket broker, as shown in the [quickstart](../01-getting-started/01-quickstart.md).

If you want to run Talon directly on a single machine without Postgres, configure:

- `control_plane.database.driver: sqlite`
- `control_plane.database.data_dir: <local directory>`
- `control_plane.message_broker.driver: local_socket`
- `control_plane.object_store.driver: local`, or the YAML alias `storage.objects.driver: local`
- `TALON_SCHEDULER_DRIVER=local_sqlite`

Keep the SQLite database on a local filesystem and run the gateway and worker on the same host.

For a complete command-by-command walkthrough, see the draft wiki note
[Local Single-Host Development Without Docker](./02-single-host-development.md).

## Useful endpoints

- Gateway edge: `http://localhost:50051`
- Sightline UI: `http://localhost:3000`
- pgAdmin database UI: `http://localhost:5050`

## Common tasks

- Inspect the [gateway service reference](../05-reference/generated/gateway-service.md) when adding or consuming API surface
- Use Sightline to verify sessions, schedules, namespaces, and tool activity
- Use the CLI for admin flows that are easier from the terminal than the UI

## Useful runtime ports

- `3000`: Sightline UI
- `50051`: gateway (native gRPC and gRPC-Web)
- `5050`: pgAdmin, when the `pgadmin` service is running

## Docs workflow

- Hand-written docs live in `docs/`.
- Generated reference pages live in `docs/05-reference/generated/`.
- If you change the gateway or schema protos, regenerate the reference pages with `pnpm --filter @impalasys/talon-docs generate:reference`.
- Use the docs markdown itself as the source of truth for this open-source repository.
