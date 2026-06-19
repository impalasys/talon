---
title: Resource Schemas
---

This page summarizes the control-plane resource messages that drive Talon agents, deployments, sandbox orchestration, MCP servers, bindings, schedules, workflows, and knowledge resources.

## `OwnerReference`

| Field | Type | Notes |
| --- | --- | --- |
| `api_version` | `string` | - |
| `kind` | `string` | - |
| `namespace` | `string` | - |
| `name` | `string` | - |
| `uid` | `string` | - |
| `controller` | `bool` | - |
| `block_owner_deletion` | `bool` | - |

## `ResourceMeta`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `namespace` | `string` | - |
| `labels` | `map<string, string>` | - |
| `annotations` | `map<string, string>` | - |
| `owner_references` | `OwnerReference` | repeated |
| `finalizers` | `string` | repeated |
| `generation` | `uint64` | - |
| `resource_version` | `string` | - |
| `uid` | `string` | - |
| `deletion_timestamp` | `int64` | optional |

## `ResourceCondition`

| Field | Type | Notes |
| --- | --- | --- |
| `type` | `string` | - |
| `status` | `string` | - |
| `reason` | `string` | - |
| `message` | `string` | - |
| `last_transition_time` | `int64` | - |
| `observed_generation` | `uint64` | - |

## `CommonResourceStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |

## `ResourceRef`

| Field | Type | Notes |
| --- | --- | --- |
| `namespace` | `string` | - |
| `name` | `string` | - |

## `NamespaceSelector`

| Field | Type | Notes |
| --- | --- | --- |
| `parent` | `string` | - |
| `match_labels` | `map<string, string>` | - |

## `Agent`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `ns` | `string` | - |
| `spec` | `AgentSpec` | - |
| `labels` | `map<string, string>` | - |

## `AgentSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `features` | `Feature` | repeated |
| `model_policy` | `ModelPolicy` | - |
| `system_prompt` | `string` | - |
| `mcp_server_refs` | `string` | repeated |
| `capabilities` | `map<string, google.protobuf.ListValue>` | - |
| `a2a` | `A2A` | - |
| `runtime` | `AgentRuntime` | - |

## `AgentStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |
| `last_session_id` | `string` | optional |

## `AgentRuntime`

| Field | Type | Notes |
| --- | --- | --- |
| `kind` | `string` | - |
| `acp` | `AcpRuntime` | - |

## `AcpRuntime`

| Field | Type | Notes |
| --- | --- | --- |
| `harness_ref` | `string` | - |
| `command` | `string` | - |
| `args` | `string` | repeated |
| `cwd` | `string` | - |
| `sandbox_policy_ref` | `string` | - |
| `persist_session` | `bool` | - |
| `env` | `map<string, string>` | - |
| `permission_policy` | `map<string, string>` | - |

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

## `A2A`

| Field | Type | Notes |
| --- | --- | --- |
| `connections` | `Connection` | repeated |
| `agent_card` | `AgentCard` | - |

## `Connection`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `description` | `string` | - |
| `target` | `ConnectionRef` | - |
| `input_modes` | `string` | repeated |
| `output_modes` | `string` | repeated |
| `timeout_seconds` | `uint32` | - |
| `max_depth` | `uint32` | - |
| `auth` | `ConnectionAuth` | - |

## `ConnectionRef`

| Field | Type | Notes |
| --- | --- | --- |
| `internal` | `InternalConnectionRef` | oneof (target) |
| `external` | `ExternalConnectionRef` | oneof (target) |

## `InternalConnectionRef`

| Field | Type | Notes |
| --- | --- | --- |
| `namespace` | `string` | - |
| `agent` | `string` | - |

## `ExternalConnectionRef`

| Field | Type | Notes |
| --- | --- | --- |
| `agent_card_url` | `string` | - |

## `ConnectionAuth`

| Field | Type | Notes |
| --- | --- | --- |
| `kind` | `string` | - |
| `secret_ref` | `string` | - |

## `AgentCard`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `description` | `string` | - |
| `version` | `string` | - |
| `capabilities` | `AgentCardCapabilities` | - |
| `default_input_modes` | `string` | repeated |
| `default_output_modes` | `string` | repeated |
| `skills` | `AgentCardSkill` | repeated |

