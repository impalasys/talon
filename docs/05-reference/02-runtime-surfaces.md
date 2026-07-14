---
title: Runtime Surfaces
sidebar_position: 2
---

Talon exposes one public gateway surface over the runtime system.

## Native gRPC and gRPC-Web

The canonical contract is the versioned `talon.v1` gRPC API, served as both native gRPC and gRPC-Web on the gateway port.

Use it when you want:

- typed service integration
- the full system-of-record contract
- native and browser streaming

## Which one should you choose

- backend integration: use native gRPC clients
- browser integration: use gRPC-Web clients
- SDK integration: use a Talon clientset and access the named services from it

## Read next

- [Events and Models](./03-events-and-models.md)
