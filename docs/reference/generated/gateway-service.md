---
title: Gateway Service
sidebar_position: 2
---

The Talon gateway is defined in `proto/gateway.proto`. It is the canonical contract for both gRPC and the REST-transcoded HTTP surface exposed through the gateway and Envoy.

## Surface summary

- Service: `talon.gateway.GatewayService`
- Transport modes: gRPC, gRPC-web, REST via `google.api.http` annotations, and the browser-oriented `/v1/ui/... ` stream path documented separately in the hand-written guides
- Total RPC methods: **32**

## Knowledge

### `GetKnowledge`

Agent knowledge data-plane queries

- Request: `GetKnowledgeRequest`
- Response: `KnowledgeResponse`
- REST mapping: `GET /v1/ns/{ns}/agents/{agent}/knowledge`

### `SearchKnowledge`

- Request: `SearchKnowledgeRequest`
- Response: `SearchKnowledgeResponse`
- REST mapping: `POST /v1/ns/{ns}/agents/{agent}/knowledge/search`
- REST body: `*`

## Sessions

### `CreateSession`

- Request: `CreateSessionRequest`
- Response: `SessionResponse`
- REST mapping: `POST /v1/ns/{ns}/agents/{agent}/sessions`
- REST body: `*`

### `GetSession`

- Request: `GetSessionRequest`
- Response: `SessionResponse`
- REST mapping: `GET /v1/ns/{ns}/agents/{agent}/sessions/{session_id}`

### `ListSessionMessages`

- Request: `ListSessionMessagesRequest`
- Response: `ListSessionMessagesResponse`
- REST mapping: `GET /v1/ns/{ns}/agents/{agent}/sessions/{session_id}/messages`

### `ListSessions`

- Request: `ListSessionsRequest`
- Response: `ListSessionsResponse`
- REST mapping: `GET /v1/ns/{ns}/agents/{agent}/sessions`

### `DeleteSession`

- Request: `DeleteSessionRequest`
- Response: `DeleteSessionResponse`
- REST mapping: `DELETE /v1/ns/{ns}/agents/{agent}/sessions/{session_id}`

### `ClearSession`

- Request: `ClearSessionRequest`
- Response: `ClearSessionResponse`
- REST mapping: `POST /v1/ns/{ns}/agents/{agent}/sessions/{session_id}:clear`
- REST body: `*`

### `SendMessage`

- Request: `SendMessageRequest`
- Response: `SendMessageResponse`
- REST mapping: `POST /v1/ns/{ns}/agents/{agent}/sessions/{session_id}/message`
- REST body: `*`

### `AppendSessionMessage`

- Request: `AppendSessionMessageRequest`
- Response: `AppendSessionMessageResponse`
- REST mapping: `POST /v1/ns/{ns}/agents/{agent}/sessions/{session_id}/messages:append`
- REST body: `*`

### `AnswerSessionPermission`

- Request: `AnswerSessionPermissionRequest`
- Response: `AnswerSessionPermissionResponse`
- REST mapping: `POST /v1/ns/{ns}/agents/{agent}/sessions/{session_id}/permissions/{request_id}:answer`
- REST body: `*`

### `StopSessionGeneration`

- Request: `StopSessionGenerationRequest`
- Response: `StopSessionGenerationResponse`
- REST mapping: `POST /v1/ns/{ns}/agents/{agent}/sessions/{session_id}:stop`
- REST body: `*`

### `StreamSessionParts`

- Request: `StreamSessionPartsRequest`
- Response: `talon.events.SessionMessagePartEvent` (server stream)
- REST mapping: `GET /v1/ns/{ns}/agents/{agent}/sessions/{session_id}/stream`

## Namespaces

### `CreateNamespace`

- Request: `CreateNamespaceRequest`
- Response: `NamespaceResponse`
- REST mapping: `POST /v1/namespaces/{name}`
- REST body: `*`

### `GetNamespace`