## `AgentCardCapabilities`

| Field | Type | Notes |
| --- | --- | --- |
| `streaming` | `bool` | - |
| `push_notifications` | `bool` | - |
| `extended_agent_card` | `bool` | - |

## `AgentCardSkill`

| Field | Type | Notes |
| --- | --- | --- |
| `id` | `string` | - |
| `name` | `string` | - |
| `description` | `string` | - |
| `tags` | `string` | repeated |
| `examples` | `string` | repeated |
| `input_modes` | `string` | repeated |
| `output_modes` | `string` | repeated |

## `McpServer`

| Field | Type | Notes |
| --- | --- | --- |
| `api_version` | `string` | - |
| `kind` | `string` | - |
| `metadata` | `ResourceMeta` | - |
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
| `metadata` | `ResourceMeta` | - |
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
| `metadata` | `ResourceMeta` | - |
| `spec` | `KnowledgeSpec` | - |

## `KnowledgeSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `path` | `string` | - |
| `content` | `string` | - |

## `Namespace`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `parent` | `string` | - |
| `is_deleted` | `bool` | - |
| `deleted_at` | `int64` | - |
| `labels` | `map<string, string>` | - |

## `NamespaceSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `parent` | `string` | - |

## `NamespaceStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |
| `is_deleted` | `bool` | - |
| `deleted_at` | `int64` | - |

## `Channel`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `ns` | `string` | - |
| `title` | `string` | - |
| `status` | `string` | - |
| `created_at` | `int64` | - |
| `updated_at` | `int64` | - |
| `metadata` | `map<string, string>` | - |
| `labels` | `map<string, string>` | - |

## `ChannelSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `title` | `string` | - |
| `metadata` | `map<string, string>` | - |

## `ChannelStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |

## `ChannelContextPolicy`

| Field | Type | Notes |
| --- | --- | --- |
| `mode` | `string` | - |
| `max_messages` | `uint32` | - |

## `ChannelSubscription`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `ns` | `string` | - |
| `channel` | `string` | - |
| `agent` | `string` | - |
| `enabled` | `bool` | - |
| `trigger` | `string` | - |
| `context_policy` | `ChannelContextPolicy` | - |
| `metadata` | `map<string, string>` | - |
| `labels` | `map<string, string>` | - |
| `reply_mode` | `string` | - |

## `ChannelSubscriptionSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `channel` | `string` | - |
| `agent` | `string` | - |
| `enabled` | `bool` | - |
| `trigger` | `string` | - |
| `context_policy` | `ChannelContextPolicy` | - |
| `reply_mode` | `string` | - |

## `ScheduleTarget`

| Field | Type | Notes |
| --- | --- | --- |
| `agent` | `string` | - |
| `session_mode` | `string` | - |
| `session_id` | `string` | - |
| `workflow` | `string` | - |

## `ScheduleSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `kind` | `string` | - |
| `cron` | `string` | - |
| `interval_seconds` | `uint32` | - |
| `run_at` | `string` | - |
| `timezone` | `string` | - |
| `target` | `ScheduleTarget` | - |
| `input_message` | `string` | - |
| `enabled` | `bool` | - |
| `input_json` | `string` | - |

## `ScheduleStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `revision` | `uint64` | - |
| `next_run_at` | `int64` | optional |
| `backend_handle` | `string` | optional |
| `backend_armed` | `bool` | - |
| `last_run_at` | `int64` | optional |
| `last_session_id` | `string` | optional |
| `last_error` | `string` | optional |
| `claimed_run_at` | `int64` | optional |
| `claim_expires_at` | `int64` | optional |
| `recent_events` | `ScheduleEvent` | repeated |

## `ScheduleEvent`

| Field | Type | Notes |
| --- | --- | --- |
| `timestamp` | `int64` | - |
| `phase` | `string` | - |
| `outcome` | `string` | - |
| `detail` | `string` | - |

## `Schedule`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `ns` | `string` | - |
| `spec` | `ScheduleSpec` | - |
| `status` | `ScheduleStatus` | - |
| `labels` | `map<string, string>` | - |

