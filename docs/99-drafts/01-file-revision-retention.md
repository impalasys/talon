---
title: File Revision Retention
---

# File Revision Retention

## Problem

File writes already create immutable CAS keys from the File UID and content
hash. The File resource status points to the latest key. Today, updating or
deleting a File removes older object keys immediately. A persisted session tool
result can therefore reference a revision that no longer exists.

The accompanying containment change makes a missing historical result visible
and replayable, but it intentionally does not make that result recoverable.

## Constraints

- Session replay must never substitute the current File for the version the
  agent originally read.
- The solution must not copy File bytes into every session.
- File update and delete must remain safe while a tool call is in flight and
  while its result is being journaled.
- Retention must be bounded and session deletion must eventually release
  otherwise unreachable revisions.
- `FileRetention.RETAINED` currently means retain the live File until it is
  updated or deleted. It must not silently acquire version-history semantics.

## Options evaluated

| Option | Advantages | Costs and unresolved risks |
| --- | --- | --- |
| Retain every superseded revision indefinitely | No extra write on session replay; existing object keys already identify revisions. | Unbounded storage; deleting a File does not reclaim its history. |
| Revision reachability index | GC can remove a revision once no durable session or journal refers to it; no byte duplication. | Requires transactional or carefully ordered claims during File reads, session persistence, session deletion, and File deletion. |
| Periodic reachability scan plus a conservative grace period | Avoids per-read index writes and can reuse durable session/journal data as the source of truth. | Scan cost, an in-flight read-to-journal race, and the grace-period policy need explicit design. |
| Time-bounded revision retention | Simple operational bound. | Old sessions can lose exact replay; this is an explicit compatibility/data-retention policy, not a transparent fix. |

## Recommended next design step

Prototype immutable File revision retention with a reachability collector before
adding a public versioning API. A File update should create the new revision and
advance the File status without immediately deleting the predecessor. The
collector should only reclaim non-current revisions that are absent from durable
session messages and submission journals, after a defined grace period that
covers in-flight tool-result persistence.

Before implementation, decide and document:

1. the grace period and maximum retained storage;
2. whether File deletion retains reachable historical revisions;
3. how a collector proves reachability across session messages and journals;
4. how session deletion releases revisions; and
5. whether an API for reading a historical File revision is required.

No File update or delete behavior changes are made by this draft.
