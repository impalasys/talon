---
title: Connector Protocol Design
---

# Connector Protocol Design

Status: draft

This document proposes a Talon connector protocol for attaching external
messaging platforms, such as Slack and iMessage providers, to Talon agents.

The design goal is runtime message bridging. Talon must safely route external
messages into the correct tenant namespace, agent, Session, or Channel in a
multi-tenant deployment where many Talon clusters or operator accounts may use
the same connector runtime.

## Problem

Talon has durable runtime abstractions for agents, Sessions, Channels, and
namespaced resources. External messaging systems have their own abstractions:

- Slack has apps, workspace installations, bot users, channels, DMs, and
  threads.
- iMessage integrations vary by provider. BlueBubbles, Photon, and similar
  systems expose different account, webhook, stream, and attachment models.

The connector protocol must translate those systems into Talon without making
Talon own every provider-specific credential, webhook, SDK, OAuth flow, or
customer-facing setup screen.

The hard part is identity and routing. For example, a single Slack app
installation in a workspace creates one bot identity and one workspace-specific
bot token. The operator's Slack app setup backend must decide which Talon
namespace that installation belongs to and create a Talon Connector with a
provider-specific message match. The runtime connector should normalize Slack
events and send them to the registered Talon cluster; Talon uses Connector
matches to choose the namespace and route.

## Non-goals

- Talon does not implement Slack OAuth directly in v1.
- Talon does not store Slack bot tokens or iMessage provider credentials in v1.
- The connector protocol does not define customer-facing OAuth setup,
  namespace selection, or account linking UX.
- Talon does not support shared ownership or fanout of one external messaging
  account across multiple Talon clusters in v1.
- Talon does not require every connector implementation to live in the Talon OSS
  repository.

## Design Principles

1. Keep provider credentials outside Talon.

   Slack bot tokens, Slack signing secrets, BlueBubbles server credentials, and
   Photon credentials belong to the operator's setup backend or connector
   runtime, not Talon. Talon stores the external connection handle needed for
   routing and delivery.

2. Make the Talon match boundary explicit.

   Talon should resolve inbound events by `(registrationId, provider match)`.
   Provider IDs such as Slack `team_id` are not globally meaningful, but they
   are meaningful inside the ConnectorClass registration that received the
   event. Talon should index Connector match rules under the registered
   ConnectorClass.

3. Separate operator setup from runtime delivery.

   The operator's app backend handles Slack OAuth, Talon/operator account
   authorization, namespace selection, and Connector creation. The connector
   protocol handles message events, delivery, and Connector health/status.

4. Let Talon own Talon routing.

   The connector runtime decides which Talon registration receives an event.
   Talon then matches the provider message against Connector resources to
   choose the namespace, agent, Session, or Channel.

5. Make v1 exclusive by default.

   One external provider account should have one active Talon connection in v1.
   More powerful policies can be added later with explicit conflict handling.

6. Keep delivery reliability bounded.

   Talon owns dispatch into its own Session/Channel model and reliable handoff
   to the connector service. Once the connector service accepts an outbound
   delivery, the connector service owns provider-specific delivery mechanics,
   retries, rate limits, final provider status, and provider credentials.

## Core Concepts

The design has three responsibility domains.

Operator setup backend records:

- `ProviderAppProfile`
- `ProviderAccount`

Connector runtime records:

- `TalonRegistration`
- provider credential/runtime lookup, if the runtime and setup backend share
  storage

Talon resources:

- `ConnectorClass`
- `Connector`

Protocol handles visible to Talon:

- `registrationId`, stored for the ConnectorClass registration
- provider match fields, stored on tenant Connectors
- provider metadata, such as Slack workspace name, `team_id`, bot user ID, or
  iMessage profile ID

Talon v1 should not persist `providerAccountId`. It is setup-backend/runtime
internal. Exposing it would create a second authorization-looking handle without
giving Talon any authority over the provider account.

### ConnectorClass

`ConnectorClass` is a regular namespace resource that describes a trusted
connector service endpoint. Operators can define connector classes in the
namespace that owns the integration, and Connectors in that namespace can refer
to the class by name. Talon v1 rejects cross-namespace `classRef.namespace`
values until there is an explicit reference policy or RBAC check.

