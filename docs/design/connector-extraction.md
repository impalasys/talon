# Connector extraction: public connector runtime design

## Status

**Proposed.** This document defines a public-safe path for adding messaging
connector runtimes to Talon. It is intentionally a new design and new
implementation; it is not a request to move code from a private service.

## Problem

Talon already owns the control-plane half of messaging integration:
`ConnectorClass` and `Connector` resources, route resolution, durable inbound
event handling, and the public connector protocol in
[`proto/external/connectors.proto`](../../proto/external/connectors.proto).
Provider adapters still need a portable runtime boundary. That boundary must
let a provider turn an authenticated provider event into a normalized Talon
event, accept Talon deliveries and activity requests, and keep provider
credentials and account lifecycle outside Talon core.

The initial target is a hostable connector SDK and a non-operational iMessage
reference skeleton. It must not include an iMessage transport implementation,
provider credentials, or managed account setup.

## Current public surface

The existing Talon protocol is the compatibility boundary, not a new protocol
to duplicate:

- `RegisterClusterRequest` / `RegisterClusterResponse` register a Talon
  `ConnectorClass` with a connector runtime.
- `ConnectorMessageEvent` carries a normalized inbound event to Talon; Talon
  deduplicates it by `event_id` under the connector-class registration and
  routes it using opaque `match_fields`.
- `ConnectorDeliveryRequest` and `ConnectorActivityRequest` let Talon request
  outbound delivery and provider-visible activity.
- `ConnectorStatusEvent` currently accepts a connector's health report for
  acknowledgement and logs it. It does not yet persist or reconcile provider
  health into `ConnectorClass` status.

`ConnectorClass` declares the platform, runtime endpoint, auth source, and
match indexes. `Connector` supplies a concrete route and Talon consumer. The
connector runtime owns provider protocol behavior; Talon owns routing,
dispatch, and durable agent state.

## Goals

- Define provider-neutral contracts that can be hosted independently of Talon
  core.
- Keep the SDK independent of HTTP frameworks, databases, cloud identity,
  and provider SDKs.
- Provide a safe testing path using in-memory ports and fixture-free reference
  implementations.
- Make provider adapters independently deployable and versionable.
- Preserve at-least-once semantics: an adapter must make inbound and outbound
  operations idempotent with stable IDs. A multi-message delivery may be
  retried from the beginning unless a future adapter documents stronger
  guarantees.

## Non-goals

- An operational iMessage, Slack, Discord, or WhatsApp adapter in the first
  phases.
- Provider OAuth/account-installation UI, tenancy management, or operator
  console flows.
- A hosted connector service, default database, object store, or secret
  manager.
- Replacing Talon's existing `ConnectorClass`, `Connector`, or external
  protocol schemas.
- Committing credentials, webhook examples from real accounts, internal
  network topology, or provider account/profile identifiers.

## Proposed layout

The first implementation lives with the public Rust SDKs, rather than inside
Talon's control-plane modules:

```text
sdk/rust/talon-connector/                 # Phase 1: provider-neutral SDK
  src/lib.rs                              # Connector and port traits
  src/model.rs                            # normalized, non-secret data types
  src/runtime.rs                          # host orchestration and retry policy
  src/ports.rs                            # injected persistence/secret/attachment ports
  src/test_support.rs                     # in-memory fakes, test-only

sdk/rust/talon-connector-talon/           # Phase 2: Talon protocol adapter
  src/lib.rs                              # maps SDK data to talon-client protocol types

sdk/rust/talon-connector-imessage/        # Phase 3 reference skeleton
  README.md                               # capability and prerequisites only at first
  src/lib.rs                              # typed mapping/skeleton; no private runtime

connectors/slack/                         # later, independently deployable adapter
connectors/discord/
connectors/whatsapp/
```

The layout is intentionally split between a generic SDK, a Talon-specific
adapter, and providers. The generic crate may depend on `async-trait` and
public data types, but not on Talon server internals. The Talon adapter may
depend on the generated public client/protocol. Provider crates are optional
and may use their own public provider SDKs after separate license, terms, and
security review.

