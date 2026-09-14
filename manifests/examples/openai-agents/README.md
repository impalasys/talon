# OpenAI Agents API

Talon routes messages to a persistent OpenAI session and records its final
answer. OpenAI runs the agent, owns conversation context, and decides when
incoming messages start or steer a turn. Talon does not run a local model loop.

## Configuration

Use `agent.yaml` with an existing tenant namespace in place of
`Tenant:example:main`. Configure its `openai` provider in the worker config:

```yaml
providers:
  openai:
    type: openai
    model: gpt-5.6-luna
```

The agent's selected model profile supplies the actual model. For a tenant
namespace, put the OpenAI API key in the existing `api-keys` Secret at the
tenant root (`Tenant:example` in this example). `spec.data.openai` contains the
base64-encoded key. Tenant credentials never fall back to a worker-wide key.
For namespaces outside a tenant, the provider's existing `apiKey` secret
configuration is used. Do not commit credentials to the example.

The key needs access to the OpenAI Agents API and the selected model. Requests
use `https://api.openai.com/v1` and `OpenAI-Beta: agents=v1`.
`OPENAI_BASE_URL` follows the existing OpenAI provider endpoint override.

## Supported behavior

- Plain user text, `systemPrompt`, and optional `thinking.effort` run with
  OpenAI `environment.type: none`. `thinking.enabled: false` without an effort
  requests `reasoning.effort: none`.
- Sending or enqueueing while busy forwards input without waiting for the
  current generation. OpenAI may acknowledge input before it appears in saved
  history. Multiple inputs can belong to one remote turn.
- Only the root turn's last completed final answer becomes a Talon reply.
  Commentary and answers superseded by steering are not sent separately.
  Coalesced inputs share the same persisted reply and existing connector flow.
- Talon saves a stop request before acknowledging it and forwards cancellation
  once OpenAI has an active turn. A restarted observer also reads that request.
  Completion, failure, cancellation, and a disconnected observer remain distinct.
  Queued inputs discarded by OpenAI on cancellation are accounted for so the
  next message can continue the same conversation.
- Tools, MCP, features, capabilities, A2A, attachments, `postHistoryPrompt`,
  ACP settings, nonzero temperature, and thinking token budgets are rejected.
  The protobuf default temperature value `0` is not sent to OpenAI. Native
  compaction is unsupported because OpenAI owns the context.
- A changed model, provider, endpoint, or instructions requires a new or
  cleared Talon session. Clearing removes the local association; it does not
  delete the old remote session from OpenAI.

## Recovery

Session mappings, accepted input order, remote turn IDs, and reply IDs live
under the tenant/agent/session in Talon's existing key-value store. Replaying
the **same Talon submission** retrieves saved work rather than sending another
message. An accepted HTTP response releases the input reservation immediately;
waiting for remote output does not reserve the conversation.

Before a request, Talon records an uncertain-submission marker. If its response
is lost, recovery inspects saved inputs. Session creation also carries a unique
metadata marker so a crash before saving its ID can be reconciled. Events are
not assumed to replay; reconnecting subscribes before reading saved state.

If an uncertain request cannot be uniquely reconciled within the recovery
interval, the worker returns an actionable recovery error and keeps the
submission recoverable. It does not create another turn. A new message ID is
new work, so do not replace a retry with a freshly generated submission.
Use `SessionService.SubmitTurn` with the same `message.id` and content for client retries.
This also reattaches an abandoned observer immediately. If no live worker owns
unfinished work, stop returns an actionable unavailable error; reattach the
same submission before stopping it. It does not report a cancellation that
was never forwarded.

Clear refuses outstanding input even after the native worker timeout. A short
local write reservation protects each input while its message and submission
are saved; a concurrent retry cannot take over that reservation. If the gateway
dies during this local write, the reservation stays unresolved and Clear stays
blocked. Inspect the session and use a new session if the original writer cannot
finish. Talon does not expire the reservation and risk a delayed writer restoring
input after Clear. This limitation precedes OpenAI execution; the worker crash
recovery described above applies to fully admitted submissions.
An interrupted OpenAI Clear also stays locked until its writer finishes; inspect
it and use a new session if that writer cannot resume. Native and ACP keep their
existing timeout behavior.

Remote sessions created by this runtime must receive their inputs through
Talon: external edits to their input history invalidate attribution.

Worker redelivery requires the existing broker or caller to redrive the
submission. The development `local_socket` broker is non-durable and cannot
itself restore an event lost on restart. This integration adds no scheduler.
Connector delivery retains the existing connector's delivery-ID and retry
semantics; a persisted answer alone is not proof of external delivery.

## Tests

Ordinary validation and the existing runtime/connector tests run without
OpenAI credentials:

```bash
cargo test --locked
```

The explicit live tests bill `OPENAI_API_KEY` using `gpt-5.6-luna`:

```bash
cargo test --locked --lib harness::openai_agents::tests::live_luna \
  -- --ignored --nocapture --test-threads=1
```

They use the actual Talon gateway and worker with temporary SQLite storage,
real OpenAI sessions, and worker cancellation over a local socket. Coverage includes
continuity, repeated steering, cancellation, rejection, tenant isolation,
clear, dropped streams, and aborted worker execution after acceptance, remote
completion, local commit, and Stop acknowledgment before remote cancellation.
Recovery creates a fresh worker/database connection and redelivers the original
event. The tests delete their remote
sessions and do not use deployed agents or external messaging connectors.

Protocol references: [sessions](https://developers.openai.com/api/docs/guides/agents-api/sessions)
and [events and recovery](https://developers.openai.com/api/docs/guides/agents-api/sessions/events).
