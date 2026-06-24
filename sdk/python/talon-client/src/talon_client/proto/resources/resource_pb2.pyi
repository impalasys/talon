from talon_client.proto.resources import agents_pb2 as _agents_pb2
from talon_client.proto.resources import channels_pb2 as _channels_pb2
from talon_client.proto.resources import common_pb2 as _common_pb2
from talon_client.proto.resources import connectors_pb2 as _connectors_pb2
from talon_client.proto.resources import deployments_pb2 as _deployments_pb2
from talon_client.proto.resources import knowledge_pb2 as _knowledge_pb2
from talon_client.proto.resources import mcp_pb2 as _mcp_pb2
from talon_client.proto.resources import namespaces_pb2 as _namespaces_pb2
from talon_client.proto.resources import sandboxes_pb2 as _sandboxes_pb2
from talon_client.proto.resources import schedules_pb2 as _schedules_pb2
from talon_client.proto.resources import sessions_pb2 as _sessions_pb2
from talon_client.proto.resources import skills_pb2 as _skills_pb2
from talon_client.proto.resources import usage_pb2 as _usage_pb2
from talon_client.proto.resources import workers_pb2 as _workers_pb2
from talon_client.proto.resources import workflows_pb2 as _workflows_pb2
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class Resource(_message.Message):
    __slots__ = ("api_version", "kind", "metadata", "spec", "status")
    API_VERSION_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    api_version: str
    kind: str
    metadata: _common_pb2.ResourceMeta
    spec: ResourceSpec
    status: ResourceStatus
    def __init__(self, api_version: _Optional[str] = ..., kind: _Optional[str] = ..., metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[ResourceSpec, _Mapping]] = ..., status: _Optional[_Union[ResourceStatus, _Mapping]] = ...) -> None: ...

class ResourceManifest(_message.Message):
    __slots__ = ("api_version", "kind", "metadata", "spec")
    API_VERSION_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    api_version: str
    kind: str
    metadata: _common_pb2.ResourceMeta
    spec: ResourceSpec
    def __init__(self, api_version: _Optional[str] = ..., kind: _Optional[str] = ..., metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[ResourceSpec, _Mapping]] = ...) -> None: ...

class RawResourceSpec(_message.Message):
    __slots__ = ("json",)
    JSON_FIELD_NUMBER: _ClassVar[int]
    json: str
    def __init__(self, json: _Optional[str] = ...) -> None: ...

class RawResourceStatus(_message.Message):
    __slots__ = ("json",)
    JSON_FIELD_NUMBER: _ClassVar[int]
    json: str
    def __init__(self, json: _Optional[str] = ...) -> None: ...

