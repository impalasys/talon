---
title: Quickstart
sidebar_position: 1
---

This is the fastest path to a working Talon stack in the monorepo while still understanding what just started.

## Prerequisites

- Docker / Docker Compose
- Rust toolchain for local binaries and CLI work
- provider credentials via `.env` or local keychain

## 1. Create `.env`

From the repository root:

```bash
cp .env.example .env
```

Then edit `.env` and set at least one real provider key. The checked-in local stack is wired to the `novita` provider in `talon.compose.yaml`, so the quickstart examples below use that configuration:

```bash
NOVITA_API_KEY=your-real-api-key
```

## 2. Start Talon locally

From the repository root:

```bash
docker compose up --build -d
```

This starts the local compose stack and exposes:

- Sightline UI: `http://localhost:3000`
- Envoy edge: `http://localhost:18789`
- native gRPC gateway: `http://localhost:50051`
- gateway UI HTTP surface: `http://localhost:50052`

It also starts:

- a worker process
- Postgres
- a Pub/Sub emulator
- an init step that applies the default agent template manifest

## 3. Open Sightline

Open `http://localhost:3000` and connect to `http://localhost:18789`.

Use Sightline to inspect:

- namespaces
- agents
- sessions
- schedules
- templates
- knowledge resources
- MCP servers and bindings

This is the fastest way to see Talon’s runtime model in action rather than only reading the APIs.

## 4. Create a namespace

Create `quickstart-namespace.yaml`:

```yaml
apiVersion: talon.impalasys.com/v1
kind: Namespace
metadata:
  name: quickstart
```

Apply it:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest apply -f quickstart-namespace.yaml
```

## 5. Create an agent directly

The quickstart does not require an agent template first. Create `quickstart-agent.yaml`:

```yaml
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: hello-agent
  namespace: quickstart
definition:
  customSpec:
    systemPrompt: |
      You are a concise quickstart assistant for Talon.
      Answer directly and keep the response short.
    modelPolicy:
      profiles:
        - name: default
          model:
            provider: novita
            name: google/gemma-4-31b-it
            temperature: 0.0
```

Apply it:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest apply -f quickstart-agent.yaml
```

Verify it exists:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest get agent hello-agent --namespace quickstart
```

## 6. Create a session

Create a session through the gateway REST surface:

```bash
curl -sS http://localhost:18789/v1/ns/quickstart/agents/hello-agent/sessions \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"ns":"quickstart","agent":"hello-agent"}'
```

The response includes a `sessionId`.

## 7. Chat with the agent over `curl`

Replace `<session-id>` with the value from the previous step:

```bash
curl -sS http://localhost:18789/v1/ui/ns/quickstart/agents/hello-agent/sessions/<session-id> \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"messages":[{"content":"Explain what Talon is in two bullets."}]}'
```

This uses the same browser-oriented UI session surface that Sightline and `@talonai/copilot` use.

## 8. Inspect the run in Sightline

In Sightline:

1. open the `quickstart` namespace
2. select `hello-agent`
3. open the session you just created

Look for the persisted messages and streamed execution steps.

## 9. Read the contracts

- [How Talon Works](../concepts/how-talon-works.md)
- [Runtime Topology](../concepts/runtime-topology.md)
- [Architecture](./architecture.md)
- [Gateway API reference](../reference/generated/gateway-service.md)
- [Manifest schema](../reference/generated/manifests-schema.md)
- [Config schema](../reference/generated/config-schema.md)

## What you learned

After the quickstart, you should know:

- which processes Talon starts locally
- which ports correspond to UI, edge, gRPC, and UI-session traffic
- how to create an agent directly without introducing an agent template first
- how to create a session and send a browser-style chat request with `curl`
- where to inspect runtime resources
- where to go next for deeper runtime or API detail
