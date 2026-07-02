---
title: Resource Schemas
---

This page summarizes the control-plane resource messages that drive Talon agents, deployments, sandbox orchestration, MCP servers, schedules, workflows, and knowledge resources.

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
| `last_transition_time` | `int64` | Unix timestamp in microseconds. |
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
| `metadata` | `ResourceMeta` | - |
| `spec` | `AgentSpec` | - |
| `status` | `AgentStatus` | - |

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
| `sandbox_policy_ref` | `string` | SandboxPolicy name resolved in the agent namespace, then namespace ancestry. |
| `persist_session` | `bool` | - |
| `env` | `map<string, string>` | - |
| `permission_policy` | `map<string, string>` | Keys: default, filesystemRead, filesystemWrite, terminal. Values: allow, ask, deny. |

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
| `internal` | `InternalConnectionRef` | optional; Internal Talon agent target. Mutually exclusive with external. |
| `external` | `ExternalConnectionRef` | optional; External A2A agent-card target. Mutually exclusive with internal. |

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
| `metadata` | `ResourceMeta` | - |
| `spec` | `McpServerSpec` | - |
| `status` | `CommonResourceStatus` | - |

## `McpServerSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `transport` | `string` | - |
| `target` | `string` | - |
| `args` | `string` | repeated |
| `headers` | `map<string, string>` | - |
| `disabled` | `bool` | - |
| `auth_broker` | `McpAuthBrokerSpec` | - |
| `policy` | `McpServerPolicy` | - |

## `McpAuthBrokerSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `kind` | `string` | - |
| `url` | `string` | - |
| `cache_ttl_seconds` | `int32` | - |
| `audience` | `string` | - |

## `McpServerPolicy`

| Field | Type | Notes |
| --- | --- | --- |
| `tools` | `McpToolPolicy` | - |

## `McpToolPolicy`

| Field | Type | Notes |
| --- | --- | --- |
| `allowlist` | `string` | repeated |

## `Knowledge`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | - |
| `spec` | `KnowledgeSpec` | - |
| `status` | `CommonResourceStatus` | - |

## `KnowledgeSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `path` | `string` | - |
| `content` | `string` | - |

## `Namespace`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | - |
| `spec` | `NamespaceSpec` | - |
| `status` | `NamespaceStatus` | - |

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
| `metadata` | `ResourceMeta` | - |
| `spec` | `ChannelSpec` | - |
| `status` | `ChannelStatus` | - |

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
| `created_at` | `int64` | - |
| `updated_at` | `int64` | - |

## `ChannelContextPolicy`

| Field | Type | Notes |
| --- | --- | --- |
| `mode` | `string` | - |
| `max_messages` | `uint32` | - |

## `ChannelSubscription`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | - |
| `spec` | `ChannelSubscriptionSpec` | - |
| `status` | `CommonResourceStatus` | - |

## `ChannelSubscriptionSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `channel` | `string` | - |
| `agent` | `string` | - |
| `enabled` | `bool` | - |
| `trigger` | `string` | - |
| `context_policy` | `ChannelContextPolicy` | - |
| `reply_mode` | `string` | - |
| `metadata` | `map<string, string>` | - |

## `ConnectorClassRuntimeSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `kind` | `string` | Runtime implementation type, for example an HTTP connector service. |
| `endpoint` | `string` | Base URL for the connector service that implements the Talon connector protocol for this class. |

## `ConnectorClassAuthSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `kind` | `string` | Authentication scheme Talon uses when calling the connector service. |
| `api_key` | `ConnectorSecretRef` | API key source used to authenticate this Talon cluster to the connector service. |

## `ConnectorSecretRef`

| Field | Type | Notes |
| --- | --- | --- |
| `plain` | `string` | optional; Inline secret value. Mutually exclusive with env. |
| `env` | `string` | optional; Environment variable name that contains the secret value. Mutually exclusive with plain. |

## `ConnectorMatchIndex`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | Connector-service-defined index name, stable within the ConnectorClass. |
| `fields` | `string` | repeated; Match field names, in priority order, used to compile routing keys. |

## `ConnectorClassSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `platform` | `string` | External messaging platform implemented by this class, such as slack or imessage. |
| `runtime` | `ConnectorClassRuntimeSpec` | How Talon reaches the connector service. |
| `auth` | `ConnectorClassAuthSpec` | How Talon authenticates requests to the connector service. |
| `match_indexes` | `ConnectorMatchIndex` | repeated; Provider-specific match indexes supported by this connector service. |

