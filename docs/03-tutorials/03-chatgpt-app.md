---
title: Build a ChatGPT-Style App
sidebar_position: 3
---

This tutorial uses the repository’s example assets plus Talon’s gRPC-Web client path to build a real product-docs chat app.

## What you are building

You will create:

- a dedicated `chatgpt-app` namespace
- a `support-docs-agent` agent backed by a real provider from `.env`
- namespace knowledge loaded from markdown files
- a browser client that talks to Talon’s `talon.v1` gRPC-Web API

The point is to use Talon as the chat backend, not just as a prompt store.

## 1. Clone the repository and start the stack

Clone the repository:

```bash
git clone https://github.com/impalasys/talon.git
cd talon
```

Create `.env` first:

```bash
cp .env.example .env
```

Then set a real provider key:

```bash
OPENAI_API_KEY=your-real-api-key
```

From the repository root:

```bash
docker compose up --build -d
```

Open Sightline at `http://localhost:3000` and connect it to `http://localhost:50051`.

## 2. Apply the app resources

Apply the example manifests one by one:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/chatgpt-app/namespace.yaml
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/chatgpt-app/support-docs-template.yaml
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/chatgpt-app/support-docs-agent.yaml
```

## 3. Load the product docs as knowledge

Sync the tutorial knowledge into the namespace:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 knowledge sync \
  --namespace chatgpt-app \
  --dir manifests/examples/chatgpt-app/knowledge
```

Verify one document loaded:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 knowledge get \
  --namespace chatgpt-app \
  --path product-docs.md
```

## 4. Try the agent from the CLI

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 session prompt \
  --namespace chatgpt-app \
  --agent support-docs-agent \
  --stream \
  "Summarize the product docs in three bullets."
```

This creates a durable session and streams the assistant reply over gRPC.

## 5. Wire a React client

The fastest frontend path is the shared copilot component:

```tsx
import { TalonCopilot } from "@impalasys/talon-chat";
import { createTalonClient } from "@impalasys/talon-client";

const talon = createTalonClient({ baseUrl: "http://localhost:50051" });

export function SupportApp() {
  return (
    <TalonCopilot
      namespace="chatgpt-app"
      agent="support-docs-agent"
      gatewayClient={talon}
    />
  );
}
```

`TalonCopilot` expects a Talon clientset-compatible gateway and uses `SessionService.SubmitTurn`, `StreamParts`, and `ListMessages` under the hood.

See `packages/talon-chat/README.md` for the minimal integration shape.

## 7. Inspect the app in Sightline

In Sightline, inspect:

- the `chatgpt-app` namespace
- the `support-docs-agent` agent
- the `product-docs.md` knowledge artifact
- the live session transcript

This is the fastest way to debug whether the browser app and the control plane agree.

## What is real in this example

- the manifest files are valid `talon-cli apply` inputs
- the knowledge sync command is real
- the browser chat path uses the real gRPC-Web gateway
- the manifests point at a real OpenAI provider already wired through `talon.docker-compose.yaml`

## What is intentionally not included

- tenant auth
- production retrieval infrastructure
- custom MCP tools

Add those after the basic chat loop works.

## Read next

- [Build a Marketing Agency](./06-marketing-agency.md)
- [Build a Client Against the Gateway](./02-build-a-client.md)
- [Authentication and Access](../04-operations/04-authentication-and-access.md)