- Request: `GetNamespaceRequest`
- Response: `NamespaceResponse`
- REST mapping: `GET /v1/namespaces/{name}`

### `DeleteNamespace`

- Request: `DeleteNamespaceRequest`
- Response: `NamespaceResponse`
- REST mapping: `DELETE /v1/namespaces/{name}`

### `ListNamespaces`

- Request: `ListNamespacesRequest`
- Response: `ListNamespacesResponse`
- REST mapping: `GET /v1/namespaces`

## Other

### `StreamSessionPartsBatch`

- Request: `StreamSessionPartsBatchRequest`
- Response: `talon.events.SessionMessagePartEvent` (server stream)
- REST mapping: `POST /v1/session-streams:batch`
- REST body: `*`

### `PostChannelMessage`

Channel data-plane actions

- Request: `PostChannelMessageRequest`
- Response: `PostChannelMessageResponse`
- REST mapping: `POST /v1/ns/{ns}/channels/{channel}/messages`
- REST body: `*`

### `GetChannelMessage`

- Request: `GetChannelMessageRequest`
- Response: `ChannelMessageResponse`
- REST mapping: `GET /v1/ns/{ns}/channels/{channel}/messages/{message_id}`

### `ListChannelMessages`

- Request: `ListChannelMessagesRequest`
- Response: `ListChannelMessagesResponse`
- REST mapping: `GET /v1/ns/{ns}/channels/{channel}/messages`

### `StreamChannelEvents`

- Request: `StreamChannelEventsRequest`
- Response: `talon.events.ChannelEvent` (server stream)
- REST mapping: `GET /v1/ns/{ns}/channels/{channel}/stream`

### `CreateWorkflowRun`

Workflow data-plane actions

- Request: `CreateWorkflowRunRequest`
- Response: `WorkflowRunResponse`
- REST mapping: `POST /v1/ns/{ns}/workflows/{workflow}/runs`
- REST body: `*`

### `GetWorkflowRun`

- Request: `GetWorkflowRunRequest`
- Response: `WorkflowRunResponse`
- REST mapping: `GET /v1/ns/{ns}/workflows/{workflow}/runs/{run_id}`

### `ListWorkflowRuns`

- Request: `ListWorkflowRunsRequest`
- Response: `ListWorkflowRunsResponse`
- REST mapping: `GET /v1/ns/{ns}/workflows/{workflow}/runs`

### `ResumeWorkflowRun`

- Request: `ResumeWorkflowRunRequest`
- Response: `WorkflowRunResponse`
- REST mapping: `POST /v1/ns/{ns}/workflows/{workflow}/runs/{run_id}:resume`
- REST body: `*`

### `CancelWorkflowRun`

- Request: `CancelWorkflowRunRequest`
- Response: `WorkflowRunResponse`
- REST mapping: `POST /v1/ns/{ns}/workflows/{workflow}/runs/{run_id}:cancel`
- REST body: `*`

### `StreamWorkflowEvents`

- Request: `StreamWorkflowEventsRequest`
- Response: `talon.data.WorkflowRunEvent` (server stream)
- REST mapping: `GET /v1/ns/{ns}/workflows/{workflow}/runs/{run_id}/stream`

### `CreateResource`

Generic resources

- Request: `CreateResourceRequest`
- Response: `ResourceResponse`
- REST mapping: `POST /v1/ns/{ns}/resources`
- REST body: `*`

### `GetResource`

- Request: `GetResourceRequest`
- Response: `ResourceResponse`
- REST mapping: `GET /v1/ns/{ns}/resources/{kind}/{name}`

### `ListResources`

- Request: `ListResourcesRequest`
- Response: `ListResourcesResponse`
- REST mapping: `GET /v1/ns/{ns}/resources`

### `DeleteResource`

- Request: `DeleteResourceRequest`
- Response: `DeleteResourceResponse`
- REST mapping: `DELETE /v1/ns/{ns}/resources/{kind}/{name}`
