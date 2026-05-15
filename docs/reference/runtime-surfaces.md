---
title: Runtime Surfaces
sidebar_position: 2
---

Talon exposes multiple API surfaces over the same runtime system.

## Native gRPC

The canonical contract is the `GatewayService` gRPC API.

Use it when you want:

- typed service integration
- the full system-of-record contract
- native streaming with `StreamSessionSteps`

## REST-transcoded HTTP

Envoy exposes REST mappings for the gateway’s control-plane operations.

Use it when you want:

- simpler HTTP access
- easier curl or service-to-service integration
- CRUD over namespaces, agents, schedules, templates, and related resources

## Browser-oriented UI session surface

The gateway also exposes a UI HTTP surface for session interactions used by Sightline-style clients.

Use it when you want:

- browser chat/session interactions
- live streamed UI responses
- tool visibility in a browser-native flow

## Which one should you choose

- backend integration: prefer gRPC
- operational HTTP integration: use REST-transcoded routes
- browser frontend integration: use the UI session surface

## Read next

- [Gateway API](./generated/gateway-service.md)
- [Events and Models](./events-and-models.md)
