---
title: Manifest Schema
---

This page summarizes the manifest types that drive Talon agents, templates, MCP servers, bindings, and knowledge resources.

## `ObjectMeta`

Metadata for finding and grouping manifests

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `namespace` | `string` | - |
| `labels` | `map<string, string>` | - |
| `annotations` | `map<string, string>` | - |

## `AgentTemplate`

----------------------------------------- Agent Template -----------------------------------------

| Field | Type | Notes |
| --- | --- | --- |
| `api_version` | `string` | - |
| `kind` | `string` | - |
| `metadata` | `ObjectMeta` | - |
| `definition` | `AgentDefinition` | - |

## `AgentSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `features` | `Feature` | repeated |
| `model_policy` | `ModelPolicy` | - |
| `system_prompt` | `string` | - |
| `mcp_server_refs` | `string` | repeated |
| `capabilities` | `map<string, google.protobuf.ListValue>` | - |

## `AgentDefinition`

| Field | Type | Notes |
| --- | --- | --- |
| `custom_spec` | `AgentSpec` | oneof (source) |
| `templated` | `TemplatedAgentSpec` | oneof (source) |

## `TemplatedAgentSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `template_name` | `string` | - |
| `delta` | `AgentSpecDelta` | - |

## `AgentSpecDelta`

| Field | Type | Notes |
| --- | --- | --- |
| `model_policy` | `ModelPolicyDelta` | - |
| `system_prompt` | `PromptDelta` | - |
| `features` | `FeatureSetDelta` | - |
| `mcp_server_refs` | `StringListDelta` | - |
| `capabilities` | `CapabilitiesPolicyDelta` | - |

## `PromptDelta`

| Field | Type | Notes |
| --- | --- | --- |
| `replace` | `string` | oneof (operation) |
| `prepend` | `string` | oneof (operation) |
| `append` | `string` | oneof (operation) |

## `FeatureSetDelta`

| Field | Type | Notes |
| --- | --- | --- |
| `upsert` | `Feature` | repeated |
| `remove` | `string` | repeated |

## `StringListDelta`

| Field | Type | Notes |
| --- | --- | --- |
| `replace` | `string` | repeated |
| `add` | `string` | repeated |
| `remove` | `string` | repeated |

## `CapabilitiesPolicyDelta`

| Field | Type | Notes |
| --- | --- | --- |
| `replace` | `map<string, google.protobuf.ListValue>` | - |

## `Feature`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `type` | `string` | - |
| `required` | `bool` | - |

## `Model`

| Field | Type | Notes |
| --- | --- | --- |
| `provider` | `string` | - |
| `name` | `string` | - |
| `temperature` | `float` | - |
| `thinking` | `ThinkingConfig` | - |

## `ThinkingConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `enabled` | `bool` | - |
| `budget_tokens` | `uint32` | optional |
| `effort` | `string` | - |

## `ModelProfile`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `model` | `Model` | - |

## `ModelPolicy`

| Field | Type | Notes |
| --- | --- | --- |
| `profiles` | `ModelProfile` | repeated |

## `ModelPolicyDelta`

| Field | Type | Notes |
| --- | --- | --- |
| `upsert` | `ModelProfile` | repeated |

## `McpServer`

| Field | Type | Notes |
| --- | --- | --- |
| `api_version` | `string` | - |
| `kind` | `string` | - |
| `metadata` | `ObjectMeta` | - |
| `spec` | `McpServerSpec` | - |

## `McpServerSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `transport` | `string` | - |
| `target` | `string` | - |
| `args` | `string` | repeated |
| `headers` | `map<string, string>` | - |
| `disabled` | `bool` | - |

## `McpServerBinding`

| Field | Type | Notes |
| --- | --- | --- |
| `api_version` | `string` | - |
| `kind` | `string` | - |
| `metadata` | `ObjectMeta` | - |
| `spec` | `McpServerBindingSpec` | - |

## `McpServerBindingSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `server_ref` | `string` | - |
| `args` | `string` | repeated |
| `headers` | `map<string, string>` | - |
| `disabled` | `bool` | - |
| `auth_broker` | `McpAuthBrokerSpec` | - |
| `allowed_tool_names` | `string` | repeated |

## `McpAuthBrokerSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `kind` | `string` | - |
| `url` | `string` | - |
| `cache_ttl_seconds` | `int32` | - |
| `audience` | `string` | - |

## `Knowledge`

| Field | Type | Notes |
| --- | --- | --- |
| `api_version` | `string` | - |
| `kind` | `string` | - |
| `metadata` | `ObjectMeta` | - |
| `spec` | `KnowledgeSpec` | - |

## `KnowledgeSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `path` | `string` | - |
| `content` | `string` | - |

## `MemoryProvider`

----------------------------------------- Memory Provider -----------------------------------------

| Field | Type | Notes |
| --- | --- | --- |
| `api_version` | `string` | - |
| `kind` | `string` | - |
| `metadata` | `ObjectMeta` | - |
| `spec` | `MemoryProviderSpec` | - |

## `MemoryProviderSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `driver` | `string` | - |
| `connection` | `ConnectionConfig` | - |
| `schema` | `SchemaConfig` | - |

## `ConnectionConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `url` | `EnvSecret` | - |
| `pool` | `PoolConfig` | - |

## `EnvSecret`

| Field | Type | Notes |
| --- | --- | --- |
| `source` | `string` | - |
| `key` | `string` | - |

## `PoolConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `max_connections` | `int32` | - |
| `min_connections` | `int32` | - |
| `idle_timeout_seconds` | `int32` | - |

## `SchemaConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
