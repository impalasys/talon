from talon_client.proto.resources import common_pb2 as _common_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class TemplateSpec(_message.Message):
    __slots__ = ("kind", "metadata", "spec_json")
    KIND_FIELD_NUMBER: _ClassVar[int]
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_JSON_FIELD_NUMBER: _ClassVar[int]
    kind: str
    metadata: _common_pb2.ResourceMeta
    spec_json: str
    def __init__(self, kind: _Optional[str] = ..., metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec_json: _Optional[str] = ...) -> None: ...

class Template(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: TemplateSpec
    status: _common_pb2.CommonResourceStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[TemplateSpec, _Mapping]] = ..., status: _Optional[_Union[_common_pb2.CommonResourceStatus, _Mapping]] = ...) -> None: ...

class DeploymentPlacement(_message.Message):
    __slots__ = ("namespace_selector",)
    NAMESPACE_SELECTOR_FIELD_NUMBER: _ClassVar[int]
    namespace_selector: _common_pb2.NamespaceSelector
    def __init__(self, namespace_selector: _Optional[_Union[_common_pb2.NamespaceSelector, _Mapping]] = ...) -> None: ...

class DeploymentSpec(_message.Message):
    __slots__ = ("placement", "templates")
    PLACEMENT_FIELD_NUMBER: _ClassVar[int]
    TEMPLATES_FIELD_NUMBER: _ClassVar[int]
    placement: DeploymentPlacement
    templates: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, placement: _Optional[_Union[DeploymentPlacement, _Mapping]] = ..., templates: _Optional[_Iterable[str]] = ...) -> None: ...

class Deployment(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: DeploymentSpec
    status: DeploymentStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[DeploymentSpec, _Mapping]] = ..., status: _Optional[_Union[DeploymentStatus, _Mapping]] = ...) -> None: ...

class DeploymentReplicaSpec(_message.Message):
    __slots__ = ("deployment_ref", "target_namespace")
    DEPLOYMENT_REF_FIELD_NUMBER: _ClassVar[int]
    TARGET_NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    deployment_ref: _common_pb2.ResourceRef
    target_namespace: str
    def __init__(self, deployment_ref: _Optional[_Union[_common_pb2.ResourceRef, _Mapping]] = ..., target_namespace: _Optional[str] = ...) -> None: ...

class DeploymentReplica(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: DeploymentReplicaSpec
    status: DeploymentReplicaStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[DeploymentReplicaSpec, _Mapping]] = ..., status: _Optional[_Union[DeploymentReplicaStatus, _Mapping]] = ...) -> None: ...

class DeploymentStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions", "replicas")
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    REPLICAS_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: str
    conditions: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceCondition]
    replicas: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceRef]
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[str] = ..., conditions: _Optional[_Iterable[_Union[_common_pb2.ResourceCondition, _Mapping]]] = ..., replicas: _Optional[_Iterable[_Union[_common_pb2.ResourceRef, _Mapping]]] = ...) -> None: ...

class DeploymentReplicaStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions", "rendered_resources", "rendered_hashes", "conflicts", "last_rendered_json", "owned_json_pointers")
    class RenderedHashesEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    class LastRenderedJsonEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    RENDERED_RESOURCES_FIELD_NUMBER: _ClassVar[int]
    RENDERED_HASHES_FIELD_NUMBER: _ClassVar[int]
    CONFLICTS_FIELD_NUMBER: _ClassVar[int]
    LAST_RENDERED_JSON_FIELD_NUMBER: _ClassVar[int]
    OWNED_JSON_POINTERS_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: str
    conditions: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceCondition]
    rendered_resources: _containers.RepeatedScalarFieldContainer[str]
    rendered_hashes: _containers.ScalarMap[str, str]
    conflicts: _containers.RepeatedScalarFieldContainer[str]
    last_rendered_json: _containers.ScalarMap[str, str]
    owned_json_pointers: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[str] = ..., conditions: _Optional[_Iterable[_Union[_common_pb2.ResourceCondition, _Mapping]]] = ..., rendered_resources: _Optional[_Iterable[str]] = ..., rendered_hashes: _Optional[_Mapping[str, str]] = ..., conflicts: _Optional[_Iterable[str]] = ..., last_rendered_json: _Optional[_Mapping[str, str]] = ..., owned_json_pointers: _Optional[_Iterable[str]] = ...) -> None: ...
