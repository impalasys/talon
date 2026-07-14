---
title: Quickstart
sidebar:
  order: 1
---

Use this quickstart to run Talon locally with one `talon-node` process and send one prompt to the default agent.

## Prerequisites

- **[Git](https://git-scm.com/)**: clones the Talon repository and supports normal local development workflows.
- **[Rust](https://www.rust-lang.org/tools/install)** toolchain: builds `talon-node` and `talon-cli` from source.
- **[Protobuf](https://protobuf.dev/installation/)** compiler: compiles Talon's service, resource, event, and model definitions.
- **[OpenAI](https://platform.openai.com/api-keys)** API key: gives the default local agent a provider to call.

## 1. Clone the repository

```bash
git clone https://github.com/impalasys/talon.git
cd talon
```

## 2. Add your provider key

From the repository root:

```bash
cp .env.example .env
```

Edit `.env` and set:

```bash
OPENAI_API_KEY=your-real-api-key
```

Load it into your shell:

```bash
set -a
. ./.env
set +a
```

## 3. Build Talon

```bash
cargo build --locked --bin talon-node --bin talon-cli
```

Create a short `talon` helper for the commands below:

```bash
talon() {
  ./target/debug/talon-cli --gateway http://127.0.0.1:50051 "$@"
}
```

## 4. Write a local config

```bash
mkdir -p .talon/local

cat > .talon/local/config.yaml <<'EOF'
providers:
  openai:
    type: openai
    model: gpt-5.4-nano
    apiKey:
      source: env
      key: OPENAI_API_KEY
default_provider: openai
workspace_dir: .

control_plane:
  database:
    driver: sqlite
    data_dir: ./data
  message_broker:
    driver: local_socket
  object_store:
    driver: local
    path: ./objects
EOF
```

This keeps local runtime state under `.talon/local/`, including the SQLite database and Unix socket broker.

## 5. Start `talon-node`

In one terminal:

```bash
export TALON_CONFIG_PATH="$PWD/.talon/local/config.yaml"
export TALON_JWT_PRIVATE_KEY_PEM="$(cat ./src/control/security/test_rsa_private_key.pem)"
export TALON_JWT_ISSUER=https://talon.localhost
export GRPC_ADDR=127.0.0.1:50051
export TALON_WORKER_UNIX_SOCKET_PATH="$PWD/.talon/local/worker.sock"

./target/debug/talon-node
```

Keep this process running. It starts the gateway and colocated worker against SQLite plus a local Unix socket broker.

## 6. Create a local API key

In a second terminal:

```bash
talon() {
  ./target/debug/talon-cli --gateway http://127.0.0.1:50051 "$@"
}

BOOTSTRAP_TOKEN="$(TALON_JWT_ISSUER=https://talon.localhost \
  ./target/debug/talon-cli auth local-token \
  --private-key-pem-file ./src/control/security/test_rsa_private_key.pem)"

export TALON_API_KEY="$(talon --token "$BOOTSTRAP_TOKEN" \
  auth api-key create --name quickstart --grant readwrite \
  | awk -F= '/^secret=/{print $2}')"
```

`talon-cli` reads `TALON_API_KEY` automatically.

## 7. Apply the default manifests

```bash
talon apply -f manifests/default
```

This creates `Namespace/default`, `Template/default`, and `Agent/default/main`.

## 8. Send a prompt

```bash
talon session prompt \
  --namespace default \
  --agent main \
  --stream \
  "Explain what Talon is in two bullets."
```

## 9. Inspect the run

The streamed response came from a durable Talon session. You can inspect it from the CLI:

```bash
talon get agent main --namespace default
```

## What happened?

`talon-node` started the gateway and worker in one process. The local config used SQLite for durable state and a Unix socket broker for same-host dispatch. You applied the default manifest directory, then ran a prompt through `Agent/default/main`.

## Next steps

- [Create your first agent](../tutorials/first-agent)
- [Learn the resource model](../concepts/resource-model)
- [Use the CLI](../reference/cli)
- [Configure providers and runtime settings](../operations/configuration)
