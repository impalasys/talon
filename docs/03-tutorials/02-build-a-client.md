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
- gRPC-Web when you want browser clients such as Sightline or `@impalasys/talon-chat`
- the SDK clientset when you want one transport with domain services like `talon.sessions` and `talon.channels`

## Option 1: typed client with the SDK

The UI end-to-end tests already create namespaces, agents, and sessions through the gateway. The working example lives in `ui/e2e/chat.spec.ts`.

The core flow is:

```ts
import { agents, common, createTalonClient, resources, v1 } from "@impalasys/talon-client";

const talon = createTalonClient("http://127.0.0.1:50051");

await talon.namespaces.create({ name: "client-demo", recursive: true });

await talon.resources.create(new v1.CreateResourceRequest({
  ns: "client-demo",
  manifest: new resources.ResourceManifest({
    apiVersion: "talon.impalasys.com/v1",
    kind: "Agent",
    metadata: new common.ResourceMeta({ namespace: "client-demo", name: "docs-agent" }),
    spec: new resources.ResourceSpec({
      kind: {
        case: "agent",
        value: new agents.AgentSpec({
          systemPrompt: "Answer from the tutorial client.",
          modelPolicy: new agents.ModelPolicy({
            profiles: [
              new agents.ModelProfile({
                name: "default",
                model: new agents.Model({ provider: "openai", name: "gpt-5.4-nano", temperature: 0.0 }),
              }),
            ],
          }),
        }),
      },
    }),
  }),
}));

const session = await talon.sessions.create({ ns: "client-demo", agent: "docs-agent" });
```

Use this path when Talon is one service inside a larger typed system. Browser clients use gRPC-Web through the SDK helper; backend services can use native gRPC.

## Option 2: send a turn through the SDK

For chat-style UX, send a user message with `SessionService.SubmitTurn`. The method returns a stream of `SessionMessagePartEvent` values.

```ts
import { data } from "@impalasys/talon-client";

let assistantText = "";
for await (const event of talon.sessions.submitTurn({
  ns: "client-demo",
  agent: "docs-agent",
  sessionId: session.sessionId,
  message: new data.SessionMessage({
    role: data.MessageRole.ROLE_USER,
    parts: [
      new data.SessionMessagePart({
        partType: data.SessionMessagePartType.TEXT,
        content: "Summarize the namespace model.",
      }),
    ],
  }),
  labels: {},
})) {
  const part = event.part;
  if (part?.partType === data.SessionMessagePartType.TEXT) {
    assistantText += part.content;
  }
}
console.log(assistantText);
```

Use `talon.sessions.listMessages(...)` to hydrate history and `talon.sessions.streamParts(...)` to resume a live stream.

## Option 3: fastest React path

If you want a working chat panel instead of building the stream parser yourself, use `@impalasys/talon-chat`.

```tsx
import { createTalonClient } from "@impalasys/talon-client";
import { TalonCopilot } from "@impalasys/talon-chat";

const talon = createTalonClient("http://localhost:50051");

export function App() {
  return (
    <TalonCopilot
      namespace="client-demo"
      agent="docs-agent"
      gatewayClient={talon}
    />
  );
}
```

That component handles:

- session creation
- `SessionService.SubmitTurn`
- `SessionService.StreamParts`
- transcript hydration from the canonical session state
- streamed tool and reasoning events

The minimal package usage is documented in `packages/talon-chat/README.md`.

## What to avoid

- Avoid inventing a separate backend just to proxy browser chat unless you need app-specific auth or policy.
- Keep Talon operations on the gRPC/gRPC-Web gateway instead of adding HTTP routes.
- Use one Talon clientset and access named services from it, rather than creating a separate gRPC transport per service.

## Read next

- [Build a ChatGPT-Style App](./03-chatgpt-app.md)
- [Runtime Surfaces](../05-reference/02-runtime-surfaces.md)
- [Sessions and execution](../02-concepts/05-sessions-and-streaming.md)
