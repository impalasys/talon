---
title: Tutorials
sidebar_position: 0
---

Talon has two tutorial tracks:

- **Core build steps** for learning the control plane and client surfaces quickly
- **System tutorials** for assembling realistic products on top of namespaces, agents, sessions, knowledge, MCP, schedules, and Sightline

## Core build steps

Start here if you have not run Talon locally before:

- [Build Your First Agent](./01-first-agent.md)
- [Build a Client Against the Gateway](./02-build-a-client.md)

These pages teach the basic runtime loop:

1. start the stack
2. create real resources through Talon APIs
3. run a session
4. inspect the result in Sightline

## System tutorials

Use these guides when you want a fuller product narrative:

- [Build a ChatGPT-Style App](./03-chatgpt-app.md)
- [Build a Channel Collaboration Room](./07-channel-collaboration.md)
- [Build a Marketing Agency](./06-marketing-agency.md)
- [Build a Customer Retention System](./05-customer-retention-system.md)
- [Build an Internal Ops Copilot](./04-internal-ops-copilot.md)

Each system tutorial includes:

- valid example assets in `talon/manifests/examples`
- exact commands for the parts the CLI supports directly
- SDK/clientset calls when a workflow needs direct API interaction
- one inspection/debugging path in Sightline

## Read next

- [How Talon Works](../02-concepts/01-how-talon-works.md)
- [Runtime Topology](../02-concepts/02-runtime-topology.md)
