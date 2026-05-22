---
title: Tutorials
sidebar_position: 0
---

Talon has two tutorial tracks:

- **Core build steps** for learning the control plane and client surfaces quickly
- **System tutorials** for assembling realistic products on top of namespaces, agents, sessions, knowledge, MCP, schedules, and Sightline

## Core build steps

Start here if you have not run Talon locally before:

- [Build Your First Agent](./first-agent.md)
- [Build a Client Against the Gateway](./build-a-client.md)

These pages teach the basic runtime loop:

1. start the stack
2. create real resources through Talon APIs
3. run a session
4. inspect the result in Sightline

## System tutorials

Use these guides when you want a fuller product narrative:

- [Build a ChatGPT-Style App](./chatgpt-app.md)
- [Build a Marketing Agency](./marketing-agency.md)
- [Build a Customer Retention System](./customer-retention-system.md)
- [Build an Internal Ops Copilot](./internal-ops-copilot.md)

Each system tutorial includes:

- valid example assets in `talon/manifests/examples`
- exact commands for the parts the CLI supports directly
- explicit REST calls when the workflow is not manifest-applied
- one inspection/debugging path in Sightline

## Read next

- [How Talon Works](../concepts/how-talon-works.md)
- [Runtime Topology](../concepts/runtime-topology.md)