class ResourceSpec(_message.Message):
    __slots__ = ("agent", "workflow", "schedule", "channel", "channel_subscription", "connector_class", "connector", "mcp_server", "knowledge", "namespace", "session", "skill", "template", "deployment", "deployment_replica", "sandbox_class", "sandbox_policy", "sandbox", "worker", "usage_policy", "raw")
    AGENT_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    SCHEDULE_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_SUBSCRIPTION_FIELD_NUMBER: _ClassVar[int]
    CONNECTOR_CLASS_FIELD_NUMBER: _ClassVar[int]
    CONNECTOR_FIELD_NUMBER: _ClassVar[int]
    MCP_SERVER_FIELD_NUMBER: _ClassVar[int]
    KNOWLEDGE_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    SESSION_FIELD_NUMBER: _ClassVar[int]
    SKILL_FIELD_NUMBER: _ClassVar[int]
    TEMPLATE_FIELD_NUMBER: _ClassVar[int]
    DEPLOYMENT_FIELD_NUMBER: _ClassVar[int]
    DEPLOYMENT_REPLICA_FIELD_NUMBER: _ClassVar[int]
    SANDBOX_CLASS_FIELD_NUMBER: _ClassVar[int]
    SANDBOX_POLICY_FIELD_NUMBER: _ClassVar[int]
    SANDBOX_FIELD_NUMBER: _ClassVar[int]
    WORKER_FIELD_NUMBER: _ClassVar[int]
    USAGE_POLICY_FIELD_NUMBER: _ClassVar[int]
    RAW_FIELD_NUMBER: _ClassVar[int]
    agent: _agents_pb2.AgentSpec
    workflow: _workflows_pb2.WorkflowSpec
    schedule: _schedules_pb2.ScheduleSpec
    channel: _channels_pb2.ChannelSpec
    channel_subscription: _channels_pb2.ChannelSubscriptionSpec
    connector_class: _connectors_pb2.ConnectorClassSpec
    connector: _connectors_pb2.ConnectorSpec
    mcp_server: _mcp_pb2.McpServerSpec
    knowledge: _knowledge_pb2.KnowledgeSpec
    namespace: _namespaces_pb2.NamespaceSpec
    session: _sessions_pb2.SessionSpec
    skill: _skills_pb2.SkillSpec
    template: _deployments_pb2.TemplateSpec
    deployment: _deployments_pb2.DeploymentSpec
    deployment_replica: _deployments_pb2.DeploymentReplicaSpec
    sandbox_class: _sandboxes_pb2.SandboxClassSpec
    sandbox_policy: _sandboxes_pb2.SandboxPolicySpec
    sandbox: _sandboxes_pb2.SandboxSpec
    worker: _workers_pb2.WorkerSpec
    usage_policy: _usage_pb2.UsagePolicySpec
    raw: RawResourceSpec
    def __init__(self, agent: _Optional[_Union[_agents_pb2.AgentSpec, _Mapping]] = ..., workflow: _Optional[_Union[_workflows_pb2.WorkflowSpec, _Mapping]] = ..., schedule: _Optional[_Union[_schedules_pb2.ScheduleSpec, _Mapping]] = ..., channel: _Optional[_Union[_channels_pb2.ChannelSpec, _Mapping]] = ..., channel_subscription: _Optional[_Union[_channels_pb2.ChannelSubscriptionSpec, _Mapping]] = ..., connector_class: _Optional[_Union[_connectors_pb2.ConnectorClassSpec, _Mapping]] = ..., connector: _Optional[_Union[_connectors_pb2.ConnectorSpec, _Mapping]] = ..., mcp_server: _Optional[_Union[_mcp_pb2.McpServerSpec, _Mapping]] = ..., knowledge: _Optional[_Union[_knowledge_pb2.KnowledgeSpec, _Mapping]] = ..., namespace: _Optional[_Union[_namespaces_pb2.NamespaceSpec, _Mapping]] = ..., session: _Optional[_Union[_sessions_pb2.SessionSpec, _Mapping]] = ..., skill: _Optional[_Union[_skills_pb2.SkillSpec, _Mapping]] = ..., template: _Optional[_Union[_deployments_pb2.TemplateSpec, _Mapping]] = ..., deployment: _Optional[_Union[_deployments_pb2.DeploymentSpec, _Mapping]] = ..., deployment_replica: _Optional[_Union[_deployments_pb2.DeploymentReplicaSpec, _Mapping]] = ..., sandbox_class: _Optional[_Union[_sandboxes_pb2.SandboxClassSpec, _Mapping]] = ..., sandbox_policy: _Optional[_Union[_sandboxes_pb2.SandboxPolicySpec, _Mapping]] = ..., sandbox: _Optional[_Union[_sandboxes_pb2.SandboxSpec, _Mapping]] = ..., worker: _Optional[_Union[_workers_pb2.WorkerSpec, _Mapping]] = ..., usage_policy: _Optional[_Union[_usage_pb2.UsagePolicySpec, _Mapping]] = ..., raw: _Optional[_Union[RawResourceSpec, _Mapping]] = ...) -> None: ...