## `ConnectorClassStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | Resource generation last reconciled by the ConnectorController. |
| `phase` | `string` | Current registration phase for this class, such as pending, ready, or degraded. |
| `conditions` | `ResourceCondition` | repeated; Detailed readiness and error conditions from registration with the connector service. |

## `ConnectorSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `class_ref` | `ResourceRef` | ConnectorClass that owns the platform adapter and match index definitions. If namespace is empty, Talon resolves the class in the Connector's namespace. In v1, a non-empty namespace must match the Connector namespace; cross-namespace class references require a future policy/RBAC gate. |
| `enabled` | `bool` | Disabled Connectors are not indexed for incoming message routing. |
| `match_fields` | `map<string, string>` | Provider-specific route fields, such as Slack team/channel IDs or an iMessage profile identifier. Talon treats these as opaque keys described by the ConnectorClass match indexes. |
| `consumer` | `talon.data.MessageConsumer` | Single Talon message consumer for messages that match this Connector. |

## `ConnectorStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | Resource generation last reconciled by the ConnectorController. |
| `phase` | `string` | Current route indexing phase for this Connector, such as ready or invalid. |
| `conditions` | `ResourceCondition` | repeated; Detailed route-indexing readiness and validation conditions. |
| `compiled_route_ids` | `string` | repeated; Materialized route IDs generated from match_fields and the owning ConnectorClass match indexes. |

## `ConnectorClass`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | Standard namespaced resource metadata. ConnectorClasses are regular namespace resources so each tenant or operator namespace can define its own trusted connector services. |
| `spec` | `ConnectorClassSpec` | Desired connector service registration and platform capabilities. |
| `status` | `ConnectorClassStatus` | Observed registration state for this connector service class. |

## `Connector`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | Standard namespaced resource metadata. Each Connector is owned by the namespace whose messages it routes into Talon. |
| `spec` | `ConnectorSpec` | Desired provider match and Talon message consumer for one route. |
| `status` | `ConnectorStatus` | Observed route-indexing state for this Connector. |

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
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |
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
| `metadata` | `ResourceMeta` | - |
| `spec` | `ScheduleSpec` | - |
| `status` | `ScheduleStatus` | - |

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
| `metadata` | `ResourceMeta` | - |
| `spec` | `WorkflowSpec` | - |
| `status` | `WorkflowStatus` | - |

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
| `spec_json` | `string` | Internal canonical JSON for the templated spec. User-facing YAML uses `spec: {...}` and the manifest parser normalizes it into this field. |

## `Template`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | - |
| `spec` | `TemplateSpec` | - |
| `status` | `CommonResourceStatus` | - |

## `DeploymentPlacement`

| Field | Type | Notes |
| --- | --- | --- |
| `namespace_selector` | `NamespaceSelector` | - |

## `DeploymentSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `placement` | `DeploymentPlacement` | - |
| `templates` | `string` | repeated |

## `Deployment`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | - |
| `spec` | `DeploymentSpec` | - |
| `status` | `DeploymentStatus` | - |

## `DeploymentReplicaSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `deployment_ref` | `ResourceRef` | - |
| `target_namespace` | `string` | - |

## `DeploymentReplica`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | - |
| `spec` | `DeploymentReplicaSpec` | - |
| `status` | `DeploymentReplicaStatus` | - |

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
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |
| `rendered_resources` | `string` | repeated |
| `rendered_hashes` | `map<string, string>` | - |
| `conflicts` | `string` | repeated |
| `last_rendered_json` | `map<string, string>` | - |
| `owned_json_pointers` | `string` | repeated |

## `SandboxClassSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `provider` | `string` | - |
| `provider_config_json` | `string` | Internal canonical JSON for provider-specific settings. User-facing YAML uses `providerConfig: {...}` and the manifest parser normalizes it here. |
| `credentials_json` | `string` | Internal canonical JSON for provider credentials. User-facing YAML uses `credentials: {...}` and the manifest parser normalizes it here. |

## `SandboxClass`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | - |
| `spec` | `SandboxClassSpec` | - |
| `status` | `CommonResourceStatus` | - |

## `SandboxWorkspaceSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `mode` | `string` | - |
| `mount_path` | `string` | Absolute workspace path inside the sandbox. The manifest parser rejects root/system mount points such as /, /etc, /usr, /proc, and /sys. |

