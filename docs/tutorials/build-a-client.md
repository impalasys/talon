---
title: Build a Client Against the Gateway
sidebar_position: 2
---

This tutorial maps Talon’s three client surfaces to concrete code paths in this repository.

Before you run the examples, clone the repository and create `.env`:

```bash
git clone https://github.com/impalasys/talon.git
cd talon
cp .env.example .env
```

Set `OPENAI_API_KEY` in `.env`, then start the local stack:

```bash
docker compose up --build -d
```

## Choose the right surface

Use:

- gRPC when you want a typed backend or integration service
- REST when you want straightforward CRUD with `curl` or ordinary HTTP clients
- the UI session surface when you want a browser chat client like Sightline

## Option 1: typed client with Connect

The UI end-to-end tests already create namespaces, agents, and sessions through the gateway. The working example lives in `ui/e2e/chat.spec.ts`.

The core flow is:

```ts
import { createClient } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { GatewayService } from "../proto/proto/gateway_pb";

const client = createClient(
  GatewayService,
  createGrpcWebTransport({ baseUrl: "http://127.0.0.1:18789" }),
);

await client.createNamespace({ name: "client-demo", recursive: true });

await client.createAgent({
  ns: "client-demo",
  name: "docs-agent",
  definition: {
    source: {
      case: "customSpec",
      value: {
        systemPrompt: "Answer from the tutorial client.",
        modelPolicy: {
          profiles: [
            {
              name: "default",
              model: { provider: "openai", name: "gpt-5.4-nano", temperature: 0.0 },
            },
          ],
        },
      },
    },
  },
});

const session = await client.createSession({ ns: "client-demo", agent: "docs-agent" });
```

Use this path when Talon is one service inside a larger typed system.

## Option 2: CRUD with REST

The gateway exposes REST-transcoded endpoints through Envoy on `http://localhost:18789`.

Create a session:

```bash
curl -sS http://localhost:18789/v1/ns/client-demo/agents/docs-agent/sessions \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"ns":"client-demo","agent":"docs-agent"}'
```

Fetch the session later:

```bash
curl -sS http://localhost:18789/v1/ns/client-demo/agents/docs-agent/sessions/<session-id>
```

Use this path for scripts, ops tooling, and quick integration tests.

## Option 3: browser chat with the UI session surface

For browser-native chat, create the session through CRUD first, then post messages to:

```text
POST /v1/ui/ns/<namespace>/agents/<agent>/sessions/<session-id>
```

Example request:

```bash
curl -sS http://localhost:18789/v1/ui/ns/client-demo/agents/docs-agent/sessions/<session-id> \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"messages":[{"content":"Summarize the namespace model."}]}'
```

The body shape is the same one used by Sightline and the copilot package.

## Fastest React path

If you want a working chat panel instead of building the stream parser yourself, use `@talonai/copilot`.

```tsx
import { TalonCopilot } from "@talonai/copilot";

export function App() {
  return (
    <TalonCopilot
      namespace="client-demo"
      agent="docs-agent"
      gatewayUrl="http://localhost:18789"
    />
  );
}
```

That component handles:

- session creation
- the UI session POST
- transcript hydration from the canonical session state
- streamed tool and reasoning events

The minimal package usage is documented in `packages/copilot/README.md`.

## What to avoid

- Do not invent a separate backend just to proxy browser chat unless you need app-specific auth or policy.
- Do not send browser chat traffic to the CRUD `message` route if you want the same behavior Sightline uses.
- Do not document schedule creation as a manifest-apply flow; Talon currently creates schedules through the gateway API.

## Read next

- [Build a ChatGPT-Style App](./chatgpt-app.md)
- [Runtime Surfaces](../reference/runtime-surfaces.md)
- [Sessions and execution](../concepts/sessions-and-streaming.md)