class ResourceStatus(_message.Message):
    __slots__ = ("agent", "workflow", "schedule", "channel", "channel_subscription", "connector_class", "connector", "mcp_server", "knowledge", "namespace", "session", "skill", "template", "deployment", "deployment_replica", "sandbox_class", "sandbox_policy", "sandbox", "worker", "usage_policy", "raw")
    AGENT_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    SCHEDULE_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_SUBSCRIPTION_FIELD_NUMBER: _ClassVar[int]
    CONNECTOR_CLASS_FIELD_NUMBER: _ClassVar[int]
    CONNECTOR_FIELD_NUMBER: _ClassVar[int]
    MCP_SERVER_FIELD_NUMBER: _ClassVar[int]
    KNOWLEDGE_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    SESSION_FIELD_NUMBER: _ClassVar[int]
    SKILL_FIELD_NUMBER: _ClassVar[int]
    TEMPLATE_FIELD_NUMBER: _ClassVar[int]
    DEPLOYMENT_FIELD_NUMBER: _ClassVar[int]
    DEPLOYMENT_REPLICA_FIELD_NUMBER: _ClassVar[int]
    SANDBOX_CLASS_FIELD_NUMBER: _ClassVar[int]
    SANDBOX_POLICY_FIELD_NUMBER: _ClassVar[int]
    SANDBOX_FIELD_NUMBER: _ClassVar[int]
    WORKER_FIELD_NUMBER: _ClassVar[int]
    USAGE_POLICY_FIELD_NUMBER: _ClassVar[int]
    RAW_FIELD_NUMBER: _ClassVar[int]
    agent: _agents_pb2.AgentStatus
    workflow: _workflows_pb2.WorkflowStatus
    schedule: _schedules_pb2.ScheduleStatus
    channel: _channels_pb2.ChannelStatus
    channel_subscription: _common_pb2.CommonResourceStatus
    connector_class: _connectors_pb2.ConnectorClassStatus
    connector: _connectors_pb2.ConnectorStatus
    mcp_server: _common_pb2.CommonResourceStatus
    knowledge: _common_pb2.CommonResourceStatus
    namespace: _namespaces_pb2.NamespaceStatus
    session: _sessions_pb2.SessionStatus
    skill: _common_pb2.CommonResourceStatus
    template: _common_pb2.CommonResourceStatus
    deployment: _deployments_pb2.DeploymentStatus
    deployment_replica: _deployments_pb2.DeploymentReplicaStatus
    sandbox_class: _common_pb2.CommonResourceStatus
    sandbox_policy: _common_pb2.CommonResourceStatus
    sandbox: _sandboxes_pb2.SandboxStatus
    worker: _workers_pb2.WorkerStatus
    usage_policy: _usage_pb2.UsagePolicyStatus
    raw: RawResourceStatus
    def __init__(self, agent: _Optional[_Union[_agents_pb2.AgentStatus, _Mapping]] = ..., workflow: _Optional[_Union[_workflows_pb2.WorkflowStatus, _Mapping]] = ..., schedule: _Optional[_Union[_schedules_pb2.ScheduleStatus, _Mapping]] = ..., channel: _Optional[_Union[_channels_pb2.ChannelStatus, _Mapping]] = ..., channel_subscription: _Optional[_Union[_common_pb2.CommonResourceStatus, _Mapping]] = ..., connector_class: _Optional[_Union[_connectors_pb2.ConnectorClassStatus, _Mapping]] = ..., connector: _Optional[_Union[_connectors_pb2.ConnectorStatus, _Mapping]] = ..., mcp_server: _Optional[_Union[_common_pb2.CommonResourceStatus, _Mapping]] = ..., knowledge: _Optional[_Union[_common_pb2.CommonResourceStatus, _Mapping]] = ..., namespace: _Optional[_Union[_namespaces_pb2.NamespaceStatus, _Mapping]] = ..., session: _Optional[_Union[_sessions_pb2.SessionStatus, _Mapping]] = ..., skill: _Optional[_Union[_common_pb2.CommonResourceStatus, _Mapping]] = ..., template: _Optional[_Union[_common_pb2.CommonResourceStatus, _Mapping]] = ..., deployment: _Optional[_Union[_deployments_pb2.DeploymentStatus, _Mapping]] = ..., deployment_replica: _Optional[_Union[_deployments_pb2.DeploymentReplicaStatus, _Mapping]] = ..., sandbox_class: _Optional[_Union[_common_pb2.CommonResourceStatus, _Mapping]] = ..., sandbox_policy: _Optional[_Union[_common_pb2.CommonResourceStatus, _Mapping]] = ..., sandbox: _Optional[_Union[_sandboxes_pb2.SandboxStatus, _Mapping]] = ..., worker: _Optional[_Union[_workers_pb2.WorkerStatus, _Mapping]] = ..., usage_policy: _Optional[_Union[_usage_pb2.UsagePolicyStatus, _Mapping]] = ..., raw: _Optional[_Union[RawResourceStatus, _Mapping]] = ...) -> None: ...
