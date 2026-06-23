from talon_client.proto.resources import common_pb2 as _common_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class SandboxClassSpec(_message.Message):
    __slots__ = ("provider", "provider_config_json", "credentials_json")
    PROVIDER_FIELD_NUMBER: _ClassVar[int]
    PROVIDER_CONFIG_JSON_FIELD_NUMBER: _ClassVar[int]
    CREDENTIALS_JSON_FIELD_NUMBER: _ClassVar[int]
    provider: str
    provider_config_json: str
    credentials_json: str
    def __init__(self, provider: _Optional[str] = ..., provider_config_json: _Optional[str] = ..., credentials_json: _Optional[str] = ...) -> None: ...

class SandboxClass(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: SandboxClassSpec
    status: _common_pb2.CommonResourceStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[SandboxClassSpec, _Mapping]] = ..., status: _Optional[_Union[_common_pb2.CommonResourceStatus, _Mapping]] = ...) -> None: ...

class SandboxWorkspaceSpec(_message.Message):
    __slots__ = ("mode", "mount_path")
    MODE_FIELD_NUMBER: _ClassVar[int]
    MOUNT_PATH_FIELD_NUMBER: _ClassVar[int]
    mode: str
    mount_path: str
    def __init__(self, mode: _Optional[str] = ..., mount_path: _Optional[str] = ...) -> None: ...

class SandboxSetupSpec(_message.Message):
    __slots__ = ("packages", "commands")
    PACKAGES_FIELD_NUMBER: _ClassVar[int]
    COMMANDS_FIELD_NUMBER: _ClassVar[int]
    packages: _containers.RepeatedScalarFieldContainer[str]
    commands: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, packages: _Optional[_Iterable[str]] = ..., commands: _Optional[_Iterable[str]] = ...) -> None: ...

class SandboxNetworkSpec(_message.Message):
    __slots__ = ("mode",)
    MODE_FIELD_NUMBER: _ClassVar[int]
    mode: str
    def __init__(self, mode: _Optional[str] = ...) -> None: ...

class SandboxFilesystemSpec(_message.Message):
    __slots__ = ("writable", "readonly")
    WRITABLE_FIELD_NUMBER: _ClassVar[int]
    READONLY_FIELD_NUMBER: _ClassVar[int]
    writable: _containers.RepeatedScalarFieldContainer[str]
    readonly: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, writable: _Optional[_Iterable[str]] = ..., readonly: _Optional[_Iterable[str]] = ...) -> None: ...

class SandboxLeasePolicySpec(_message.Message):
    __slots__ = ("mode",)
    MODE_FIELD_NUMBER: _ClassVar[int]
    mode: str
    def __init__(self, mode: _Optional[str] = ...) -> None: ...

class SandboxRuntimeTemplateSpec(_message.Message):
    __slots__ = ("image", "workspace", "setup", "network", "filesystem", "lease_policy")
    IMAGE_FIELD_NUMBER: _ClassVar[int]
    WORKSPACE_FIELD_NUMBER: _ClassVar[int]
    SETUP_FIELD_NUMBER: _ClassVar[int]
    NETWORK_FIELD_NUMBER: _ClassVar[int]
    FILESYSTEM_FIELD_NUMBER: _ClassVar[int]
    LEASE_POLICY_FIELD_NUMBER: _ClassVar[int]
    image: str
    workspace: SandboxWorkspaceSpec
    setup: SandboxSetupSpec
    network: SandboxNetworkSpec
    filesystem: SandboxFilesystemSpec
    lease_policy: SandboxLeasePolicySpec
    def __init__(self, image: _Optional[str] = ..., workspace: _Optional[_Union[SandboxWorkspaceSpec, _Mapping]] = ..., setup: _Optional[_Union[SandboxSetupSpec, _Mapping]] = ..., network: _Optional[_Union[SandboxNetworkSpec, _Mapping]] = ..., filesystem: _Optional[_Union[SandboxFilesystemSpec, _Mapping]] = ..., lease_policy: _Optional[_Union[SandboxLeasePolicySpec, _Mapping]] = ...) -> None: ...

