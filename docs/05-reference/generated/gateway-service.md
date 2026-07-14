---
title: Talon v1 Services
sidebar:
  order: 2
---

The Talon gateway API is defined by the domain service files in `proto/talon/v1/*.proto`. They are the canonical first-class gRPC and gRPC-Web contract exposed directly by the gateway.

## Surface summary

- Package: `talon.v1`
- Services: `NamespaceService`, `ResourceService`, `SessionService`, `ChannelService`, `WorkflowService`, `KnowledgeService`, `AuthService`, `ConnectorService`, `SearchService`
- Transport modes: native gRPC and gRPC-Web on the gateway port
- Total RPC methods: **45**

## NamespaceService

### `Create`

- Request: `CreateNamespaceRequest`
- Response: `NamespaceResponse`

### `Get`

- Request: `GetNamespaceRequest`
- Response: `NamespaceResponse`

### `Delete`

- Request: `DeleteNamespaceRequest`
- Response: `NamespaceResponse`

### `List`

- Request: `ListNamespacesRequest`
- Response: `ListNamespacesResponse`

## ResourceService

### `Create`

- Request: `CreateResourceRequest`
- Response: `ResourceResponse`

### `Get`

- Request: `GetResourceRequest`
- Response: `ResourceResponse`

### `List`

- Request: `ListResourcesRequest`
- Response: `ListResourcesResponse`

### `Delete`

- Request: `DeleteResourceRequest`
- Response: `DeleteResourceResponse`

## SessionService

### `Create`

- Request: `CreateSessionRequest`
- Response: `SessionResponse`

### `Get`

- Request: `GetSessionRequest`
- Response: `SessionResponse`

### `List`

- Request: `ListSessionsRequest`
- Response: `ListSessionsResponse`

### `ListMessages`

- Request: `ListSessionMessagesRequest`
- Response: `ListSessionMessagesResponse`

### `Delete`

- Request: `DeleteSessionRequest`
- Response: `DeleteSessionResponse`

### `Clear`

- Request: `ClearSessionRequest`
- Response: `ClearSessionResponse`

### `SendMessage`

- Request: `SendMessageRequest`
- Response: `SendMessageResponse`

### `AppendMessage`

- Request: `AppendSessionMessageRequest`
- Response: `AppendSessionMessageResponse`

### `UpdateMessage`

- Request: `UpdateSessionMessageRequest`
- Response: `UpdateSessionMessageResponse`

### `AnswerPermission`

- Request: `AnswerSessionPermissionRequest`
- Response: `AnswerSessionPermissionResponse`

### `StopGeneration`

- Request: `StopSessionGenerationRequest`
- Response: `StopSessionGenerationResponse`

### `StreamParts`

- Request: `StreamSessionPartsRequest`
- Response: `talon.events.SessionMessagePartEvent` (server stream)

### `StreamPartsBatch`

- Request: `StreamSessionPartsBatchRequest`
- Response: `talon.events.SessionMessagePartEvent` (server stream)

### `SubmitTurn`

- Request: `SubmitSessionTurnRequest`
- Response: `talon.events.SessionMessagePartEvent` (server stream)

## ChannelService

### `PostMessage`

- Request: `PostChannelMessageRequest`
- Response: `PostChannelMessageResponse`

### `GetMessage`

- Request: `GetChannelMessageRequest`
- Response: `ChannelMessageResponse`

### `ListMessages`

- Request: `ListChannelMessagesRequest`
- Response: `ListChannelMessagesResponse`

### `StreamEvents`

- Request: `StreamChannelEventsRequest`
- Response: `talon.events.ChannelEvent` (server stream)

## WorkflowService

### `CreateRun`

- Request: `CreateWorkflowRunRequest`
- Response: `WorkflowRunResponse`

### `GetRun`

- Request: `GetWorkflowRunRequest`
- Response: `WorkflowRunResponse`

### `ListRuns`

- Request: `ListWorkflowRunsRequest`
- Response: `ListWorkflowRunsResponse`

### `ResumeRun`

- Request: `ResumeWorkflowRunRequest`
- Response: `WorkflowRunResponse`

### `CancelRun`

- Request: `CancelWorkflowRunRequest`
- Response: `WorkflowRunResponse`

### `StreamEvents`

- Request: `StreamWorkflowEventsRequest`
- Response: `talon.data.WorkflowRunEvent` (server stream)

## KnowledgeService

### `Get`

- Request: `GetKnowledgeRequest`
- Response: `KnowledgeResponse`

### `Search`

- Request: `SearchKnowledgeRequest`
- Response: `SearchKnowledgeResponse`

## AuthService

### `GetSsoConfig`

- Request: `GetSsoConfigRequest`
- Response: `GetSsoConfigResponse`

### `ExchangeOidcToken`

- Request: `ExchangeOidcTokenRequest`
- Response: `ExchangeOidcTokenResponse`

### `MintAccessToken`

- Request: `MintAccessTokenRequest`
- Response: `MintAccessTokenResponse`

### `CreateApiKey`

- Request: `CreateApiKeyRequest`
- Response: `CreateApiKeyResponse`

### `ListApiKeys`

- Request: `ListApiKeysRequest`
- Response: `ListApiKeysResponse`

### `RevokeApiKey`

- Request: `RevokeApiKeyRequest`
- Response: `RevokeApiKeyResponse`

### `ExchangeApiKey`

- Request: `ExchangeApiKeyRequest`
- Response: `ExchangeApiKeyResponse`

## ConnectorService

### `IngestMessageEvent`

IngestMessageEvent delivers one normalized provider message event to Talon. Talon deduplicates under the ConnectorClass registration by event_id, resolves a Connector by match_fields, and dispatches the message to the resolved message consumer.

- Request: `talon.external.ConnectorMessageEvent`
- Response: `talon.external.ConnectorMessageEventResponse`

### `ReportStatus`

ReportStatus lets the connector service report registration or provider connection health without sending a message event.

- Request: `talon.external.ConnectorStatusEvent`
- Response: `talon.external.ConnectorAckResponse`

## SearchService

### `Search`

- Request: `SearchRequest`
- Response: `SearchResponse`

### `GetResult`

- Request: `GetSearchResultRequest`
- Response: `GetSearchResultResponse`