## `WorkflowStepOutputPolicy`

| Field | Type | Notes |
| --- | --- | --- |
| `format` | `string` | - |
| `schema_json` | `string` | - |

## `WorkflowStepRetryPolicy`

| Field | Type | Notes |
| --- | --- | --- |
| `max_attempts` | `uint32` | - |
| `initial_backoff_seconds` | `int64` | - |
| `max_backoff_seconds` | `int64` | - |
| `multiplier` | `double` | - |

## `WorkflowStep`

| Field | Type | Notes |
| --- | --- | --- |
| `id` | `string` | - |
| `type` | `string` | - |
| `after` | `string` | repeated |
| `when_json` | `string` | - |
| `agent` | `string` | - |
| `prompt` | `string` | - |
| `tool` | `string` | - |
| `input_json` | `string` | - |
| `workflow` | `string` | - |
| `output` | `WorkflowStepOutputPolicy` | - |
| `resume_schema_json` | `string` | - |
| `retry` | `WorkflowStepRetryPolicy` | - |
| `timeout` | `string` | - |
| `wait_duration` | `string` | - |
| `wait_until` | `string` | - |

## `WorkflowSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `description` | `string` | - |
| `input_schema_json` | `string` | - |
| `output_schema_json` | `string` | - |
| `steps` | `WorkflowStep` | repeated |
| `output_json` | `string` | - |
| `concurrency` | `uint32` | - |

## `Workflow`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `ns` | `string` | - |
| `spec` | `WorkflowSpec` | - |
| `labels` | `map<string, string>` | - |

## `WorkflowStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |

## `TemplateSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `kind` | `string` | - |
| `metadata` | `ResourceMeta` | - |
| `spec_json` | `string` | - |

## `DeploymentPlacement`

| Field | Type | Notes |
| --- | --- | --- |
| `namespace_selector` | `NamespaceSelector` | - |

## `DeploymentSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `placement` | `DeploymentPlacement` | - |
| `templates` | `string` | repeated |

## `DeploymentReplicaSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `deployment_ref` | `ResourceRef` | - |
| `target_namespace` | `string` | - |

## `DeploymentStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |
| `replicas` | `ResourceRef` | repeated |

## `DeploymentReplicaStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `rendered_resources` | `string` | repeated |
| `rendered_hashes` | `map<string, string>` | - |
| `conflicts` | `string` | repeated |
| `last_rendered_json` | `map<string, string>` | - |
| `owned_json_pointers` | `string` | repeated |
| `phase` | `string` | - |

## `SandboxClassSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `provider` | `string` | - |
| `provider_config_json` | `string` | - |
| `credentials_json` | `string` | - |

## `SandboxWorkspaceSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `mode` | `string` | - |
| `mount_path` | `string` | - |

## `SandboxSetupSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `packages` | `string` | repeated |

## `SandboxNetworkSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `mode` | `string` | - |

## `SandboxFilesystemSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `writable` | `string` | repeated |
| `readonly` | `string` | repeated |

## `SandboxLeasePolicySpec`

| Field | Type | Notes |
| --- | --- | --- |
| `mode` | `string` | - |

## `SandboxRuntimeTemplateSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `image` | `string` | - |
| `workspace` | `SandboxWorkspaceSpec` | - |
| `setup` | `SandboxSetupSpec` | - |
| `network` | `SandboxNetworkSpec` | - |
| `filesystem` | `SandboxFilesystemSpec` | - |
| `lease_policy` | `SandboxLeasePolicySpec` | - |

## `SandboxPolicySpec`

| Field | Type | Notes |
| --- | --- | --- |
| `class_ref` | `ResourceRef` | - |
| `template` | `SandboxRuntimeTemplateSpec` | - |
| `max_concurrent` | `uint32` | - |

## `SandboxLease`

| Field | Type | Notes |
| --- | --- | --- |
| `owner_kind` | `string` | - |
| `owner_agent` | `string` | - |
| `owner_session_id` | `string` | - |
| `token` | `string` | - |
| `acquired_at` | `int64` | - |
| `expires_at` | `int64` | - |
| `heartbeat_at` | `int64` | - |