## Connector contract

The SDK uses injected ports so a host, not a provider adapter, chooses auth,
storage, and transport:

```rust
#[async_trait::async_trait]
pub trait Connector: Send + Sync {
    fn descriptor(&self) -> ConnectorDescriptor;

    async fn verify(
        &self,
        request: RawInboundRequest,
        secrets: &dyn InboundSecretResolver,
        clock: &dyn Clock,
    ) -> Result<VerifiedInboundRequest, ConnectorError>;

    async fn ingest(
        &self,
        request: VerifiedInboundRequest,
        context: &ConnectorContext,
    ) -> Result<InboundEvent, ConnectorError>;

    async fn deliver(
        &self,
        request: DeliveryRequest,
        context: &ConnectorContext,
    ) -> Result<DeliveryReceipt, ConnectorError>;

    async fn activity(
        &self,
        request: ActivityRequest,
        context: &ConnectorContext,
    ) -> Result<ActivityReceipt, ConnectorError>;
}
```

`RawInboundRequest` is an opaque transport wrapper. Each provider adapter owns
the `verify` implementation, including provider signature checks and replay
timestamp validation, and can only obtain its verification material through
the narrow `InboundSecretResolver` port. It returns a
`VerifiedInboundRequest` only on success. The generic host never interprets a
provider signature. It next reserves the request with `ReplayGuard`, calls
`ingest` to normalize it, and is the sole owner of submitting that event to
`EventSink`, acknowledging the provider, and translating retry outcomes. A
connector implementation must never call `EventSink` itself.

`InboundEvent` has a stable event ID, message ID, opaque match fields,
conversation/thread IDs, sender projection, text, timestamp, labels, and
attachment references.
`DeliveryRequest` has an idempotency key, destination, optional reply target,
text, attachments, and labels. `ConnectorError` is structured and safe to log:
it categorizes failures without rendering secrets or provider payloads.

`ConnectorContext` exposes these ports:

- `BindingResolver`: resolves provider identity and match fields to an opaque
  binding.
- `SecretResolver`: retrieves opaque provider configuration only when needed.
- `EventSink`: owned by the generic host; submits normalized inbound events.
- `InboundSecretResolver`: supplies only the verification material named by a
  provider adapter during `verify`.
- `AttachmentStore`: stores attachment streams and returns an opaque reference.
- `DeliveryStore`: reserves idempotency keys and records outcomes.
- `Clock` and `ReplayGuard`: enable deterministic tests and replay-window
  validation.

Provider adapters must not retain credentials in connector structs or emit
them in `Debug`, errors, metrics, or tracing fields. A provider's `verify`
implementation uses a constant-time comparison where the provider scheme
permits it; the generic host enforces duplicate reservations through
`ReplayGuard` before calling `ingest`. Invalid, malformed, stale, or duplicate
requests must not invoke `SecretResolver` or `EventSink`; only a valid
`verify` call may access the narrowly scoped `InboundSecretResolver`.

## iMessage reference skeleton

The initial iMessage artifact only declares capabilities and validates
non-secret configuration. It can map public, synthetic input to the neutral
data model in unit tests, but does not read local message databases, control a
desktop client, contact an external iMessage bridge, or make network calls.

The eventual adapter should support conversation IDs, replies, attachments,
reactions, typing, and ordered multi-bubble delivery. It should treat a
multi-bubble send as explicitly at-least-once and report a per-delivery
idempotency key. A live runtime requires a separate proposal covering
supported deployment environments, provider/third-party terms, permissions,
and data retention.

## Secrets and configuration

The proposed SDK configuration model contains references and non-sensitive
routing metadata only. Runtime provider secret values come from a
host-provided `SecretResolver`; they are never serialized into SDK
configuration, persistent delivery records, generated examples, or logs.
Talon's existing `ConnectorSecretRef` still supports an inline `plain` value
for the Talon-to-runtime API key, but public manifests and connector examples
must use `ConnectorClass.auth.api_key.env` and must never use `plain`.
Provider credentials remain entirely within the connector host.