It answers:

- Which connector service should Talon call?
- Which platform or platform family does it support?
- How does Talon authenticate to that service?
- Which protocol version and capabilities are available?

Example:

```yaml
kind: ConnectorClass
metadata:
  namespace: customer-acme
  name: slack
spec:
  platform: slack
  runtime:
    kind: externalService
    endpoint: https://slack-connector.example.com
  auth:
    kind: apiKey
    apiKey:
      ref:
        source: env
        key: TALON_SLACK_CONNECTOR_API_KEY
```

`ConnectorClass.spec.auth.apiKey` identifies the Talon cluster or Talon operator
account to the connector service. It does not identify a Slack workspace,
iMessage account, or customer tenant.

The API key should use the same secret reference pattern as Talon configuration
secrets. Inline/plain values may be acceptable for local development and
bootstrap, but production deployments should reference an external secret
source.

### TalonRegistration

`TalonRegistration` is a connector-service-side record created when
`ConnectorController` registers a Talon cluster with a connector service.

It answers:

- Which Talon cluster or operator account is this?
- Where should the connector service send callbacks?
- Which signing material should protect callbacks?

Talon stores the returned `registrationId` in `ConnectorClass.status` and writes
a controller-managed `ConnectorRegistration` index in the Sys namespace. The
connector service stores the callback URL and callback authentication metadata.

### ProviderAccount

`ProviderAccount` is an operator-setup-backend record for the real external
messaging account or installation.

Examples:

- Slack: one ProviderAppProfile installation into one workspace or enterprise
  context.
- BlueBubbles: one BlueBubbles server or Apple Messages identity.
- Photon: one Photon/Spectrum project or account.

Provider credentials live here or in the connector runtime's credential store.
Talon should not need to know the provider account ID in order to authorize
routing.

### ProviderAppProfile

`ProviderAppProfile` is an operator-setup-backend record for provider app
configuration.

For Slack, this is the Slack app configuration:

- client ID
- client secret
- signing secret
- OAuth redirect URLs
- bot scopes
- event subscriptions

This matters because one connector service may own several Slack apps. A Slack
provider account is not merely "workspace T123"; it is the installation of a
specific Slack app profile into workspace or enterprise context T123.

Talon does not select provider app profiles in v1. The operator's setup backend
chooses the Slack app, OAuth profile, and customer-facing setup flow.

### Connector

`Connector` is a tenant namespace resource in Talon. It represents a
single external-message route that is available inside that namespace.

It answers:

- Which connector class handles this external platform?
- Which provider messages belong to this namespace?
- Which Talon target should receive matching messages?

Example:

```yaml
kind: Connector
metadata:
  namespace: customer-acme
  name: slack-main
spec:
  classRef:
    name: slack
  enabled: true
  matchFields:
    teamId: T123
    enterpriseId: E456
  target:
    channel:
      channel: campaigns
      agent: marketing-agent
      continuity: reuse
      replyPolicy: thread
```

The normal flow is:

1. The user installs the operator's Slack app or connects another provider
   through the operator's setup backend.
2. The setup backend authenticates the user against the operator's account
   system and chooses the target Talon namespace and Connector.
3. The setup backend determines the provider match fields, such as Slack
   workspace/team ID or iMessage profile ID.
4. The setup backend creates or updates the Talon Connector with `classRef`,
   `matchFields`, and `target`.

Provider-native IDs such as Slack `team_id` are not global authorization
boundaries. They are match fields scoped by the namespaced ConnectorClass
registration. Talon should index them with `registrationId` and the
ConnectorClass namespace/name.

For Slack, route by stable Slack IDs, not mutable names. If a connector is
channel-specific, prefer `channelId` over channel name. A name may be kept as
display metadata only.

For iMessage, match fields depend on the provider. Examples include phone
number, Apple Messages handle, BlueBubbles chat GUID, or Photon/iMessage profile
ID.

Each v1 Connector is one route. It has one top-level `matchFields` map and one
top-level `target`. To route different Slack channels, iMessage profiles, or
conversation classes differently, create multiple Connector resources with
different `matchFields`.

## Ownership Policy

V1 should use exclusive ownership:

