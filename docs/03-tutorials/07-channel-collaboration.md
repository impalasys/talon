---
title: Build a Channel Collaboration Room
sidebar:
  order: 5
---

This tutorial shows Talon channels as the shared public chat layer above agent-owned sessions.

## What you will build

You will create:

- a `channel-collaboration` namespace
- an `incident-room` channel
- a `triage-agent` subscription triggered by mentions
- a `scribe-agent` subscription triggered manually
- channel-routed sessions where agents decide whether to publish a public reply

## Start the stack with the channel tutorial

Create `.env` from `.env.example` and set `OPENAI_API_KEY`, then start the local stack with the optional channel tutorial profile:

```bash
docker compose --profile tutorial-channels up --build -d
```

This starts the normal local stack and runs a one-shot bootstrap service that applies the channel tutorial manifests from `manifests/examples/channel-collaboration`.

Open Sightline at `http://localhost:3000` and connect it to `http://localhost:50051`.

## 1. Inspect the resources

In Sightline, expand the `channel-collaboration` namespace.

You should see:

- `triage-agent`
- `scribe-agent`
- `incident-room`
- `triage` and `scribe` subscriptions under the expanded channel

## 2. Post a mention-routed message

Post into the channel:

```ts
import { createTalonClient } from "@impalasys/talon-client";

const talon = createTalonClient("http://localhost:50051");

await talon.channels.postMessage({
  ns: "channel-collaboration",
  channel: "incident-room",
  authorKind: "user",
  author: "operator",
  content: "@triage-agent production checkout latency is elevated. What should we do first?",
});
```

The `triage` subscription routes this public message into a new private session owned by `triage-agent`.

## 3. Post a manually routed message

Route a message to the `scribe` subscription:

```ts
await talon.channels.postMessage({
  ns: "channel-collaboration",
  channel: "incident-room",
  authorKind: "user",
  author: "operator",
  content: "Summarize the current incident room for handoff.",
  subscriptionNames: ["scribe"],
});
```

The manual route creates a separate `scribe-agent` session without requiring an `@scribe-agent` mention.

## 4. Inspect channel output

In Sightline:

1. select `incident-room`
2. use the Messages tab to see public channel messages
3. expand `triage-agent` or `scribe-agent` to inspect the private sessions created by channel routing

Normal assistant text stays inside the private session. A public channel reply appears only when an agent explicitly calls `channel_publish`.
Set `replyMode: none` on a `ChannelSubscription` when a routed session should observe or process the channel message without receiving channel reply tools.

## Why this structure matters

Channels are the multiplayer layer. Sessions remain the durable execution record for a single agent. A `ChannelSubscription` is the bridge that decides when a public channel message should create an agent-owned session.
