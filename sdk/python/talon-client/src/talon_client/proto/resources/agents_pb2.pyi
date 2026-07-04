from google.protobuf import struct_pb2 as _struct_pb2
from talon_client.proto.resources import common_pb2 as _common_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class Agent(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: AgentSpec
    status: AgentStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[AgentSpec, _Mapping]] = ..., status: _Optional[_Union[AgentStatus, _Mapping]] = ...) -> None: ...

class AgentSpec(_message.Message):
    __slots__ = ("features", "model_policy", "system_prompt", "mcp_server_refs", "post_history_prompt", "capabilities", "a2a", "runtime")
    class CapabilitiesEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: _struct_pb2.ListValue
        def __init__(self, key: _Optional[str] = ..., value: _Optional[_Union[_struct_pb2.ListValue, _Mapping]] = ...) -> None: ...
    FEATURES_FIELD_NUMBER: _ClassVar[int]
    MODEL_POLICY_FIELD_NUMBER: _ClassVar[int]
    SYSTEM_PROMPT_FIELD_NUMBER: _ClassVar[int]
    MCP_SERVER_REFS_FIELD_NUMBER: _ClassVar[int]
    POST_HISTORY_PROMPT_FIELD_NUMBER: _ClassVar[int]
    CAPABILITIES_FIELD_NUMBER: _ClassVar[int]
    A2A_FIELD_NUMBER: _ClassVar[int]
    RUNTIME_FIELD_NUMBER: _ClassVar[int]
    features: _containers.RepeatedCompositeFieldContainer[Feature]
    model_policy: ModelPolicy
    system_prompt: str
    mcp_server_refs: _containers.RepeatedScalarFieldContainer[str]
    post_history_prompt: str
    capabilities: _containers.MessageMap[str, _struct_pb2.ListValue]
    a2a: A2A
    runtime: AgentRuntime
    def __init__(self, features: _Optional[_Iterable[_Union[Feature, _Mapping]]] = ..., model_policy: _Optional[_Union[ModelPolicy, _Mapping]] = ..., system_prompt: _Optional[str] = ..., mcp_server_refs: _Optional[_Iterable[str]] = ..., post_history_prompt: _Optional[str] = ..., capabilities: _Optional[_Mapping[str, _struct_pb2.ListValue]] = ..., a2a: _Optional[_Union[A2A, _Mapping]] = ..., runtime: _Optional[_Union[AgentRuntime, _Mapping]] = ...) -> None: ...

class AgentStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions", "last_session_id")
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    LAST_SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: str
    conditions: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceCondition]
    last_session_id: str
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[str] = ..., conditions: _Optional[_Iterable[_Union[_common_pb2.ResourceCondition, _Mapping]]] = ..., last_session_id: _Optional[str] = ...) -> None: ...

class AgentRuntime(_message.Message):
    __slots__ = ("kind", "acp")
    KIND_FIELD_NUMBER: _ClassVar[int]
    ACP_FIELD_NUMBER: _ClassVar[int]
    kind: str
    acp: AcpRuntime
    def __init__(self, kind: _Optional[str] = ..., acp: _Optional[_Union[AcpRuntime, _Mapping]] = ...) -> None: ...

class AcpRuntime(_message.Message):
    __slots__ = ("harness_ref", "command", "args", "cwd", "sandbox_policy_ref", "persist_session", "env", "permission_policy")
    class EnvEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    class PermissionPolicyEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    HARNESS_REF_FIELD_NUMBER: _ClassVar[int]
    COMMAND_FIELD_NUMBER: _ClassVar[int]
    ARGS_FIELD_NUMBER: _ClassVar[int]
    CWD_FIELD_NUMBER: _ClassVar[int]
    SANDBOX_POLICY_REF_FIELD_NUMBER: _ClassVar[int]
    PERSIST_SESSION_FIELD_NUMBER: _ClassVar[int]
    ENV_FIELD_NUMBER: _ClassVar[int]
    PERMISSION_POLICY_FIELD_NUMBER: _ClassVar[int]
    harness_ref: str
    command: str
    args: _containers.RepeatedScalarFieldContainer[str]
    cwd: str
    sandbox_policy_ref: str
    persist_session: bool
    env: _containers.ScalarMap[str, str]
    permission_policy: _containers.ScalarMap[str, str]
    def __init__(self, harness_ref: _Optional[str] = ..., command: _Optional[str] = ..., args: _Optional[_Iterable[str]] = ..., cwd: _Optional[str] = ..., sandbox_policy_ref: _Optional[str] = ..., persist_session: bool = ..., env: _Optional[_Mapping[str, str]] = ..., permission_policy: _Optional[_Mapping[str, str]] = ...) -> None: ...

