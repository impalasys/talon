# Durable model-context compaction

Status: implemented in PR 171 (`feat/durable-model-context-compaction`)

## Executive summary

Long-running sessions can accumulate enough assistant, user, tool-call, and
tool-result history to exceed the effective input window of the selected model.
Before this change, the runtime had no durable boundary for reducing that
history. A worker reclaim or process restart could therefore recover the full
old journal and lose any in-memory reduction made during execution.

The implemented design adds a `COMPACTION` session-journal phase. The executor
builds a smaller replayable history, asks the execution sink to persist that
history, and only then replaces its in-memory context. Recovery treats the
compaction entry as a replacement boundary, restoring the replay history and
discarding derived projection state. Tool-call metadata is preserved so the
recovered transcript remains a valid provider conversation.

This is a sound durability design and is materially safer than an in-memory
only or pointer-only approach. It is not the final optimal design: the next
improvement should unify the durable trigger with the existing model-limit-aware
request estimator and eventually use provider/model tokenization instead of a
character heuristic.

## What happened

The original PR introduced compaction logic, but review identified several gaps:

1. Compaction changed `ExecutionContext.history` without appending a journal
   entry, so recovery could reconstruct the pre-compaction transcript.
2. Recovery rebuilt compacted messages with `LoopMessage::text`, dropping
   `tool_calls` and `tool_call_id`.
3. The recovery loop failed to advance its cursor on some successful or
   unexpected-entry paths, allowing an infinite loop.
4. The trigger estimated text only, excluding tool-call arguments, media, and
   tool schemas.
5. Repeated compaction could make no progress when the retained tail was still
   over the threshold.
6. The new protobuf messages needed the same serde defaults as sibling journal
   payloads.
7. The hydration test checked journal persistence but did not execute the
   hydration path.

The branch was subsequently rebased onto the newer `main`. That base already
contained model-limit-aware, ephemeral request compaction and newer tool-output
handling. The durable boundary was integrated on top of those changes rather
than replacing them. SDK clients were regenerated from the rebased proto
definitions so the new compaction types coexist with the newer base fields.

## Design goals and invariants

The design maintains these invariants:

- A compaction is not considered successful until its journal entry is durable.
- If the journal write fails, the old in-memory history remains intact and the
  execution fails rather than continuing with unrecoverable state.
- The journal entry contains enough information to reconstruct provider-visible
  history without replaying the LLM request.
- Assistant tool calls and tool-result IDs remain linked after recovery.
- Recovery applies a compaction entry as a history replacement, not as another
  ordinary conversation turn.
- The recovery cursor advances on every handled path.
- A compaction attempt either reduces the estimated history size or disables
  retries for the current execution.
- Leading system messages are retained across durable compaction.

## Implemented architecture

### Data contract

`proto/data/session_journal_entry.proto` adds:

- `SESSION_EXECUTION_PHASE_COMPACTION`
- `SessionJournalEntryPayloadCompaction`
- `CompactMessage`
- `CompactToolCall`

The payload stores the replay history, the journal entry through which the
history was compacted, and before/after size estimates. The checked-in Go,
Java, JavaScript, Python, and Rust SDK bindings are generated from this schema.

### Execution path

Before preparing the next provider request, the executor:

1. Collects the registered tools.
2. Estimates history, prompts, tool calls/IDs, content parts, and tool-schema
   weight.
3. If the estimate exceeds the durable threshold and enough history exists,
   selects an assistant boundary, preserving tool-only assistant messages.
4. Retains leading system messages, summarizes the older segment, and keeps the
   recent replayable tail.
5. Verifies that the serialized estimate decreased.
6. Calls `ExecutionSink::on_compaction` with the replacement history.
7. Replaces `context.history` only after the sink succeeds.
8. Lets the existing model-limit-aware request compactor perform its final
   request shaping.

The worker sink implements `on_compaction` by appending the journal entry with
the current submission attempt and latest journal boundary. The journal layer
uses the common append path, retaining attempt fencing, retry behavior, ID
allocation, and submission-pointer updates.

### Recovery path

During claim recovery, a `COMPACTION` entry:

- Reconstructs `LoopMessage` content, tool calls, and tool-call IDs.
- Clears the existing runtime history.
- Clears projection parts and resets recovered-part numbering.
- Installs the compacted replay history.
- Advances past the compaction entry.

