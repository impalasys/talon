---
title: Build Your First Agent
sidebar_position: 1
---

This tutorial gets you to a working Talon agent with real commands and a real local inspection loop.

## What you will build

By the end you will have:

- the local Talon stack running
- a tutorial namespace
- a real-provider agent using credentials from `.env`
- a session you can inspect in Sightline

## Before you start

Clone the repository:

```bash
git clone https://github.com/impalasys/talon.git
cd talon
```

Create a local `.env` file:

```bash
cp .env.example .env
```

Then set a real provider key. This tutorial uses the `openai` provider already defined in `talon.docker-compose.yaml`:

```bash
OPENAI_API_KEY=your-real-api-key
```

From the repository root:

```bash
docker compose up --build -d
```

Wait for the stack to come up, then open `http://localhost:3000` and connect Sightline to `http://localhost:50051`.

## 1. Create a tutorial namespace

Create `first-agent-namespace.yaml`:

```yaml
apiVersion: talon.impalasys.com/v1
kind: Namespace
metadata:
  name: first-agent
```

Apply it:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f first-agent-namespace.yaml
```

Create `first-agent-agent.yaml`:

```yaml
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: hello-agent
  namespace: first-agent
spec:
    systemPrompt: |
      You are a concise tutorial assistant for Talon.
      Explain what you are doing and keep answers short.
    modelPolicy:
      profiles:
        - name: default
          model:
            provider: openai
            name: gpt-5.4-nano
            temperature: 0.0
```

Apply it:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f first-agent-agent.yaml
```

Verify the agent exists:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 get agent hello-agent --namespace first-agent
```

## 2. Send a message

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 session prompt \
  --namespace first-agent \
  --agent hello-agent \
  --label source=tutorial \
  --stream \
  "Explain what Talon is in two bullets."
```

This creates a durable session, sends the prompt, and streams `SessionService.StreamParts` events from the gateway.

## 3. Inspect the result in Sightline

In Sightline:

1. open the `first-agent` namespace
2. select `hello-agent`
3. open the session you just created

Look for:

- the persisted user message
- the assistant reply
- any streamed reasoning or tool steps

## 4. Understand the gateway surface

You just used the typed Talon gateway:

- native gRPC for the CLI
- gRPC-Web for browser clients such as Sightline and `@impalasys/talon-chat`
- named `talon.v1` services such as `SessionService`, `ResourceService`, and `NamespaceService`

There is no separate JSON-transcoded gateway route in the local stack.

## Troubleshooting

- If `apply` fails, check that your manifest uses `metadata.namespace` for `Agent` resources.
- If the session request fails, confirm the gateway is listening on `http://localhost:50051`.
- If Sightline connects but shows nothing, refresh after creating the namespace and session.

## Read next

- [Build a Client Against the Gateway](./build-a-client.md)
- [Build a ChatGPT-Style App](./chatgpt-app.md)
- [Using Sightline](../concepts/using-sightline.md)