```text
one ProviderAccount -> one operator backend connection -> one primary Talon Connector match
```

If a second Talon registration tries to connect the same provider account, the
operator setup backend should reject or explicitly transfer the setup:

```http
409 provider_account_already_connected
```

Example response:

```json
{
  "error": "provider_account_already_connected",
  "providerAccountStatus": "connected",
  "transferSupported": true
}
```

The response should not leak another operator's namespace, connector name, or
callback URL unless the setup backend has established that the requester is
authorized to see that information.

Future ownership modes may include:

- `exclusive`: one provider account has one active Talon connection.
- `partitioned`: multiple Connectors share one provider account with disjoint
  match scopes, such as separate Slack channel IDs.
- `fanout`: the connector service sends the same event to multiple Talon
  registrations. This is likely only safe for passive listeners.
- `customer_oauth_profile`: each operator or customer supplies a distinct
  provider app profile, producing separate external bot identities where the
  provider supports that.

Partitioning and fanout are not v1 features because they require conflict
resolution before Talon receives the event.

## Connector-Service Inbound Routing

The connector runtime performs the first routing step because provider webhooks
arrive there, not in Talon.

For each provider event, the connector service should:

1. Verify the provider webhook, request signature, or stream authentication.
2. Identify the `ProviderAppProfile` that received the event.
3. Identify the `ProviderAccount` using provider-native account/install fields.
4. Find the `TalonRegistration` associated with that provider app/profile.
5. Normalize the provider event into `ConnectorMessageEvent`.
6. Send the event to the registration's Talon callback URL with
   `registrationId` and provider-native match fields.

For Slack Events API, the provider account lookup must include the Slack app
identity, such as `api_app_id` or connector-service app profile, plus workspace
or enterprise context such as `team_id` and `enterprise_id`. Looking up by
`team_id` alone is not sufficient because the same workspace may install
multiple Slack apps.

If no TalonRegistration exists for the provider app/profile, the connector
runtime should ignore or dead-letter the event. It should not guess a Talon
cluster from provider IDs.

## Connector Service Protocol

The connector service exposes endpoints under `ConnectorClass.spec.runtime.endpoint`.

### Discovery

```http
GET /.well-known/talon-connector
```

Response:

```json
{
  "protocolVersion": "v1",
  "platform": "slack",
  "capabilities": {
    "deliveries": true,
    "messageEvents": true,
    "connectionStatus": true,
    "attachments": true
  },
  "providerAccountConnectionPolicy": "exclusive",
  "supportsPartitionedConnections": false,
  "supportsFanout": false
}
```

Field ownership:

| Field | Provided by | Why it exists |
| --- | --- | --- |
| `protocolVersion` | Connector service | Lets Talon reject incompatible protocol versions before registration. |
| `platform` | Connector service | Confirms that the endpoint matches the declared `ConnectorClass.spec.platform`. |
| `capabilities` | Connector service | Allows Talon to enable only supported runtime delivery, callback, status, and attachment features. |
| `providerAccountConnectionPolicy` | Connector service | Tells Talon whether one external account can be shared. V1 expects `exclusive`. |
| `supports*` flags | Connector service | Prevents Talon from assuming advanced runtime routing modes exist. |

### Cluster Registration

```http
POST /v1/clusters/register
Authorization: Bearer <ConnectorClass api key>
```

Request:

```json
{
  "clusterId": "talon-agency-prod",
  "namespace": "customer-acme",
  "connectorClass": "slack",
  "callbackBaseUrl": "https://talon.example.com/v1/connectors",
  "protocolVersion": "v1"
}
```

Response:

```json
{
  "registrationId": "reg_abc",
  "callbackAuth": {
    "kind": "hmac-sha256",
    "keyId": "key_1"
  },
  "status": "active"
}
```

Field ownership:

