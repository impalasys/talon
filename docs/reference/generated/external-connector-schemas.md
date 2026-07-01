---
title: External Connector Schemas
---

This page summarizes the connector runtime contract that external connector services implement when registering clusters, receiving deliveries and activities, and calling back into Talon.

## `RegisterClusterRequest`

Talon accepted the event and dispatched or durably scheduled it. Talon had already processed this event_id under the ConnectorClass registration. The connector should not retry. Talon processed the event but found no matching Connector route. Usually not retryable unless the operator changes Connector configuration. Talon intentionally ignored the event, for example an unsupported event kind. Talon understood the event but refused to accept it, for example because the matched Connector is disabled. RegisterClusterRequest is sent by Talon to a connector service at POST /v1/clusters/register when a ConnectorClass becomes ready to receive callbacks.

| Field | Type | Notes |
| --- | --- | --- |
| `cluster_id` | `string` | Operator-defined Talon cluster identifier. |
| `registration_id` | `string` | Talon-owned ConnectorClass registration identifier, formatted as Namespace/<namespace>/ConnectorClass/<name>. |
| `namespace` | `string` | Namespace that owns the ConnectorClass. |
| `connector_class` | `string` | ConnectorClass name. |
| `callback_base_url` | `string` | Base URL for Talon's connector callback API. |
| `callback_auth_kind` | `string` | Authentication scheme the connector service must use for callbacks. |
| `callback_auth_key` | `string` | Secret value the connector service must present for callbacks. |
| `protocol_version` | `string` | Connector protocol version requested by Talon. |

## `RegisterClusterResponse`

RegisterClusterResponse is returned by the connector service from POST /v1/clusters/register.

| Field | Type | Notes |
| --- | --- | --- |
| `registration_id` | `string` | optional; Optional echo of Talon's registration_id. If present, it must match. |

## `ConnectorMessageEvent`

ConnectorMessageEvent delivers one normalized provider message event to Talon. Connector services call Talon's ConnectorService.IngestMessageEvent after provider webhook/OAuth handling.

| Field | Type | Notes |
| --- | --- | --- |
| `event_id` | `string` | Connector-service idempotency key for this normalized event. |
| `event_kind` | `ConnectorMessageEventKind` | Normalized event kind. V1 dispatch primarily handles CREATED. |
| `registration_id` | `string` | Talon-owned ConnectorClass registration identifier, formatted as Namespace/<namespace>/ConnectorClass/<name>. This scopes webhook delivery to one ConnectorClass registration. |
| `connector_class` | `string` | ConnectorClass name expected by the connector service. Talon uses this as a defensive consistency check when resolving the registration. |
| `match_fields` | `map<string, string>` | Provider-specific routing keys normalized by the connector service. Talon treats these as opaque values and matches them against Connector resources. |
| `external_conversation_id` | `string` | Stable provider conversation identifier, such as a Slack channel/DM ID or iMessage chat GUID. |
| `external_thread_id` | `string` | optional; Stable provider thread identifier when the platform has threads. Omitted for platforms or conversations without a thread concept. |
| `external_message_id` | `string` | Provider-native message identifier for delivery correlation, replies, edits, deletes, and deduplication diagnostics. |
| `conversation_type` | `string` | Normalized conversation shape, such as dm, group, channel, or channel_thread. |
| `sender` | `talon.data.Principal` | Normalized sender identity. |
| `text` | `string` | Plain text projection of the provider message. |
| `attachments` | `talon.data.ObjectRef` | repeated; Talon object-store references for attachments associated with the provider message. The connector service uploads provider attachments to Talon before sending the event. |
| `event_time_ms` | `int64` | Provider event timestamp in Unix milliseconds. Connector services should preserve provider time when available. |
| `labels` | `map<string, string>` | Connector-service-defined labels for diagnostics or provider-specific routing hints that do not justify a first-class protocol field. |

## `ConnectorMessageEventResponse`