## `SandboxSetupSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `packages` | `string` | repeated |
| `commands` | `string` | repeated |

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

## `SandboxPolicy`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | - |
| `spec` | `SandboxPolicySpec` | - |
| `status` | `CommonResourceStatus` | - |

## `SandboxLease`

| Field | Type | Notes |
| --- | --- | --- |
| `owner_kind` | `string` | - |
| `owner_agent` | `string` | - |
| `owner_session_id` | `string` | - |
| `token` | `string` | - |
| `acquired_at` | `int64` | Unix timestamp in microseconds. |
| `expires_at` | `int64` | Unix timestamp in microseconds. |
| `heartbeat_at` | `int64` | Unix timestamp in microseconds. |

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
| `policy_ref` | `string` | SandboxPolicy name resolved in the sandbox namespace. |
| `class_ref` | `ResourceRef` | - |
| `runtime_template` | `SandboxRuntimeTemplateSpec` | - |

## `Sandbox`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | - |
| `spec` | `SandboxSpec` | - |
| `status` | `SandboxStatus` | - |

## `SessionSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `agent` | `string` | - |
| `labels` | `map<string, string>` | - |

## `Session`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | - |
| `spec` | `SessionSpec` | - |
| `status` | `SessionStatus` | - |

## `SessionStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |
| `created_at` | `int64` | - |
| `last_active` | `int64` | - |
| `acp_session_id` | `string` | - |
| `sandbox_ref` | `string` | Sandbox resource name in the same namespace as this Session. |

## `SkillSpec`

| Field | Type | Notes |
| --- | --- | --- |
| `description` | `string` | - |
| `instructions` | `string` | - |

## `Skill`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | - |
| `spec` | `SkillSpec` | - |
| `status` | `CommonResourceStatus` | - |

## `UsageSelector`

| Field | Type | Notes |
| --- | --- | --- |
| `agent` | `string` | - |
| `provider` | `string` | - |
| `model` | `string` | - |

## `UsageLimit`

| Field | Type | Notes |
| --- | --- | --- |
| `selector` | `UsageSelector` | - |
| `metric` | `string` | - |
| `max` | `uint64` | - |
| `window` | `string` | - |

## `UsagePolicySpec`

| Field | Type | Notes |
| --- | --- | --- |
| `namespace_scope` | `string` | - |
| `hard` | `UsageLimit` | repeated |

## `UsageLimitStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `selector` | `UsageSelector` | - |
| `metric` | `string` | - |
| `max` | `uint64` | - |
| `window` | `string` | - |
| `window_start` | `int64` | - |
| `reset_at` | `int64` | - |
| `used` | `uint64` | - |
| `remaining` | `uint64` | - |
| `exceeded` | `bool` | - |

## `UsagePolicyStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |
| `hard` | `UsageLimitStatus` | repeated |

## `UsagePolicy`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | - |
| `spec` | `UsagePolicySpec` | - |
| `status` | `UsagePolicyStatus` | - |

## `WorkerEndpoint`

| Field | Type | Notes |
| --- | --- | --- |
| `url` | `string` | - |
| `protocol` | `string` | - |
| `audience` | `string` | - |

## `WorkerStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `observed_generation` | `uint64` | - |
| `phase` | `string` | - |
| `conditions` | `ResourceCondition` | repeated |
| `started_at` | `int64` | Unix timestamp in microseconds. |
| `heartbeat_at` | `int64` | Unix timestamp in microseconds. |
| `expires_at` | `int64` | Unix timestamp in microseconds. |
| `version` | `string` | - |
| `endpoints` | `WorkerEndpoint` | repeated |

## `Worker`

| Field | Type | Notes |
| --- | --- | --- |
| `metadata` | `ResourceMeta` | - |
| `spec` | `WorkerSpec` | - |
| `status` | `WorkerStatus` | - |

## `Resource`

| Field | Type | Notes |
| --- | --- | --- |
| `api_version` | `string` | - |
| `kind` | `string` | - |
| `metadata` | `ResourceMeta` | - |
| `spec` | `ResourceSpec` | - |
| `status` | `ResourceStatus` | - |

## `ResourceManifest`