Subsequent `LLM_RESPONSE` and `TOOL_RESULT` entries are then processed normally.
An LLM response without tool calls advances immediately; a response with tool
calls advances before scanning its tool-result entries; unexpected phases also
advance before continuing.

### State transition

```text
history + prompts + tools
          |
          v
  durable threshold exceeded?
       | no                    | yes
       v                       v
  existing request      build replayable replacement
  compaction                 |
                             v
                   append COMPACTION journal entry
                       | success       | failure
                       v               v
              replace in-memory      keep old history;
              history                 fail execution
```

## Why this approach is good

The explicit compaction journal entry is a strong choice for a durable worker
system:

- Recovery is constant-step with respect to the old transcript: it can install
  the replacement snapshot instead of replaying every old event.
- The write-ahead ordering gives a clear crash boundary. A crash before the
  append sees the old history; a crash after the append can recover the new
  history.
- The payload is self-contained and versionable through protobuf.
- The same journal preserves the existing attempt fencing and idempotent
  submission machinery.
- Keeping tool metadata avoids a subtle but serious class of invalid provider
  transcripts.

This is preferable to mutating only memory, and safer than storing only a
compaction pointer that requires replaying and reinterpreting all earlier
entries during recovery.

## What is not optimal yet

### 1. Two compaction policies exist

The durable executor trigger uses a character-weight estimate, while
`messages_for_llm` also applies model-limit-aware ephemeral compaction. The
policies protect different boundaries, but they can duplicate work and may
disagree about when a request is safe.

Preferred follow-up: expose one request-budget calculation that returns both
the effective model budget and structured metrics, then use it for the durable
trigger and final request preparation. Durable compaction should trigger when
the canonical request representation cannot fit after the normal ephemeral
policy, not from an independent fixed threshold alone.

### 2. Character weight is only an approximation

Serialized character weight is deterministic and provider-independent, which is
useful for recovery decisions, but it is not token count. Unicode, JSON syntax,
multimodal parts, and provider-specific wrappers can vary significantly.

Preferred follow-up: add a tokenizer interface keyed by provider/model, with a
conservative fallback to the current character estimator. Keep the durable
payload’s before/after estimates as diagnostics, not as correctness claims.

### 3. The sink is doing persistence and event fan-out

`ExecutionSink` is a practical integration point because it already owns the
submission attempt and journal boundary. Conceptually, however, compaction is a
durable execution-boundary operation rather than a streaming event.

Preferred follow-up: split a small `DurableExecutionBoundary` interface from
the streaming sink, or inject a journal writer into the executor. This would
make the durability dependency explicit and make failure-injection tests easier
without coupling compaction to presentation events.

### 4. Snapshot size needs an operational limit

The current compaction payload stores the replayable replacement history inline
in the journal entry. Very large media or tool payloads should be represented by
object references, with bounded inline text and explicit hydration limits.

Preferred follow-up: enforce a maximum serialized compaction payload and store
large content in the existing object store, keeping references in
`CompactMessage`.

## Alternatives considered

| Approach | Benefit | Cost / reason not selected |
| --- | --- | --- |
| In-memory compaction only | Minimal code and no journal growth | Recovery loses the boundary; unsafe after reclaim or restart |
| Replay all old journal entries plus a pointer | Small compaction record | Recovery remains expensive and depends on historical replay semantics |
| Tokenizer-only trigger | Closest to provider limits | Provider/model tokenizer availability and reproducibility are not uniform |
| Two-tier ephemeral then durable compaction | Fewer durable writes | More reconciliation paths and greater live/recovery divergence risk |
| Explicit durable replacement snapshot | Fast recovery and clear crash ordering | Adds schema, journal bytes, and generated SDK maintenance |

The selected snapshot approach is the best current tradeoff for correctness and
operational recovery. The recommended end state is the same snapshot protocol,
with a shared model-aware budget calculator and bounded/object-backed payloads.

## Verification

The implementation was verified with:

- Focused recovery hydration test, including tool-call metadata and continuation.
- Focused executor compaction continuity test.
- Full `cargo test --locked` before the rebase.
- Rebased `cargo check --locked` after integrating current `main`.
- Pre-push validation on the final rebased push: metadata, formatting, binary
  build, full Rust tests, binary tests, integration test, and doc tests.

## Review disposition

All five previously unresolved review threads were addressed and resolved after
the rebase. The remaining top-level comments are informational automation
artifacts: generated summaries, a discontinued Gemini notice, CodeRabbit
processing/status text, and a Sightline screenshot artifact. They do not
request code or review-state action.