| Field | Provided by | Why it exists |
| --- | --- | --- |
| `clusterId` | Talon | Stable operator-defined cluster identity for connector-service diagnostics and policy. |
| `namespace` | Talon | Identifies the ConnectorClass namespace for this registration so the connector service does not infer Talon tenant/class from the API key. |
| `connectorClass` | Talon | Identifies the ConnectorClass name for this registration and gives callbacks a stable consistency check. |
| `callbackBaseUrl` | Talon | Tells the connector service where to send message and status callbacks. |
| `protocolVersion` | Talon | Confirms the requested protocol contract. |
| `registrationId` | Connector service | Stable connector-service handle for this Talon registration. Used on every callback and delivery. |
| `callbackAuth` | Connector service | Describes how connector-to-Talon callbacks will be authenticated. |
| `status` | Connector service | Lets Talon surface registration readiness. |

## Operator Setup Backend

The connector protocol does not define Slack OAuth, namespace selection, or
operator/customer login. Those belong to the operator's Slack app backend or
equivalent product setup service.

For Slack, the setup backend typically handles this flow:

1. User installs the operator's Slack app directly through Slack OAuth.
2. Slack redirects to the operator setup backend.
3. The setup backend stores the Slack bot token and provider metadata.
4. The setup backend authenticates the user against the operator's account
   system.
5. The setup backend determines the target Talon namespace, Connector name,
   provider match fields, and Talon target.
6. The setup backend creates or updates the Talon Connector resource through
   Talon's normal authenticated resource API.

The Connector resource is therefore authored by an actor that is already
authorized to manage the target namespace. Talon does not accept namespace
selection from Slack OAuth or from a generic connector setup callback.

Example Connector created by setup:

```yaml
kind: Connector
metadata:
  namespace: customer-acme
  name: slack-main
spec:
  classRef:
    name: slack
  enabled: true
  matchFields:
    teamId: T123
  target:
    session:
      agent: marketing-agent
      continuity: reuse
```

The setup backend may also configure the connector runtime so the Slack app or
provider profile maps to the correct `registrationId`. That mapping is only
cluster-level. Namespace routing happens in Talon by evaluating
`Connector.spec.matchFields`.

## Talon Callback Protocol

The connector runtime calls Talon under the registered `callbackBaseUrl`.

Every callback must include:

- `registrationId`
- timestamp
- nonce or idempotency key
- signature

Talon must reject callbacks with unknown registrations, invalid signatures,
stale timestamps, replayed nonces, or disabled ConnectorClasses.

### Connector Status

The connector runtime or setup backend may report Connector health changes, but
it must not use this callback to choose namespaces or create initial namespace
bindings. Connector creation belongs to the operator setup backend using
Talon's authenticated resource API.

```http
POST /v1/connectors/status
```

Payload:

```json
{
  "registrationId": "reg_abc",
  "matchFields": {
    "teamId": "T123"
  },
  "status": "degraded",
  "reason": "provider_token_revoked"
}
```

Field ownership:

| Field | Provided by | Why it exists |
| --- | --- | --- |
| `registrationId` | Connector runtime/setup backend | Identifies the Talon registration this status belongs to. |
| `matchFields` | Connector runtime/setup backend | Provider-native match metadata used to resolve affected Connectors. |
| `status` | Connector runtime/setup backend | Reports connection health: `connected`, `degraded`, `disabled`, or `revoked`. |
| `reason` | Connector runtime/setup backend | Operator-facing diagnostic reason. |

### Message Event

```http
POST /v1/connectors/message-events
```

Canonical event shape:

```proto
message ConnectorMessageEvent {
  string event_id = 1;
  string event_kind = 2;
  string registration_id = 3;
  string connector_class = 4;
  map<string, string> match_fields = 5;
  string external_conversation_id = 6;
  optional string external_thread_id = 7;
  string external_message_id = 8;
  string conversation_type = 9;
  ConnectorActor sender = 10;
  string text = 11;
  repeated ConnectorAttachment attachments = 12;
  int64 event_time_ms = 13;
  map<string, string> labels = 14;
}
```

Field ownership:

