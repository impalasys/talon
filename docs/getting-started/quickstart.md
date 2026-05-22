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

Then edit `.env` and set at least one real provider key. The tutorial examples in this repo use OpenAI by default:

```bash
OPENAI_API_KEY=your-real-api-key
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

## 4. Create or inspect an agent

Talon models runtime resources around namespaces and agents. The default operator flow is:

1. choose a namespace
2. select or create an agent
3. create a session
4. send a message
5. stream the response and tool activity

## 5. Try the CLI

The admin CLI targets the native gRPC gateway by default:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 get agenttemplate <name>
```

If you want the HTTP-transcoded surface instead:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest get agenttemplate <name>
```

## 6. Read the contracts

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
- where to inspect runtime resources
- where to go next for deeper runtime or API detail