Examples must use placeholder environment variable names and reserved example
domains. Tests must use synthetic IDs, payloads, and attachments. A connector
may expose a health state such as `connected`, `degraded`, `disabled`, or
`revoked` to its host, but Talon currently acknowledges rather than persists
`ConnectorStatusEvent`; a future Talon status-reconciliation change is needed
before those states are operator-visible in Talon. Diagnostics must use stable
error codes rather than raw provider response bodies.

## What must not go public

The following is expressly excluded from this repository and these PRs:

- private source, database schemas, migrations, generated bindings, tests,
  cryptography helpers, deployment manifests, sidecars, service identities,
  domains, endpoints, and configuration from the private system;
- production/development credentials, API keys, webhook secrets, callback
  tokens, encryption keys, signed URLs, real account/profile IDs, real
  message payloads, or attachment content;
- private authorization/tenant policy, account lifecycle behavior, audit
  records, internal routing metadata, logging conventions, and support tools;
- any third-party iMessage bridge/SDK implementation or derived code until its
  license, distribution terms, API stability, and platform compliance have
  been reviewed; and
- insecure development fallbacks or legacy plaintext compatibility behavior.

If provenance or public suitability is uncertain, create a new interface and
synthetic test instead of porting code.

## Licensing and contribution policy

All new source is authored for Talon under the repository's
`AGPL-3.0-only` policy and must carry its standard file header. Contributors
must follow [`CONTRIBUTING.md`](../../CONTRIBUTING.md), including the
[`CLA.md`](../../CLA.md) process. Before adding a provider SDK, fixture, or
protocol-derived implementation, record its license/terms review in that
adapter's PR. Private implementation is reference-only until ownership and
relicensing authority are confirmed; no private code, tests, or generated
artifacts may be copied.

## Phased migration plan

1. **Design:** publish this boundary, threat model, and explicit exclusions.
2. **Core SDK prototype:** add the dependency-light contract, redaction-safe
   errors, and in-memory fake ports. Include a no-network reference connector.
   Its tests must cover malformed/invalid signatures, stale and duplicate
   replay attempts. Provider `verify` tests must prove that rejected input does
   not reach the host `EventSink`, while host tests must prove it never calls
   `ingest` or `EventSink` for rejected or duplicate input. Test `Debug`,
   `Display`, errors, and tracing fields against synthetic secret/payload
   canaries. Add deterministic `Clock` and `DeliveryStore` tests for concurrent duplicates,
   crash-after-reservation, retry-after-transient-failure, and receipt
   recovery: a persisted receipt permits no second provider send for the same
   `delivery_id`.
3. **Talon protocol adapter:** map core event, delivery, and activity types to
   the existing public connector protocol. Test a table-driven contract suite
   against generated public types, including registration/class consistency,
   optional thread/reply fields, attachments and labels, CREATED-only inbound
   support, and accepted/duplicate/unmatched/rejected outcomes. The adapter
   must not retry a duplicate; unmatched handling must be explicitly selected
   by the host. Do not claim Talon-visible connector health until a separate
   Talon status-persistence phase is approved.
4. **iMessage skeleton:** add the reference crate to the validated public Rust
   SDK workspace, with no operational capabilities and only typed synthetic
   payload mapping, pending a separate runtime, licensing, and compliance
   design. Enforce a dependency allowlist that excludes HTTP/socket clients and
   macOS message-database bindings; tests must prove validation and mapping use
   injected synthetic input only.
5. **Provider adapters:** add Slack, Discord, and WhatsApp independently,
   based on public provider documentation and synthetic fixtures.
6. **Durable host (optional):** select a standalone persistence and deployment
   story after the ports stabilize. It must not make any private database,
   identity system, secret store, or cloud topology the default.

Each phase remains independently reviewable. The runtime must be deployed as
an untrusted integration boundary: scoped outbound credentials, least
privilege callback authentication, redacted logs, rotation support, and
explicit operator ownership are required before production use.
