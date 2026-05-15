---
title: Build a Client Against the Gateway
sidebar_position: 2
---

Talon’s canonical contract is gRPC, but the gateway also exposes REST mappings and a browser-oriented UI session path.

## Pick your client strategy

Use:

- **gRPC** when you want strong typed service integration
- **REST** when you want simpler HTTP access to control-plane operations
- **browser-native UI session endpoints** when you want a frontend client similar to Sightline or an AI SDK-based chat surface

## Recommended frontend path

For browser clients, treat the gateway as the source of truth and let the browser connect directly to the browser-oriented UI session surface exposed through the gateway/Envoy edge.

That keeps your app closer to Talon’s actual runtime model and avoids inventing a separate “proxy backend” just for chat streaming.

## What to read next

- [Build a ChatGPT-Style App](./chatgpt-app.md)
- [Build a Marketing Agency](./marketing-agency.md)
- [Runtime Surfaces](../reference/runtime-surfaces.md)
- [Gateway API](../reference/generated/gateway-service.md)
- [Sessions and execution](../concepts/sessions-and-streaming.md)
- [Config schema](../reference/generated/config-schema.md)