| Field | Provided by | Why it exists |
| --- | --- | --- |
| `event_id` | Connector service/provider | Idempotency key. Talon deduplicates by `registrationId + eventId`. |
| `event_kind` | Connector service | Distinguishes message create/edit/delete/reaction/delivery events. V1 may only process `message_created`. |
| `registration_id` | Connector service | Identifies which Talon registration the callback targets. |
| `connector_class` | Connector service | Defensive consistency check against the ConnectorClass registration. |
| `match_fields` | Provider, normalized by connector service | Provider-specific route fields such as Slack `teamId` or iMessage profile ID. Talon uses these fields to match Connector resources. |
| `external_conversation_id` | Provider, normalized by connector service | Stable provider conversation key such as Slack channel/DM ID or iMessage chat GUID. Used for Connector matching and routing. |
| `external_thread_id` | Provider, normalized by connector service | Thread key where the provider supports threads. Optional for iMessage. |
| `external_message_id` | Provider, normalized by connector service | Provider message ID used for reply, edit, delete, and delivery correlation. |
| `conversation_type` | Connector service | Normalized route type: `dm`, `group`, `channel`, or `channel_thread`. |
| `sender` | Connector service/provider | Lets Talon identify the human/bot/system sender without provider-specific parsing. |
| `text` | Connector service/provider | Normalized text content delivered to the agent. |
| `attachments` | Connector service/provider/Talon object store | Describes files and media without requiring Talon to understand every provider API. |
| `event_time_ms` | Provider or connector service | Stable ordering and audit timestamp. |
| `labels` | Connector service | Escape hatch for provider-specific routing hints without schema churn. |

### Connection Revoked

Provider revocation and disconnects should use the Connector status callback.

Talon should mark the Connector degraded or disabled rather than deleting it
automatically. Deletion is an operator action unless the owner explicitly
requests disconnect.

## Outbound Delivery

Talon sends outbound messages to the connector service:

```http
POST /v1/deliveries
Authorization: Bearer <ConnectorClass api key>
```

This is a connector-service acceptance boundary. TCP and HTTP success only tell
Talon that the connector service received the request. The connector service
response tells Talon whether the service accepted responsibility for provider
delivery. After acceptance, the connector service owns Slack/iMessage/provider
retries, rate limits, idempotency against the provider, and final delivery
state.

Payload:

```proto
message ConnectorDeliveryRequest {
  string delivery_id = 1;
  string registration_id = 2;
  string connector_class = 3;
  string namespace = 4;
  string connector_name = 5;
  map<string, string> match_fields = 6;
  string external_conversation_id = 7;
  optional string external_thread_id = 8;
  optional string reply_to_external_message_id = 9;
  string text = 10;
  repeated ConnectorAttachment attachments = 11;
  map<string, string> labels = 12;
}

message ConnectorDeliveryResponse {
  bool accepted = 1;
  string disposition = 2; // accepted | duplicate | rejected
  string error = 3;
}
```

Field ownership:

| Field | Provided by | Why it exists |
| --- | --- | --- |
| `delivery_id` | Talon | Idempotency for Talon-to-connector-service handoff. Connector services must treat duplicate delivery IDs as the same accepted/rejected request. |
| `registration_id` | Talon, originally connector service | Ensures delivery is for the registered Talon cluster. |
| `connector_class` | Talon | Defensive consistency check and runtime diagnostics. |
| `namespace` | Talon | Operator-facing diagnostics and audit context. The connector service must not use this as provider authorization. |
| `connector_name` | Talon | Identifies the Talon Connector that owns this outbound route. |
| `match_fields` | Talon, copied from Connector match/event | Provider-specific route context such as Slack `teamId` or iMessage account/profile ID. |
| `external_conversation_id` | Talon, copied from route/event | Provider conversation target. |
| `external_thread_id` | Talon, copied from route/event | Provider thread target, if applicable. |
| `reply_to_external_message_id` | Talon, copied from event | Allows connector service to send replies or provider-native threading where supported. |
| `text` | Talon agent/runtime | Message text to send externally. |
| `attachments` | Talon runtime/object store | Files or media to deliver externally. |
| `labels` | Talon | Provider-specific hints such as Slack `reply_broadcast`. |

Response semantics:

- `accepted`: connector service accepted responsibility for provider delivery.
  Talon records the delivery as handed off and does not retry.
- `duplicate`: connector service has already accepted or rejected the same
  `delivery_id`. Talon treats this as terminal and does not retry.
- `rejected`: connector service rejected the request as invalid or unauthorized.
  Talon records a terminal failure and does not retry.
- Timeout, network failure, or HTTP 5xx: Talon may retry the same
  `delivery_id`.

