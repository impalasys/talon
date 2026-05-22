---
title: Build Your First Agent
sidebar_position: 1
---

This tutorial gets you to a working Talon agent with real commands and a real local inspection loop.

## What you will build

By the end you will have:

- the local Talon stack running
- a tutorial namespace
- a mock-model agent that works without external provider credentials
- a session you can inspect in Sightline

## Before you start

From the repository root:

```bash
./run.sh
```

Wait for the stack to come up, then open `http://localhost:3000` and connect Sightline to `http://localhost:18789`.

## 1. Create a tutorial namespace

Create a file named `first-agent.yaml`:

```yaml
apiVersion: talon.impalasys.com/v1
kind: Namespace
metadata:
  name: first-agent
---
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: hello-agent
  namespace: first-agent
definition:
  customSpec:
    systemPrompt: |
      You are a concise tutorial assistant for Talon.
      Explain what you are doing and keep answers short.
    modelPolicy:
      profiles:
        - name: default
          model:
            provider: mock
            name: minimax
            temperature: 0.0
```

Apply it:

```bash
cargo run --bin talon-cli -- --rest apply -f first-agent.yaml
```

Verify the agent exists:

```bash
cargo run --bin talon-cli -- --rest get agent hello-agent --namespace first-agent
```

## 2. Create a session

Create a session through the gateway REST surface:

```bash
curl -sS http://localhost:18789/v1/ns/first-agent/agents/hello-agent/sessions \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"ns":"first-agent","agent":"hello-agent","labels":{"source":"tutorial"}}'
```

The response includes a `sessionId`. Save it for the next step.

## 3. Send a message

Replace `<session-id>` with the value from the previous step:

```bash
curl -sS http://localhost:18789/v1/ui/ns/first-agent/agents/hello-agent/sessions/<session-id> \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"messages":[{"content":"Explain what Talon is in two bullets."}]}'
```

The UI surface returns a streamed response format used by Sightline and `@talonai/copilot`.

## 4. Inspect the result in Sightline

In Sightline:

1. open the `first-agent` namespace
2. select `hello-agent`
3. open the session you just created

Look for:

- the persisted user message
- the assistant reply
- any streamed reasoning or tool steps

## 5. Understand the control-plane surfaces

You just used two different Talon APIs:

- `POST /v1/ns/.../sessions` to create the durable session resource
- `POST /v1/ui/ns/.../sessions/<id>` to drive the browser-style chat flow

That split is intentional. CRUD happens on the control-plane API. Browser chat happens on the UI session API.

## Troubleshooting

- If `apply` fails, check that your manifest uses `metadata.namespace` for `Agent` resources.
- If the UI request fails, confirm you are calling the `v1/ui` route, not the CRUD `message` route.
- If Sightline connects but shows nothing, refresh after creating the namespace and session.

## Read next

- [Build a Client Against the Gateway](./build-a-client.md)
- [Build a ChatGPT-Style App](./chatgpt-app.md)
- [Using Sightline](../concepts/using-sightline.md)
