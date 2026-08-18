---
title: Connectors and Conversations
sidebar:
  order: 2
---

> **Maturity:** Connector resources and their Talon-side routing contract are implemented. Provider onboarding, account management, and provider-specific protocol proposals remain design work and live in Contributors.

## In brief

Connectors translate external conversation events into Talon work and translate approved Talon output back to an external system.

## Ownership

A ConnectorClass describes a trusted adapter and its match indexes. A Connector maps provider-neutral match fields to one consumer: a Session, Channel, or Workflow. The connector service owns provider credentials and provider-specific protocol behavior; Talon owns routing, durable state, and dispatch.

## Lifecycle

An adapter normalizes an inbound provider event. Talon validates and deduplicates it, resolves a matching Connector, and dispatches its configured consumer. Outbound work is handed to the adapter as a delivery request, while activity and status updates remain observable in Talon.

## Relationships

Connectors are ingress and egress adapters, not Channels or Sessions themselves. Provider-specific setup is deliberately outside this page; exact message shapes belong in the generated [Connector Reference](../../reference/generated/external-connector-schemas).

## Read next

- [Channels and Subscriptions](../coordination-model/channels-and-subscriptions)
- [Gateway and Clients](./gateway-and-clients)