class Feature(_message.Message):
    __slots__ = ("name", "type", "required")
    NAME_FIELD_NUMBER: _ClassVar[int]
    TYPE_FIELD_NUMBER: _ClassVar[int]
    REQUIRED_FIELD_NUMBER: _ClassVar[int]
    name: str
    type: str
    required: bool
    def __init__(self, name: _Optional[str] = ..., type: _Optional[str] = ..., required: bool = ...) -> None: ...

class Model(_message.Message):
    __slots__ = ("provider", "name", "temperature", "thinking")
    PROVIDER_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    TEMPERATURE_FIELD_NUMBER: _ClassVar[int]
    THINKING_FIELD_NUMBER: _ClassVar[int]
    provider: str
    name: str
    temperature: float
    thinking: ThinkingConfig
    def __init__(self, provider: _Optional[str] = ..., name: _Optional[str] = ..., temperature: _Optional[float] = ..., thinking: _Optional[_Union[ThinkingConfig, _Mapping]] = ...) -> None: ...

class ThinkingConfig(_message.Message):
    __slots__ = ("enabled", "budget_tokens", "effort")
    ENABLED_FIELD_NUMBER: _ClassVar[int]
    BUDGET_TOKENS_FIELD_NUMBER: _ClassVar[int]
    EFFORT_FIELD_NUMBER: _ClassVar[int]
    enabled: bool
    budget_tokens: int
    effort: str
    def __init__(self, enabled: bool = ..., budget_tokens: _Optional[int] = ..., effort: _Optional[str] = ...) -> None: ...

class ModelProfile(_message.Message):
    __slots__ = ("name", "model")
    NAME_FIELD_NUMBER: _ClassVar[int]
    MODEL_FIELD_NUMBER: _ClassVar[int]
    name: str
    model: Model
    def __init__(self, name: _Optional[str] = ..., model: _Optional[_Union[Model, _Mapping]] = ...) -> None: ...

class ModelPolicy(_message.Message):
    __slots__ = ("profiles",)
    PROFILES_FIELD_NUMBER: _ClassVar[int]
    profiles: _containers.RepeatedCompositeFieldContainer[ModelProfile]
    def __init__(self, profiles: _Optional[_Iterable[_Union[ModelProfile, _Mapping]]] = ...) -> None: ...

class A2A(_message.Message):
    __slots__ = ("connections", "agent_card")
    CONNECTIONS_FIELD_NUMBER: _ClassVar[int]
    AGENT_CARD_FIELD_NUMBER: _ClassVar[int]
    connections: _containers.RepeatedCompositeFieldContainer[Connection]
    agent_card: AgentCard
    def __init__(self, connections: _Optional[_Iterable[_Union[Connection, _Mapping]]] = ..., agent_card: _Optional[_Union[AgentCard, _Mapping]] = ...) -> None: ...

class Connection(_message.Message):
    __slots__ = ("name", "description", "target", "input_modes", "output_modes", "timeout_seconds", "max_depth", "auth")
    NAME_FIELD_NUMBER: _ClassVar[int]
    DESCRIPTION_FIELD_NUMBER: _ClassVar[int]
    TARGET_FIELD_NUMBER: _ClassVar[int]
    INPUT_MODES_FIELD_NUMBER: _ClassVar[int]
    OUTPUT_MODES_FIELD_NUMBER: _ClassVar[int]
    TIMEOUT_SECONDS_FIELD_NUMBER: _ClassVar[int]
    MAX_DEPTH_FIELD_NUMBER: _ClassVar[int]
    AUTH_FIELD_NUMBER: _ClassVar[int]
    name: str
    description: str
    target: ConnectionRef
    input_modes: _containers.RepeatedScalarFieldContainer[str]
    output_modes: _containers.RepeatedScalarFieldContainer[str]
    timeout_seconds: int
    max_depth: int
    auth: ConnectionAuth
    def __init__(self, name: _Optional[str] = ..., description: _Optional[str] = ..., target: _Optional[_Union[ConnectionRef, _Mapping]] = ..., input_modes: _Optional[_Iterable[str]] = ..., output_modes: _Optional[_Iterable[str]] = ..., timeout_seconds: _Optional[int] = ..., max_depth: _Optional[int] = ..., auth: _Optional[_Union[ConnectionAuth, _Mapping]] = ...) -> None: ...

