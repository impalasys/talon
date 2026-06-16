---
title: Gateway Service
sidebar_position: 2
---

The Talon gateway is defined in `proto/gateway.proto`. It is the canonical contract for both gRPC and the REST-transcoded HTTP surface exposed through the gateway and Envoy.

## Surface summary

- Service: `talon.gateway.GatewayService`
- Transport modes: gRPC, gRPC-web, REST via `google.api.http` annotations, and the browser-oriented `/v1/ui/... ` stream path documented separately in the hand-written guides
- Total RPC methods: **66**

## Agents

### `CreateAgent`

- Request: `CreateAgentRequest`
- Response: `AgentResponse`
- REST mapping: `POST /v1/ns/{ns}/agents`
- REST body: `*`

### `GetAgent`

- Request: `GetAgentRequest`
- Response: `GetAgentResponse`
- REST mapping: `GET /v1/ns/{ns}/agents/{name}`

### `ModifyAgent`

- Request: `ModifyAgentRequest`
- Response: `AgentResponse`
- REST mapping: `PUT /v1/ns/{ns}/agents/{agent}`
- REST body: `*`

### `ListAgents`

- Request: `ListAgentsRequest`
- Response: `ListAgentsResponse`
- REST mapping: `GET /v1/ns/{ns}/agents`

## Knowledge

### `GetKnowledge`

- Request: `GetKnowledgeRequest`
- Response: `KnowledgeResponse`
- REST mapping: `GET /v1/ns/{ns}/agents/{agent}/knowledge`

### `SearchKnowledge`

- Request: `SearchKnowledgeRequest`
- Response: `SearchKnowledgeResponse`
- REST mapping: `POST /v1/ns/{ns}/agents/{agent}/knowledge/search`
- REST body: `*`

### `CreateNamespaceKnowledge`

- Request: `CreateNamespaceKnowledgeRequest`
- Response: `NamespaceKnowledgeResponse`
- REST mapping: `POST /v1/namespaces/{ns}/knowledge`
- REST body: `*`

### `GetNamespaceKnowledge`

- Request: `GetNamespaceKnowledgeRequest`
- Response: `NamespaceKnowledgeResponse`
- REST mapping: `GET /v1/namespaces/{ns}/knowledge/{name}`

### `ListNamespaceKnowledge`

- Request: `ListNamespaceKnowledgeRequest`
- Response: `ListNamespaceKnowledgeResponse`
- REST mapping: `GET /v1/namespaces/{ns}/knowledge`

### `DeleteNamespaceKnowledge`

- Request: `DeleteNamespaceKnowledgeRequest`
- Response: `DeleteNamespaceKnowledgeResponse`
- REST mapping: `DELETE /v1/namespaces/{ns}/knowledge/{name}`

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

### `StopSessionGeneration`

- Request: `StopSessionGenerationRequest`
- Response: `StopSessionGenerationResponse`
- REST mapping: `POST /v1/ns/{ns}/agents/{agent}/sessions/{session_id}:stop`
- REST body: `*`

### `StreamSessionParts`

- Request: `StreamSessionPartsRequest`
- Response: `talon.events.SessionMessagePartEvent` (server stream)
- REST mapping: `GET /v1/ns/{ns}/agents/{agent}/sessions/{session_id}/stream`

## Schedules

### `CreateSchedule`

- Request: `CreateScheduleRequest`
- Response: `ScheduleResponse`
- REST mapping: `POST /v1/ns/{ns}/schedules`
- REST body: `*`

### `GetSchedule`

- Request: `GetScheduleRequest`
- Response: `ScheduleResponse`
- REST mapping: `GET /v1/ns/{ns}/schedules/{name}`

### `ModifySchedule`

- Request: `ModifyScheduleRequest`
- Response: `ScheduleResponse`
- REST mapping: `PUT /v1/ns/{ns}/schedules/{name}`
- REST body: `*`

### `ListSchedules`

- Request: `ListSchedulesRequest`
- Response: `ListSchedulesResponse`
- REST mapping: `GET /v1/ns/{ns}/schedules`

### `DeleteSchedule`

- Request: `DeleteScheduleRequest`
- Response: `DeleteScheduleResponse`
- REST mapping: `DELETE /v1/ns/{ns}/schedules/{name}`

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

## MCP

### `CreateMcpServer`

- Request: `CreateMcpServerRequest`
- Response: `McpServerResponse`
- REST mapping: `POST /v1/mcp-servers`
- REST body: `*`

### `GetMcpServer`

- Request: `GetMcpServerRequest`
- Response: `McpServerResponse`
- REST mapping: `GET /v1/mcp-servers/{name}`

### `ListMcpServers`

- Request: `ListMcpServersRequest`
- Response: `ListMcpServersResponse`
- REST mapping: `GET /v1/mcp-servers`

### `DeleteMcpServer`

- Request: `DeleteMcpServerRequest`
- Response: `DeleteMcpServerResponse`
- REST mapping: `DELETE /v1/mcp-servers/{name}`

### `CreateMcpServerBinding`