## `SandboxProcessStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `id` | `string` | - |
| `command` | `string` | - |
| `args` | `string` | repeated |
| `protocol` | `string` | - |
| `phase` | `string` | - |

## `SandboxStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |
| `backend_id` | `string` | - |
| `lease` | `SandboxLease` | - |
| `processes` | `SandboxProcessStatus` | repeated |

## `SandboxSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `policy_ref` | `string` | - |
| `class_ref` | `ResourceRef` | - |
| `runtime_template_json` | `string` | - |

## `SessionSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `agent` | `string` | - |
| `labels` | `map<string, string>` | - |

## `SessionStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |
| `created_at` | `int64` | - |
| `last_active` | `int64` | - |
| `acp_session_id` | `string` | - |
| `sandbox_ref` | `string` | - |

## `PermissionRequestSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `agent` | `string` | - |
| `session_id` | `string` | - |
| `action` | `string` | - |
| `prompt` | `string` | - |
| `payload_json` | `string` | - |

## `PermissionRequestStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |
| `decision` | `string` | - |
| `decided_by` | `string` | - |
| `decided_at` | `int64` | - |

## `Resource`

| Field | Type | Notes |
| --- | --- | --- |
| `api_version` | `string` | - |
| `kind` | `string` | - |
| `metadata` | `ResourceMeta` | - |
| `spec` | `ResourceSpec` | - |
| `status` | `ResourceStatus` | - |

## `RawResourceSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `json` | `string` | - |

## `RawResourceStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `json` | `string` | - |

## `ResourceSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `agent` | `AgentSpec` | oneof (kind) |
| `workflow` | `WorkflowSpec` | oneof (kind) |
| `schedule` | `ScheduleSpec` | oneof (kind) |
| `channel` | `ChannelSpec` | oneof (kind) |
| `channel_subscription` | `ChannelSubscriptionSpec` | oneof (kind) |
| `mcp_server` | `McpServerSpec` | oneof (kind) |
| `mcp_server_binding` | `McpServerBindingSpec` | oneof (kind) |
| `knowledge` | `KnowledgeSpec` | oneof (kind) |
| `namespace` | `NamespaceSpec` | oneof (kind) |
| `session` | `SessionSpec` | oneof (kind) |
| `template` | `TemplateSpec` | oneof (kind) |
| `deployment` | `DeploymentSpec` | oneof (kind) |
| `deployment_replica` | `DeploymentReplicaSpec` | oneof (kind) |
| `sandbox_class` | `SandboxClassSpec` | oneof (kind) |
| `sandbox_policy` | `SandboxPolicySpec` | oneof (kind) |
| `sandbox` | `SandboxSpec` | oneof (kind) |
| `permission_request` | `PermissionRequestSpec` | oneof (kind) |
| `raw` | `RawResourceSpec` | oneof (kind) |

## `ResourceStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `agent` | `AgentStatus` | oneof (kind) |
| `workflow` | `WorkflowStatus` | oneof (kind) |
| `schedule` | `ScheduleStatus` | oneof (kind) |
| `channel` | `ChannelStatus` | oneof (kind) |
| `channel_subscription` | `CommonResourceStatus` | oneof (kind) |
| `mcp_server` | `CommonResourceStatus` | oneof (kind) |
| `mcp_server_binding` | `CommonResourceStatus` | oneof (kind) |
| `knowledge` | `CommonResourceStatus` | oneof (kind) |
| `namespace` | `NamespaceStatus` | oneof (kind) |
| `session` | `SessionStatus` | oneof (kind) |
| `template` | `CommonResourceStatus` | oneof (kind) |
| `deployment` | `DeploymentStatus` | oneof (kind) |
| `deployment_replica` | `DeploymentReplicaStatus` | oneof (kind) |
| `sandbox_class` | `CommonResourceStatus` | oneof (kind) |
| `sandbox_policy` | `CommonResourceStatus` | oneof (kind) |
| `sandbox` | `SandboxStatus` | oneof (kind) |
| `permission_request` | `PermissionRequestStatus` | oneof (kind) |
| `raw` | `RawResourceStatus` | oneof (kind) |
