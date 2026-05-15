---
title: Build a ChatGPT-Style App
sidebar_position: 3
---

This tutorial shows how to build a browser chat product on top of Talon’s existing session and streaming surfaces.

## What you are building

You will build a single-agent app with:

- a dedicated namespace for app resources
- one conversational agent or template
- optional knowledge-backed grounding
- a browser client that talks to Talon’s UI session API
- live inspection through Sightline

The closest fit is a “ChatGPT for my product docs” app.

## Talon concepts used

- namespace
- agent template and agent
- session
- streamed step events
- knowledge
- browser-oriented UI session API
- Sightline

## Runtime surfaces used

- `talon-cli apply` for declarative setup
- the gateway for control-plane resources
- the `/v1/ui/...` session surface for browser chat
- Sightline on `http://localhost:3000` for inspection

Read these first if the surface split is unfamiliar:

- [Runtime Surfaces](../reference/runtime-surfaces.md)
- [Sessions and execution](../concepts/sessions-and-streaming.md)

## Architecture

```text
Browser chat UI
  -> Envoy edge :18789
    -> Gateway UI session surface
      -> Session state in Postgres-backed control plane
      -> Worker for model/tool execution
      -> Optional knowledge/tool lookups
```

The browser should use Talon’s UI session surface directly. Do not invent a separate proxy backend just to stream chat unless you need app-specific policy or auth.

## Prerequisites

Start the local stack:

```bash
cd talon
./run.sh
```

Open Sightline at `http://localhost:3000` and connect it to `http://localhost:18789`.

## Apply the example assets

This tutorial ships with example assets in:

- `talon/manifests/examples/chatgpt-app/app.yaml`
- `talon/manifests/examples/chatgpt-app/knowledge/product-docs.md`

Use them as a starting point:

```bash
cd talon
cargo run --bin talon-cli -- --rest apply -f manifests/examples/chatgpt-app/app.yaml
```

The bundle defines:

- a namespace for the app
- a support-docs agent template
- an app-facing agent
- optional placeholder MCP bindings you can swap for real search or retrieval tools

## Build the frontend

The fastest frontend shape is:

1. create or look up a session for your agent
2. `POST` user messages to the UI session route
3. stream step events into the transcript
4. render tool activity inline when present

Model your client after Talon’s own UI behavior:

- the local UI already uses the browser-native session flow
- the existing chat e2e tests demonstrate streamed tool activity and assistant responses

If you need a deeper surface comparison, read [Build a Client Against the Gateway](./build-a-client.md).

## Walk an end-to-end flow

Use the example app resources and send prompts like:

- “Summarize Talon in three bullets”
- “What ports does the local stack expose?”
- “What is the difference between the gateway and Sightline?”

Watch for:

- the created session
- streamed assistant output
- optional tool-start and tool-result events
- persisted session history in Sightline

## Inspect and debug in Sightline

In Sightline, inspect:

- the tutorial namespace
- the agent/template definitions
- the live session
- streamed steps for the run

When something looks wrong:

- verify you connected Sightline to `http://localhost:18789`
- verify the namespace and agent exist
- verify your frontend is using the UI session path rather than a control-plane CRUD route

## Extend the system

Good next steps:

- attach real product docs as knowledge
- add one MCP-backed retrieval or search tool
- add auth in front of the browser app
- split the agent into support, sales, and onboarding variants

## Production notes

For a deployed product, add:

- a real auth story in front of the browser app
- explicit namespace strategy for tenants or workspaces
- bounded MCP bindings instead of broad tool access

## What you learned

You used Talon as a browser chat backend with durable sessions and observable execution, not just a raw model wrapper.

## Read next

- [Build a Marketing Agency](./marketing-agency.md)
- [Using Sightline](../concepts/using-sightline.md)
- [Authentication and Access](../operations/authentication-and-access.md)
