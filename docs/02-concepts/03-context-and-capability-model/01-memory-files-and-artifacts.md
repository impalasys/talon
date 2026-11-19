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
in a later message containing only Text parts, then repeat the tool with
`confirmation_id`. Attachments and other non-Text parts do not count as confirmation. A confirmation
expires after one hour, is single-use, and is bound to the requesting session,
agent, namespace, path, and resource version. Any intervening File update requires
a fresh confirmation. Each session keeps at most one active pending deletion;
a new request supersedes the previous confirmation ID, and successful consumption
atomically removes the pending record. Abandoned requests occupy at most that
single slot until replaced or checked after expiry. Confirmation ordering uses a
server-owned, versioned admission state. User payloads and their admission order
commit in one atomic storage transaction, including queued replies. Client
timestamps, IDs, and database acknowledgement timing do not set that order.
Explicit transcript edits update the payload and its confirmation digest in the
same transaction; an edit retains the original order and cannot become a later
confirmation. Consuming a confirmation atomically updates the consumed-order
watermark and removes the pending token. If input changes during confirmation,
request a new delete token.

New turns require User-role messages. `SubmitTurn` requires a nonempty, client-generated
message ID so a failed response can be retried with the same ID. Fresh turns require fresh message IDs;
exact retries resume the original submission, including after dispatch failures,
and cannot start a second turn. The payload, admission state, idempotency receipt,
and pending submission commit together. Recovery reloads the current canonical
payload after explicit transcript edits. Queue acceptance similarly commits the
payload, receipt, and admission state together. Queue receipts preallocate stable
canonical message and submission IDs. Stable client IDs map to the original queue
entry across timestamp changes; new queue keys sort by committed admission
order, after any legacy backlog. These IDs cannot be admitted again through the direct
turn path. Promotion preserves canonical edits;
reclassification and terminal cleanup atomically guard all channel copies.
Compact terminal receipts remain with the session transcript so a delayed retry
cannot recreate an already dispatched input. Session clear/delete removes this
idempotency history with the transcript.

Exact namespace grants also
apply to File mounts in `run_python_code`. Deletion removes the resource metadata conditionally;
it does not eagerly remove shared blobs or a potentially replaced latest alias.
The storage commit determines deletion success. Lifecycle notifications are
best-effort with bounded retries and a two-second deadline per publish attempt; a prolonged notification outage does not undo
a committed deletion. Read/list the File resource for authoritative existence.
Finalizer-dependent cleanup is excluded from this native operation.

Files with finalizers are rejected before requesting confirmation. Complete their
external cleanup first; native deletion does not remove finalizers or create a
pending-finalizer deletion that depends on an unreliable notification.
