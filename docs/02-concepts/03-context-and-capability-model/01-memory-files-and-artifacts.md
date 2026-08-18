---
title: Memory, Files, and Artifacts
sidebar:
  order: 1
---

## In brief

Session history is interaction context; File resources are durable content; artifacts are execution-produced objects with explicit ownership and access.

## Ownership

Files are namespaced, content-addressed resources with a declared purpose and optional indexing behavior. Memory-purpose Files provide durable agent or namespace context. Artifacts are objects created during execution and can be promoted or granted for later use. Legacy Knowledge remains available for existing content, but File memory is the forward model for durable context.

## Lifecycle

Content can be uploaded or created, indexed where configured, attached to a namespace or runtime flow, and retained under resource policy. A session can create artifacts; promoted or granted artifacts become available outside that immediate execution without turning every object into public namespace memory.

## Relationships

Prompt and message history are not a substitute for durable memory. [Skills](./skills) package reusable instructions and supporting files, while the [Reference](../../reference/generated/resource-schemas) defines exact File and artifact shapes.

## Read next

- [Skills](./skills)
- [Sessions and Submissions](../execution-model/sessions-and-submissions)