There is no required delivery-ack callback in v1. Provider-level statuses such
as Slack `sent`/`failed` or iMessage `delivered`/`read` are connector-service
responsibility after acceptance. A later observability extension may add
optional status callbacks, but they must not be required for correctness.

## Efficient Inbound Routing In Talon

Talon must not scan namespaces or routes for inbound events. In v1,
`ConnectorController` writes a `ConnectorRegistration` entry in the Sys namespace
keyed by `registrationId`. The gateway reads that single entry, validates the
optional `connector_class` consistency field, and then resolves provider match
fields under the indexed ConnectorClass namespace/name.

Required indexes:

```text
registrationId -> ConnectorClass namespace/name + registration state
registrationId + classNamespace + className + provider match key -> connectorUid, namespace, connectorName, classNamespace, className, enabled
registrationId + eventId -> idempotency record
```

Inbound flow:

1. Read `ConnectorRegistration(registrationId)`.
2. Verify callback signature and timestamp.
3. Resolve the event provider fields against Connector match indexes under the
   registration's ConnectorClass.
4. Reject if `connector_class` conflicts with the resolved ConnectorClass.
5. Deduplicate `event_id`.
6. Dispatch to `Connector.spec.target`.

Connector match precedence is provider-specific but must be deterministic. For
Slack, preferred precedence is:

1. App/profile + enterprise ID + team ID + channel ID
2. App/profile + team ID + channel ID
3. App/profile + enterprise ID + team ID
4. App/profile + team ID

This lets a workspace-level Connector handle all messages while a
channel-specific Connector overrides selected channels. Talon should reject
ambiguous Connectors at write time or use an explicit priority field; it should
not randomly choose between equally specific matches.

## Session And Channel Targets

The route target decides whether external messages enter a Session or Channel.

Sessions are useful when:

- A conversation should preserve long-lived working memory.
- A DM should map to one durable agent conversation.
- The external provider conversation is itself the interaction unit.

Channels are useful when:

- External messages should be treated as a message stream rather than one long
  agent session.
- The operator wants a fresh session per message, per thread, or per routing
  policy.
- Multiple users participate and Talon should expose the interaction as a
  shared collaboration surface.

Example target:

```yaml
target:
  session:
    agent: marketing-agent
    continuity: reuse
```

Example Channel target:

```yaml
target:
  channel:
    channel: campaigns
    agent: marketing-agent
    continuity: newPerMessage
```

`continuity` should be explicit. It controls whether Talon reuses an existing
runtime context or creates a new one for each message/thread.

## Slack Connector

Slack setup backend responsibilities:

- Own one or more Slack apps.
- Own Slack OAuth redirect handling and token exchange.
- Store Slack bot tokens per workspace installation.
- Authenticate the installer against the operator's account system.
- Select the Talon namespace, Connector name, and target.
- Determine Slack match fields such as `teamId`, `enterpriseId`, and optional
  channel IDs.
- Create or update the Talon Connector through Talon's resource API.

Slack connector runtime responsibilities:

- Verify Slack request signatures.
- Receive Slack Events API events or Socket Mode events.
- Normalize Slack events into `ConnectorMessageEvent`.
- Send outbound messages using Slack APIs such as `chat.postMessage`.

Slack terminology:

```text
Slack App
  Configured app: client ID, client secret, scopes, event subscriptions, signing secret.

Slack OAuth Installation
  A workspace admin/user installs the Slack App into a workspace.

Bot User
  The app's bot identity in that workspace.

Bot Token
  The workspace-specific xoxb token for that bot user.
```

Connector mapping:

```text
Slack App                         -> ProviderAppProfile
Slack OAuth Installation           -> ProviderAccount
Slack bot token                    -> ProviderAccount/runtime credential
Slack team_id                      -> Connector.spec.matchFields.teamId and event matchFields.teamId
Slack channel, MPIM, or DM ID       -> externalConversationId
Slack thread_ts                    -> externalThreadId
Slack event_id                     -> eventId
Slack message ts                   -> externalMessageId
```

V1 policy:

- One Slack OAuth installation should map to one primary Talon workspace-level
  Connector match by default.
- A second Talon registration attempting to bind the same installation should
  receive
  `409 provider_account_already_connected`.
