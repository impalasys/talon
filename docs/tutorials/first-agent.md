---
title: Build Your First Agent
sidebar_position: 1
---

This tutorial uses the local Talon stack and the built-in runtime surfaces already present in this repo.

## Goal

By the end you should be able to:

- run Talon locally
- choose a namespace
- create or inspect an agent
- start a session
- stream a response in Sightline

## Step 1: start the stack

```bash
cd talon
./run.sh
```

This brings up the gateway, worker, UI, edge proxy, persistence, and the default agent template bootstrap.

## Step 2: connect Sightline

Open `http://localhost:3000` and connect it to `http://localhost:18789`.

## Step 3: inspect the control plane

In Sightline, inspect:

- namespaces
- agent templates
- agents
- schedules
- knowledge
- MCP servers

At this point, focus on understanding the resource model:

- templates define reusable behavior
- agents are the runtime resources you interact with
- sessions are the durable execution units

## Step 4: create a session

Use an existing agent in a namespace and create a new session. Send a prompt and watch:

- the assistant response stream
- tool activity if the agent invokes MCP-backed tools

## Step 5: inspect the contract

For the equivalent control-plane calls, read:

- [Gateway API reference](../reference/generated/gateway-service.md)
- [Sessions and streaming](../concepts/sessions-and-streaming.md)

## Read next

- [Build a Client Against the Gateway](./build-a-client.md)
- [Build a ChatGPT-Style App](./chatgpt-app.md)
- [Using Sightline](../concepts/using-sightline.md)
