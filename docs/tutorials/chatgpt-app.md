---
title: Build a ChatGPT-Style App
sidebar_position: 3
---

This tutorial uses the repository’s example assets plus the browser session API to build a real product-docs chat app.

## What you are building

You will create:

- a dedicated `chatgpt-app` namespace
- a `support-docs-agent` agent backed by a real provider from `.env`
- namespace knowledge loaded from markdown files
- a browser client that talks to Talon’s UI session API

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

Open Sightline at `http://localhost:3000` and connect it to `http://localhost:18789`.

## 2. Apply the app resources

Apply the example manifests one by one:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest apply -f manifests/examples/chatgpt-app/namespace.yaml
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest apply -f manifests/examples/chatgpt-app/support-docs-template.yaml
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest apply -f manifests/examples/chatgpt-app/support-docs-agent.yaml
```

## 3. Load the product docs as knowledge

Sync the tutorial knowledge into the namespace:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest knowledge sync \
  --namespace chatgpt-app \
  --dir manifests/examples/chatgpt-app/knowledge
```

Verify one document loaded:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest knowledge get \
  --namespace chatgpt-app \
  --path product-docs.md
```

## 4. Create a session

```bash
curl -sS http://localhost:18789/v1/ns/chatgpt-app/agents/support-docs-agent/sessions \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"ns":"chatgpt-app","agent":"support-docs-agent"}'
```

Copy the returned `sessionId`.

## 5. Send a browser-style chat request

Replace `<session-id>`:

```bash
curl -sS http://localhost:18789/v1/ui/ns/chatgpt-app/agents/support-docs-agent/sessions/<session-id> \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"messages":[{"content":"Summarize the product docs in three bullets."}]}'
```

This is the same route a frontend would call.

## 6. Wire a React client

The fastest frontend path is the shared copilot component:

```tsx
import { TalonCopilot } from "@impalasys/talon-chat";

export function SupportApp() {
  return (
    <TalonCopilot
      namespace="chatgpt-app"
      agent="support-docs-agent"
      gatewayUrl="http://localhost:18789"
    />
  );
}
```

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
- the browser chat route is the real gateway UI surface
- the manifests point at a real OpenAI provider already wired through `talon.compose.yaml`

## What is intentionally not included

- tenant auth
- production retrieval infrastructure
- custom MCP tools

Add those after the basic chat loop works.

## Read next

- [Build a Marketing Agency](./marketing-agency.md)
- [Build a Client Against the Gateway](./build-a-client.md)
- [Authentication and Access](../operations/authentication-and-access.md)
