---
title: Gateway and Clients
sidebar:
  order: 1
---

## In brief

The gateway is Talon's public control-plane boundary. It exposes the versioned `talon.v1` contract to clients and coordinates access to durable state and live events.

## Ownership

Native gRPC is the canonical service contract. gRPC-Web adapts the same services for browser clients, and SDK clientsets expose named services over those transports. The gateway owns public validation, authorization, persistence, and read/stream surfaces; workers do not become public client endpoints.

## Lifecycle

Clients create or read resources, submit work, and subscribe to streams through the gateway. The gateway returns durable state for later reads and live session, channel, or workflow events while work is in progress.

## Relationships

This page explains the boundary, not a client tutorial. See [Build a Client](../../build/build-a-client) for integration choices and [Reference](../../reference) for exact service methods and models.

## Read next

- [Execution and Recovery](../execution-model/execution-and-recovery)
- [Identity, Access, and Secrets](./identity-access-and-secrets)
