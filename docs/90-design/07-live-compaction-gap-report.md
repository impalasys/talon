# Live compaction gap report

## Evidence

This report concerns session `019fbfba-9b78-73d3-866f-17a2d5d8b303` in
`novita-retry/compaction-smoke`.

- Configured and resolved limits: 8,192 context tokens and 4,096 maximum output
  tokens.
- Four Parallel Search results were stored as zstd-compressed typed text CAS
  objects. Their stored `ObjectRef.size_bytes` values were 6,845, 9,539, 9,282,
  and 11,692 bytes. Their decoded text sizes were 57,890, 56,580, 57,106, and
  56,428 bytes.
- Durable compaction was invoked before the second ordinary model call.
- It did not commit a summary, journal entry, or session-message marker.
- The following ordinary model request reported 60,047 input tokens.

## Gap 1: durable budgeting does not use the representation sent to the model

`serialized_message_weight` uses `ObjectRef.size_bytes` for text object parts.
For a zstd object this is the compressed CAS byte length. The OpenAI-compatible
adapter decodes that object and sends the decoded text to the model. Therefore
the durable trigger and the actual provider request measure different inputs.

| Result | Budgeted object bytes | Decoded model text bytes |
| --- | ---: | ---: |
| Search 1 | 6,845 | 57,890 |
| Search 2 | 9,539 | 56,580 |
| Search 3 | 9,282 | 57,106 |
| Search 4 | 11,692 | 56,428 |
| Total | 37,358 | 228,004 |

The result is that a request can be considered within, or only marginally over,
the durable budget while the provider receives an order-of-magnitude larger
payload. The live request demonstrates this: it reached 60,047 tokens despite
the 8,192-token model limit.

The canonical logical size is already available on each object as
`metadata["uncompressed_size_bytes"]`; it is not used by
`content_part_weight`.

## Gap 2: the compaction cut point excludes the useful tool interaction

`compact()` chooses the newest assistant message as its tail boundary, then
summarizes only history before that assistant message. In this run the chosen
assistant message was the response that made the four search calls. Its four
tool results are after that boundary, and therefore remain in the exact tail.

The compacted prefix was only the original user request. The compactor did not
receive the assistant tool call or any search result. This defeats the point of
compacting the oversized research interaction: the expensive data stays in the
tail while the model spends a separate request summarizing a short prompt.

## Gap 3: a no-progress attempt calls the LLM but leaves no durable evidence

`compact()` performs these steps:

1. Select the prefix.
2. Call `summarize()`.
3. Build the replacement history.
4. Compare its estimated size with the original history.
5. Return `false` without persistence when the replacement is not smaller.

Because the selected prefix was short, the summary was larger than it. The
attempt was rejected at step 5. The request and response are not written to a
journal entry, session message, or CAS object when this happens. Debug logging
was the only observation point; the live retry did not retain that stream.

This creates three audit gaps:

- no durable record that compaction was attempted;
- no durable request payload for review;
- no durable provider output or failure reason.

It also spends a compactor LLM call that cannot improve the context.

## Gap 4: compression accounting and exact-tail policy interact badly

The exact tail is deliberately preserved. That is correct only if the retained
tail fits the model budget. Here the retained tail contains the four full search
results. The trigger underestimates them because it sees compressed byte sizes;
the cut policy then preserves them because they follow the selected assistant
boundary. The next provider request expands and sends them all.

Neither component is independently sufficient to explain the 60k request. The
failure is their interaction:

```text
compressed-size estimate
  + assistant-only cut boundary
  + exact tail containing tool results
  + provider-side text hydration
  = oversized ordinary model request after a discarded compaction call
```

## Gap 5: the retry report could not show actual compactor output

The requested review artifact requires an agent transcript and the compactor's
actual request and response. The agent transcript is durable. This failed
compactor attempt is not. The exact request can be reconstructed from the
deterministic prompt and selected prefix; the exact provider response cannot be
recovered after the process because no persistence path exists for rejected
attempts.

## Test gaps

Current tests cover text-object materialization during persisted history replay.
They do not cover this live-path combination:

1. execute a tool that returns a compressed text `ObjectRef`;
2. use the resulting object in live `ExecutionContext.history`;
3. configure a small model window;
4. verify durable budget uses logical decoded size;
5. verify the selected prefix contains the assistant tool call and its matched
   tool results when that interaction is being compacted;
6. assert the committed marker and journal entry share the summary object;
7. assert a rejected/no-progress attempt does not issue a compactor provider
   call, or persists a reviewable failed-attempt record if issuing it is
   intentional;
8. verify the next ordinary provider request is within the effective input
   budget after compaction.

## Required design decisions

These gaps require explicit decisions before changing behavior:

1. **Logical object size:** use the decoded size metadata for all text object
   budgeting, with a safe fallback when the metadata is absent or invalid.
2. **Compaction unit:** compact an assistant tool-call message together with
   its matched tool-result messages; never leave that interaction split across
   the summary/tail boundary.
3. **No-progress ordering:** estimate a candidate's usefulness before calling
   the compactor, or persist attempt telemetry sufficient to inspect rejected
   calls without making it user-visible.
4. **Tail fit:** if the exact retained tail alone exceeds the effective input
   limit, define whether execution fails explicitly, retains a smaller exact
   tail, or uses a different compaction boundary. The current implementation
   silently sends the oversized provider request.

## Scope of the report

This report identifies gaps observed in the live run. It does not select among
the required design decisions or modify compaction behavior.