| Field | Type | Notes |
| --- | --- | --- |
| `status` | `ConnectorMessageEventStatus` | Mutually exclusive Talon outcome for this message event. |
| `reason` | `string` | Machine-readable reason for rejected, ignored, or unmatched outcomes. |
| `namespace` | `string` | Namespace of the Connector that matched the event. Empty when unmatched or duplicate without a fresh route lookup. |
| `connector_name` | `string` | Name of the Connector that matched the event. Empty when unmatched or duplicate without a fresh route lookup. |
| `consumer` | `talon.data.MessageConsumer` | Consumer snapshot used for dispatch. Returned for observability so connector services can log which Talon destination accepted the event. |

## `ConnectorDeliveryRequest`

ConnectorDeliveryRequest is sent by Talon to a connector service at POST /v1/deliveries when an agent response should be delivered to the external provider.

| Field | Type | Notes |
| --- | --- | --- |
| `delivery_id` | `string` | Talon-generated idempotency key for this outbound delivery request. |
| `registration_id` | `string` | Talon-owned ConnectorClass registration identifier for the connector service that should perform provider delivery. |
| `connector_class` | `string` | ConnectorClass name associated with the outbound route. |
| `namespace` | `string` | Talon namespace that produced the outbound message. |
| `connector_name` | `string` | Connector resource name that provides provider routing context. |
| `match_fields` | `map<string, string>` | Provider-specific route fields copied from the matched Connector/event. |
| `external_conversation_id` | `string` | Provider conversation identifier to deliver into. |
| `external_thread_id` | `string` | optional; Provider thread identifier to deliver into when replying in a thread. |
| `reply_to_external_message_id` | `string` | optional; Provider message identifier this delivery should reply to, when applicable. |
| `text` | `string` | Plain text body produced by Talon. |
| `attachments` | `talon.data.ObjectRef` | repeated; Object-store references Talon wants the connector service to deliver to the provider. |
| `labels` | `map<string, string>` | Talon-defined delivery labels for diagnostics and provider-specific hints. |

## `ConnectorDeliveryResponse`

| Field | Type | Notes |
| --- | --- | --- |
| `accepted` | `bool` | True when the connector service has accepted responsibility for provider delivery. After this point, provider retries/rate limits are connector service responsibilities. |
| `disposition` | `string` | Machine-readable handoff outcome, such as accepted, duplicate, or rejected. |
| `error` | `string` | Diagnostic error string when accepted is false. |

## `ConnectorActivityRequest`

ConnectorActivityRequest is sent by Talon to a connector service at POST /v1/activities for provider-visible activity indicators such as typing.

| Field | Type | Notes |
| --- | --- | --- |
| `activity_id` | `string` | Talon-generated idempotency key for this activity notification. |
| `registration_id` | `string` | Talon-owned ConnectorClass registration identifier for the connector service that should perform provider activity. |
| `connector_class` | `string` | ConnectorClass name associated with the outbound route. |
| `namespace` | `string` | Talon namespace that produced the activity. |
| `connector_name` | `string` | Connector resource name that provides provider routing context. |
| `match_fields` | `map<string, string>` | Provider-specific route fields copied from the matched Connector/event. |
| `external_conversation_id` | `string` | Provider conversation identifier to deliver activity into. |
| `external_thread_id` | `string` | optional; Provider thread identifier to deliver activity into when applicable. |
| `kind` | `string` | Normalized activity kind, such as typing. |
| `phase` | `string` | Activity phase, such as started or stopped. |
| `status_text` | `string` | Optional human-readable provider activity text. |
| `labels` | `map<string, string>` | Talon-defined activity labels for diagnostics and provider-specific hints. |

## `ConnectorStatusEvent`

ConnectorStatusEvent lets the connector service report registration or provider connection health without sending a message event.

| Field | Type | Notes |
| --- | --- | --- |
| `registration_id` | `string` | Talon-owned ConnectorClass registration identifier whose health is being reported. |
| `match_fields` | `map<string, string>` | Optional provider-specific route fields identifying the affected Connector or provider account. |
| `status` | `string` | Connector-service-reported health, such as connected, degraded, disabled, or revoked. |
| `reason` | `string` | Machine-readable diagnostic reason, such as provider_token_revoked. |

## `ConnectorAckResponse`

| Field | Type | Notes |
| --- | --- | --- |
| `accepted` | `bool` | True when Talon accepted the status/report request. |
| `disposition` | `string` | Machine-readable acknowledgement outcome. |
