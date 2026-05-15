---
title: Gateway Service
sidebar_position: 2
---

The Talon gateway is defined in `proto/gateway.proto`. It is the canonical contract for both gRPC and the REST-transcoded HTTP surface exposed through the gateway and Envoy.

## Surface summary

- Service: `talon.gateway.GatewayService`
- Transport modes: gRPC, gRPC-web, REST via `google.api.http` annotations, and the browser-oriented `/v1/ui/... ` stream path documented separately in the hand-written guides
- Total RPC methods: **38**

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

### `ListSessions`

- Request: `ListSessionsRequest`
- Response: `ListSessionsResponse`
- REST mapping: `GET /v1/ns/{ns}/agents/{agent}/sessions`

### `DeleteSession`

- Request: `DeleteSessionRequest`
- Response: `DeleteSessionResponse`
- REST mapping: `DELETE /v1/ns/{ns}/agents/{agent}/sessions/{session_id}`

### `SendMessage`

- Request: `SendMessageRequest`
- Response: `SendMessageResponse`
- REST mapping: `POST /v1/ns/{ns}/agents/{agent}/sessions/{session_id}/message`
- REST body: `*`

### `StopSessionGeneration`

- Request: `StopSessionGenerationRequest`
- Response: `StopSessionGenerationResponse`
- REST mapping: `POST /v1/ns/{ns}/agents/{agent}/sessions/{session_id}:stop`
- REST body: `*`

### `StreamSessionSteps`

- Request: `StreamSessionStepsRequest`
- Response: `talon.events.SessionStepEvent` (server stream)
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

## Templates

### `CreateAgentTemplate`

- Request: `CreateAgentTemplateRequest`
- Response: `AgentTemplateResponse`
- REST mapping: `POST /v1/templates`
- REST body: `*`

### `GetAgentTemplate`

- Request: `GetAgentTemplateRequest`
- Response: `AgentTemplateResponse`
- REST mapping: `GET /v1/templates/{name}`

### `ListAgentTemplates`

- Request: `ListAgentTemplatesRequest`
- Response: `ListAgentTemplatesResponse`
- REST mapping: `GET /v1/templates`

### `DeleteAgentTemplate`

- Request: `DeleteAgentTemplateRequest`
- Response: `DeleteAgentTemplateResponse`
- REST mapping: `DELETE /v1/templates/{name}`

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