class ConnectionRef(_message.Message):
    __slots__ = ("internal", "external")
    INTERNAL_FIELD_NUMBER: _ClassVar[int]
    EXTERNAL_FIELD_NUMBER: _ClassVar[int]
    internal: InternalConnectionRef
    external: ExternalConnectionRef
    def __init__(self, internal: _Optional[_Union[InternalConnectionRef, _Mapping]] = ..., external: _Optional[_Union[ExternalConnectionRef, _Mapping]] = ...) -> None: ...

class InternalConnectionRef(_message.Message):
    __slots__ = ("namespace", "agent")
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    namespace: str
    agent: str
    def __init__(self, namespace: _Optional[str] = ..., agent: _Optional[str] = ...) -> None: ...

class ExternalConnectionRef(_message.Message):
    __slots__ = ("agent_card_url",)
    AGENT_CARD_URL_FIELD_NUMBER: _ClassVar[int]
    agent_card_url: str
    def __init__(self, agent_card_url: _Optional[str] = ...) -> None: ...

class ConnectionAuth(_message.Message):
    __slots__ = ("kind", "secret_ref")
    KIND_FIELD_NUMBER: _ClassVar[int]
    SECRET_REF_FIELD_NUMBER: _ClassVar[int]
    kind: str
    secret_ref: str
    def __init__(self, kind: _Optional[str] = ..., secret_ref: _Optional[str] = ...) -> None: ...

class AgentCard(_message.Message):
    __slots__ = ("name", "description", "version", "capabilities", "default_input_modes", "default_output_modes", "skills")
    NAME_FIELD_NUMBER: _ClassVar[int]
    DESCRIPTION_FIELD_NUMBER: _ClassVar[int]
    VERSION_FIELD_NUMBER: _ClassVar[int]
    CAPABILITIES_FIELD_NUMBER: _ClassVar[int]
    DEFAULT_INPUT_MODES_FIELD_NUMBER: _ClassVar[int]
    DEFAULT_OUTPUT_MODES_FIELD_NUMBER: _ClassVar[int]
    SKILLS_FIELD_NUMBER: _ClassVar[int]
    name: str
    description: str
    version: str
    capabilities: AgentCardCapabilities
    default_input_modes: _containers.RepeatedScalarFieldContainer[str]
    default_output_modes: _containers.RepeatedScalarFieldContainer[str]
    skills: _containers.RepeatedCompositeFieldContainer[AgentCardSkill]
    def __init__(self, name: _Optional[str] = ..., description: _Optional[str] = ..., version: _Optional[str] = ..., capabilities: _Optional[_Union[AgentCardCapabilities, _Mapping]] = ..., default_input_modes: _Optional[_Iterable[str]] = ..., default_output_modes: _Optional[_Iterable[str]] = ..., skills: _Optional[_Iterable[_Union[AgentCardSkill, _Mapping]]] = ...) -> None: ...

class AgentCardCapabilities(_message.Message):
    __slots__ = ("streaming", "push_notifications", "extended_agent_card")
    STREAMING_FIELD_NUMBER: _ClassVar[int]
    PUSH_NOTIFICATIONS_FIELD_NUMBER: _ClassVar[int]
    EXTENDED_AGENT_CARD_FIELD_NUMBER: _ClassVar[int]
    streaming: bool
    push_notifications: bool
    extended_agent_card: bool
    def __init__(self, streaming: bool = ..., push_notifications: bool = ..., extended_agent_card: bool = ...) -> None: ...

class AgentCardSkill(_message.Message):
    __slots__ = ("id", "name", "description", "tags", "examples", "input_modes", "output_modes")
    ID_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    DESCRIPTION_FIELD_NUMBER: _ClassVar[int]
    TAGS_FIELD_NUMBER: _ClassVar[int]
    EXAMPLES_FIELD_NUMBER: _ClassVar[int]
    INPUT_MODES_FIELD_NUMBER: _ClassVar[int]
    OUTPUT_MODES_FIELD_NUMBER: _ClassVar[int]
    id: str
    name: str
    description: str
    tags: _containers.RepeatedScalarFieldContainer[str]
    examples: _containers.RepeatedScalarFieldContainer[str]
    input_modes: _containers.RepeatedScalarFieldContainer[str]
    output_modes: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, id: _Optional[str] = ..., name: _Optional[str] = ..., description: _Optional[str] = ..., tags: _Optional[_Iterable[str]] = ..., examples: _Optional[_Iterable[str]] = ..., input_modes: _Optional[_Iterable[str]] = ..., output_modes: _Optional[_Iterable[str]] = ...) -> None: ...