- Channel-specific Connector matches may be layered on later if Talon enforces
  deterministic match specificity or explicit priority.
- If two independent bots are required in one Slack workspace, use separate
  Slack apps/OAuth profiles so Slack creates distinct bot identities.

Suggested routing defaults:

- Slack DM: `conversationType=dm`, route to Session with `continuity=reuse`.
- Slack channel mention: `conversationType=channel`, route only if mention or
  route policy expects it.
- Slack thread reply: `conversationType=channel_thread`, preserve
  `externalThreadId`.
- Slack top-level mention: connector may use message `ts` as the thread key for
  subsequent replies.

## iMessage Connectors

iMessage should be provider-specific at the ConnectorClass level:

```text
imessage-bluebubbles
imessage-photon
```

They share a platform family but differ in runtime, credentials, reliability,
webhook/stream behavior, attachment support, and account identity.

BlueBubbles mapping:

```text
BlueBubbles server / Apple Messages identity -> ProviderAccount
Chat GUID                                    -> externalConversationId
Message GUID                                 -> externalMessageId
Thread ID                                    -> absent
```

Photon mapping:

```text
Photon/Spectrum project or account           -> ProviderAccount
Provider conversation/space ID               -> externalConversationId
Provider message ID                          -> externalMessageId
Thread ID                                    -> absent unless provider supports one
```

iMessage connector events should support:

- `sender.externalAddress` for phone/email identity.
- `conversationDisplayName` in labels or provider metadata.
- Attachments with `objectKey`, `externalUrl`, `expiresAt`, and `mediaType`.
- Delivery statuses such as `accepted`, `sent`, `delivered`, `read`,
  `undeliverable`, and `rate_limited` where supported.

Suggested routing defaults:

- iMessage DM: Session with `continuity=reuse`.
- iMessage group: Channel or Session depending on operator preference.

## Security Requirements

- Namespaced `ConnectorClass` API keys authenticate Talon to connector services.
- Connector-to-Talon callbacks must be signed per registration.
- Callback timestamps must be bounded.
- Callback nonces or event IDs must be replay-protected.
- `(registrationId, provider match)` must resolve to exactly one enabled
  Connector in Talon, or to a deterministic most-specific Connector.
- Talon must reject callbacks where `registrationId`, `connector_class`, or
  provider match fields are inconsistent with the resolved ConnectorClass.
- Talon must not trust provider-native IDs as authorization boundaries.
- Connector services must not leak existing owner metadata in conflict responses
  unless authorized.

## Implementation Plan

1. Add proto resource definitions:
   - `ConnectorClass`
   - `Connector`

2. Add connector protocol messages:
   - `ConnectorMessageEvent`
   - `ConnectorDeliveryRequest`
   - `ConnectorDeliveryResponse`
   - `ConnectorStatusEvent`

3. Wire resources into the resource oneof and generated schemas.

4. Add `ConnectorController`:
   - watch namespaced `ConnectorClass`
   - discover connector service
   - register cluster
   - maintain readiness/status

5. Add callback ingest API:
   - verify registration/signature
   - ingest message events
   - ingest Connector status changes

6. Add inbound routing:
   - index `(registrationId, provider match key) -> Connector`
   - dispatch to Connector target
   - create/reuse Session or post into Channel

7. Add outbound delivery:
   - convert agent responses into `ConnectorDeliveryRequest`
   - call connector service
   - retry only failed Talon-to-connector handoff attempts
   - treat `accepted`, `duplicate`, and `rejected` connector responses as terminal

8. Define the operator setup backend integration:
   - setup backend creates/updates Connector through Talon's resource API
   - setup backend writes provider-specific match fields into Connector spec
   - Talon does not define Slack OAuth setup endpoints

9. Build a Slack reference connector runtime.

10. Build one iMessage connector runtime after Slack, preferably choosing either
   BlueBubbles or Photon first rather than abstracting both prematurely.

## Open Questions

- Should Connector match conflicts be rejected at write time, or should Talon
  support an explicit priority field for overlapping matches?
- How should Talon represent dead-lettered connector events for operator review?
- How much provider metadata should be copied into `Connector.status` before it
  becomes a leaky mirror of connector-service internals?