- Request: `CreateMcpServerBindingRequest`
- Response: `McpServerBindingResponse`
- REST mapping: `POST /v1/namespaces/{ns}/mcp-bindings`
- REST body: `*`

### `GetMcpServerBinding`

- Request: `GetMcpServerBindingRequest`
- Response: `McpServerBindingResponse`
- REST mapping: `GET /v1/namespaces/{ns}/mcp-bindings/{name}`

### `ListMcpServerBindings`

- Request: `ListMcpServerBindingsRequest`
- Response: `ListMcpServerBindingsResponse`
- REST mapping: `GET /v1/namespaces/{ns}/mcp-bindings`

### `DeleteMcpServerBinding`

- Request: `DeleteMcpServerBindingRequest`
- Response: `DeleteMcpServerBindingResponse`
- REST mapping: `DELETE /v1/namespaces/{ns}/mcp-bindings/{name}`

## Other

### `StreamSessionPartsBatch`

- Request: `StreamSessionPartsBatchRequest`
- Response: `talon.events.SessionMessagePartEvent` (server stream)
- REST mapping: `POST /v1/session-streams:batch`
- REST body: `*`

### `CreateChannel`

Channels

- Request: `CreateChannelRequest`
- Response: `ChannelResponse`
- REST mapping: `POST /v1/ns/{ns}/channels`
- REST body: `*`

### `GetChannel`

- Request: `GetChannelRequest`
- Response: `ChannelResponse`
- REST mapping: `GET /v1/ns/{ns}/channels/{name}`

### `ModifyChannel`

- Request: `ModifyChannelRequest`
- Response: `ChannelResponse`
- REST mapping: `PUT /v1/ns/{ns}/channels/{name}`
- REST body: `*`

### `ListChannels`

- Request: `ListChannelsRequest`
- Response: `ListChannelsResponse`
- REST mapping: `GET /v1/ns/{ns}/channels`

### `DeleteChannel`

- Request: `DeleteChannelRequest`
- Response: `DeleteChannelResponse`
- REST mapping: `DELETE /v1/ns/{ns}/channels/{name}`

### `PostChannelMessage`

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

### `CreateChannelSubscription`

- Request: `CreateChannelSubscriptionRequest`
- Response: `ChannelSubscriptionResponse`
- REST mapping: `POST /v1/ns/{ns}/channels/{channel}/subscriptions`
- REST body: `*`

### `GetChannelSubscription`

- Request: `GetChannelSubscriptionRequest`
- Response: `ChannelSubscriptionResponse`
- REST mapping: `GET /v1/ns/{ns}/channels/{channel}/subscriptions/{name}`

### `ModifyChannelSubscription`

- Request: `ModifyChannelSubscriptionRequest`
- Response: `ChannelSubscriptionResponse`
- REST mapping: `PUT /v1/ns/{ns}/channels/{channel}/subscriptions/{name}`
- REST body: `*`

### `ListChannelSubscriptions`

- Request: `ListChannelSubscriptionsRequest`
- Response: `ListChannelSubscriptionsResponse`
- REST mapping: `GET /v1/ns/{ns}/channels/{channel}/subscriptions`

### `DeleteChannelSubscription`

- Request: `DeleteChannelSubscriptionRequest`
- Response: `DeleteChannelSubscriptionResponse`
- REST mapping: `DELETE /v1/ns/{ns}/channels/{channel}/subscriptions/{name}`

### `StreamChannelEvents`

- Request: `StreamChannelEventsRequest`
- Response: `talon.events.ChannelEvent` (server stream)
- REST mapping: `GET /v1/ns/{ns}/channels/{channel}/stream`

### `CreateWorkflow`

Workflows

- Request: `CreateWorkflowRequest`
- Response: `WorkflowResponse`
- REST mapping: `POST /v1/ns/{ns}/workflows`
- REST body: `*`

### `GetWorkflow`

- Request: `GetWorkflowRequest`
- Response: `WorkflowResponse`
- REST mapping: `GET /v1/ns/{ns}/workflows/{name}`

### `ListWorkflows`

- Request: `ListWorkflowsRequest`
- Response: `ListWorkflowsResponse`
- REST mapping: `GET /v1/ns/{ns}/workflows`

### `DeleteWorkflow`

- Request: `DeleteWorkflowRequest`
- Response: `DeleteWorkflowResponse`
- REST mapping: `DELETE /v1/ns/{ns}/workflows/{name}`

### `CreateWorkflowRun`

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

Generic v2 resources

- Request: `CreateResourceRequest`
- Response: `ResourceResponse`
- REST mapping: `POST /v2/ns/{ns}/resources`
- REST body: `*`

### `GetResource`

- Request: `GetResourceRequest`
- Response: `ResourceResponse`
- REST mapping: `GET /v2/ns/{ns}/resources/{kind}/{name}`

### `ListResources`

- Request: `ListResourcesRequest`
- Response: `ListResourcesResponse`
- REST mapping: `GET /v2/ns/{ns}/resources`

### `DeleteResource`

- Request: `DeleteResourceRequest`
- Response: `DeleteResourceResponse`
- REST mapping: `DELETE /v2/ns/{ns}/resources/{kind}/{name}`