| Field | Type | Notes |
| --- | --- | --- |
| `api_version` | `string` | - |
| `kind` | `string` | - |
| `metadata` | `ResourceMeta` | - |
| `spec` | `ResourceSpec` | - |

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
| `connector_class` | `ConnectorClassSpec` | oneof (kind) |
| `connector` | `ConnectorSpec` | oneof (kind) |
| `mcp_server` | `McpServerSpec` | oneof (kind) |
| `knowledge` | `KnowledgeSpec` | oneof (kind) |
| `namespace` | `NamespaceSpec` | oneof (kind) |
| `session` | `SessionSpec` | oneof (kind) |
| `skill` | `SkillSpec` | oneof (kind) |
| `template` | `TemplateSpec` | oneof (kind) |
| `deployment` | `DeploymentSpec` | oneof (kind) |
| `deployment_replica` | `DeploymentReplicaSpec` | oneof (kind) |
| `sandbox_class` | `SandboxClassSpec` | oneof (kind) |
| `sandbox_policy` | `SandboxPolicySpec` | oneof (kind) |
| `sandbox` | `SandboxSpec` | oneof (kind) |
| `worker` | `WorkerSpec` | oneof (kind) |
| `usage_policy` | `UsagePolicySpec` | oneof (kind) |
| `raw` | `RawResourceSpec` | oneof (kind) |

## `ResourceStatus`

| Field | Type | Notes |
| --- | --- | --- |
| `agent` | `AgentStatus` | oneof (kind) |
| `workflow` | `WorkflowStatus` | oneof (kind) |
| `schedule` | `ScheduleStatus` | oneof (kind) |
| `channel` | `ChannelStatus` | oneof (kind) |
| `channel_subscription` | `CommonResourceStatus` | oneof (kind) |
| `connector_class` | `ConnectorClassStatus` | oneof (kind) |
| `connector` | `ConnectorStatus` | oneof (kind) |
| `mcp_server` | `CommonResourceStatus` | oneof (kind) |
| `knowledge` | `CommonResourceStatus` | oneof (kind) |
| `namespace` | `NamespaceStatus` | oneof (kind) |
| `session` | `SessionStatus` | oneof (kind) |
| `skill` | `CommonResourceStatus` | oneof (kind) |
| `template` | `CommonResourceStatus` | oneof (kind) |
| `deployment` | `DeploymentStatus` | oneof (kind) |
| `deployment_replica` | `DeploymentReplicaStatus` | oneof (kind) |
| `sandbox_class` | `CommonResourceStatus` | oneof (kind) |
| `sandbox_policy` | `CommonResourceStatus` | oneof (kind) |
| `sandbox` | `SandboxStatus` | oneof (kind) |
| `worker` | `WorkerStatus` | oneof (kind) |
| `usage_policy` | `UsagePolicyStatus` | oneof (kind) |
| `raw` | `RawResourceStatus` | oneof (kind) |

## `ResourceRef`

| Field | Type | Notes |
| --- | --- | --- |
| `namespace` | `string` | Namespace containing the referenced Talon resource. Empty means the reference is resolved relative to the owning resource or route. |
| `name` | `string` | Resource name within namespace. |

## `SessionMessageConsumer`

| Field | Type | Notes |
| --- | --- | --- |
| `agent` | `ResourceRef` | Agent that consumes matching messages through a Talon Session. |
| `session_id` | `string` | Existing Talon Session id to deliver matching messages into. The session is resolved under agent.namespace/name, with an empty agent namespace resolved relative to the owning Connector namespace. |
| `continuity` | `string` | Session continuity policy. "reuse" reuses the connector session pointer for the external conversation/thread; "pinned" requires session_id; any other value creates a new Session for each message. |

## `ChannelMessageConsumer`

| Field | Type | Notes |
| --- | --- | --- |
| `channel` | `ResourceRef` | Channel that receives matching messages before agent routing. |
| `agent` | `ResourceRef` | Agent that consumes the persisted Channel message. |
| `continuity` | `string` | Channel routing continuity policy. This is reserved for channel dispatch policies that create agent runtime context per message or thread. |
| `reply_policy` | `string` | Reply behavior requested from the connector-aware channel router, such as replying in the provider thread instead of the root conversation. |

## `MessageConsumer`

| Field | Type | Notes |
| --- | --- | --- |
| `session` | `SessionMessageConsumer` | optional; Session consumer payload. Mutually exclusive with channel. |
| `channel` | `ChannelMessageConsumer` | optional; Channel consumer payload. Mutually exclusive with session. |