class SandboxPolicySpec(_message.Message):
    __slots__ = ("class_ref", "template", "max_concurrent")
    CLASS_REF_FIELD_NUMBER: _ClassVar[int]
    TEMPLATE_FIELD_NUMBER: _ClassVar[int]
    MAX_CONCURRENT_FIELD_NUMBER: _ClassVar[int]
    class_ref: _common_pb2.ResourceRef
    template: SandboxRuntimeTemplateSpec
    max_concurrent: int
    def __init__(self, class_ref: _Optional[_Union[_common_pb2.ResourceRef, _Mapping]] = ..., template: _Optional[_Union[SandboxRuntimeTemplateSpec, _Mapping]] = ..., max_concurrent: _Optional[int] = ...) -> None: ...

class SandboxPolicy(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: SandboxPolicySpec
    status: _common_pb2.CommonResourceStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[SandboxPolicySpec, _Mapping]] = ..., status: _Optional[_Union[_common_pb2.CommonResourceStatus, _Mapping]] = ...) -> None: ...

class SandboxLease(_message.Message):
    __slots__ = ("owner_kind", "owner_agent", "owner_session_id", "token", "acquired_at", "expires_at", "heartbeat_at")
    OWNER_KIND_FIELD_NUMBER: _ClassVar[int]
    OWNER_AGENT_FIELD_NUMBER: _ClassVar[int]
    OWNER_SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    TOKEN_FIELD_NUMBER: _ClassVar[int]
    ACQUIRED_AT_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    HEARTBEAT_AT_FIELD_NUMBER: _ClassVar[int]
    owner_kind: str
    owner_agent: str
    owner_session_id: str
    token: str
    acquired_at: int
    expires_at: int
    heartbeat_at: int
    def __init__(self, owner_kind: _Optional[str] = ..., owner_agent: _Optional[str] = ..., owner_session_id: _Optional[str] = ..., token: _Optional[str] = ..., acquired_at: _Optional[int] = ..., expires_at: _Optional[int] = ..., heartbeat_at: _Optional[int] = ...) -> None: ...

class SandboxProcessStatus(_message.Message):
    __slots__ = ("id", "command", "args", "protocol", "phase")
    ID_FIELD_NUMBER: _ClassVar[int]
    COMMAND_FIELD_NUMBER: _ClassVar[int]
    ARGS_FIELD_NUMBER: _ClassVar[int]
    PROTOCOL_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    id: str
    command: str
    args: _containers.RepeatedScalarFieldContainer[str]
    protocol: str
    phase: str
    def __init__(self, id: _Optional[str] = ..., command: _Optional[str] = ..., args: _Optional[_Iterable[str]] = ..., protocol: _Optional[str] = ..., phase: _Optional[str] = ...) -> None: ...

class SandboxStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions", "backend_id", "lease", "processes")
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    BACKEND_ID_FIELD_NUMBER: _ClassVar[int]
    LEASE_FIELD_NUMBER: _ClassVar[int]
    PROCESSES_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: str
    conditions: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceCondition]
    backend_id: str
    lease: SandboxLease
    processes: _containers.RepeatedCompositeFieldContainer[SandboxProcessStatus]
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[str] = ..., conditions: _Optional[_Iterable[_Union[_common_pb2.ResourceCondition, _Mapping]]] = ..., backend_id: _Optional[str] = ..., lease: _Optional[_Union[SandboxLease, _Mapping]] = ..., processes: _Optional[_Iterable[_Union[SandboxProcessStatus, _Mapping]]] = ...) -> None: ...

class SandboxSpec(_message.Message):
    __slots__ = ("policy_ref", "class_ref", "runtime_template")
    POLICY_REF_FIELD_NUMBER: _ClassVar[int]
    CLASS_REF_FIELD_NUMBER: _ClassVar[int]
    RUNTIME_TEMPLATE_FIELD_NUMBER: _ClassVar[int]
    policy_ref: str
    class_ref: _common_pb2.ResourceRef
    runtime_template: SandboxRuntimeTemplateSpec
    def __init__(self, policy_ref: _Optional[str] = ..., class_ref: _Optional[_Union[_common_pb2.ResourceRef, _Mapping]] = ..., runtime_template: _Optional[_Union[SandboxRuntimeTemplateSpec, _Mapping]] = ...) -> None: ...

class Sandbox(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: SandboxSpec
    status: SandboxStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[SandboxSpec, _Mapping]] = ..., status: _Optional[_Union[SandboxStatus, _Mapping]] = ...) -> None: ...
