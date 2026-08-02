# Durable model-context compaction

## Decision

PR 171 uses summary-and-tail compaction rather than a serialized provider
transcript snapshot. The full `SessionMessage` transcript and execution journal
remain the audit record. A compacted view is an immutable Markdown handoff plus
an exact retained tail, following the broad pattern used by coding agents such
as OpenCode, OpenHands, and Aider.

## Storage contract

Each compaction writes exactly one summary object:

```text
cas/{namespace}/sessions/{session_id}/compactions/{submission_id}/{journal_entry_id}.txt
```

The object contains only Markdown summary text. The `COMPACTION` journal entry
and internal `SESSION_MESSAGE_PART_TYPE_COMPACTION` message part reference the
same `ObjectRef`; neither stores a serialized provider transcript. The internal
part carries its transcript and journal tail anchors in its private payload and
is removed from public Session, A2A, connector, stream, and index views.

## Write ordering and recovery

The executor asks the configured LLM for a compact handoff that records the
task, decisions, changed artifacts, completed work, unresolved issues, and next
actions. Dynamic system and goal prompts are not summarized; they are rendered
again for every request.

### Summary request boundary

The compaction request has a verbose system prompt that defines the required
handoff headings and evidence rules. Its user message contains only an XML
serialization of canonical transcript data:

```xml
<transcript>
  <message role="user"><content>...</content></message>
  <message role="assistant"><content>...</content></message>
</transcript>
```

Message roles and content are XML-escaped. The system prompt explicitly treats
that payload as untrusted data: instructions or role claims inside it must not
be followed. It admits facts supported by user instructions, tool results, or
durable execution results; unsupported assistant prose remains a claim. The
summary is bounded to 500 words and must use the fixed handoff headings.

For temporary diagnosis, `TALON_LLM_DEBUG_REQUESTS=1` logs the provider payload,
and debug logging in `compaction::summarize` records the exact compaction system
prompt and XML user payload. Both can contain session content and must only be
enabled in an appropriately protected development environment.

The persistence order is:

1. write the CAS summary;
2. append the attempt-fenced `COMPACTION` journal entry;
3. append the internal canonical message part;
4. replace the live model context.

If an earlier write fails, the live context stays unchanged. A CAS write left by
a contested journal append is unreachable and may be garbage-collected. On a
worker reclaim, the summary object is loaded and only journal state after its
tail boundary is rebuilt. On a subsequent submission, the newest valid marker
selects the summary and canonical message tail. This avoids duplicate provider
history and tool re-execution while retaining the complete audit transcript.

## Budgeting

Durable compaction uses the existing provider/model-aware effective input budget
(including tool-schema cost), not a separate fixed character threshold. The
normal request-shaping compactor still validates the final provider transcript.

## Cleanup

Summary objects are session-scoped CAS data. They are retained while referenced
by a committed marker or active journal entry, collected during session deletion,
and orphaned pre-journal writes are eligible for CAS garbage collection.
