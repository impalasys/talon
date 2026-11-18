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

### File tool authorization and deletion

The `files` capability grants actions (`read`/`inspect`, `create`, `update`,
`delete`). Listing selects `namespace` (default: current). Tools that accept
`uri` resolve its namespace when supplied, otherwise use `namespace` (default:
current). Reads also inherit access to ancestor namespaces. Mutations are scoped to the current namespace. Add exact namespace
names in `capabilities.fileNamespaces` to permit additional namespaces for an
already-granted action; wildcard strings do not grant wildcard access.

`delete_file` covers all File purposes, including MEMORY. A session is required.
The first call returns `status: confirmation_required`, `confirmationId`, and
`requiredReply` without deleting anything. Ask the user to send that exact reply
in a later message, then repeat the tool with `confirmation_id`. A confirmation
expires after one hour, is single-use, and is bound to the requesting session,
agent, namespace, path, and resource version. Any intervening File update requires
a fresh confirmation. Confirmation ordering uses a server-owned, versioned
admission state, recorded when input is accepted (including queued replies),
not client timestamps or IDs. Pending inputs and edits fence consumption;
editing an old message does not make it a later confirmation. If input changes
during confirmation, request a new delete token. Exact namespace grants also
apply to File mounts in `run_python_code`. Deletion removes the resource metadata conditionally;
it does not eagerly remove shared blobs or a potentially replaced latest alias.

Files with finalizers are rejected before requesting confirmation. Complete their
external cleanup first; native deletion does not remove finalizers or create a
pending-finalizer deletion that depends on an unreliable notification.
