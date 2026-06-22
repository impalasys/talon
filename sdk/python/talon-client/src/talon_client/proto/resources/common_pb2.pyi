from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class OwnerReference(_message.Message):
    __slots__ = ("api_version", "kind", "namespace", "name", "uid", "controller", "block_owner_deletion")
    API_VERSION_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    UID_FIELD_NUMBER: _ClassVar[int]
    CONTROLLER_FIELD_NUMBER: _ClassVar[int]
    BLOCK_OWNER_DELETION_FIELD_NUMBER: _ClassVar[int]
    api_version: str
    kind: str
    namespace: str
    name: str
    uid: str
    controller: bool
    block_owner_deletion: bool
    def __init__(self, api_version: _Optional[str] = ..., kind: _Optional[str] = ..., namespace: _Optional[str] = ..., name: _Optional[str] = ..., uid: _Optional[str] = ..., controller: bool = ..., block_owner_deletion: bool = ...) -> None: ...

class ResourceMeta(_message.Message):
    __slots__ = ("name", "namespace", "labels", "annotations", "owner_references", "finalizers", "generation", "resource_version", "uid", "deletion_timestamp")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    class AnnotationsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    NAME_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    ANNOTATIONS_FIELD_NUMBER: _ClassVar[int]
    OWNER_REFERENCES_FIELD_NUMBER: _ClassVar[int]
    FINALIZERS_FIELD_NUMBER: _ClassVar[int]
    GENERATION_FIELD_NUMBER: _ClassVar[int]
    RESOURCE_VERSION_FIELD_NUMBER: _ClassVar[int]
    UID_FIELD_NUMBER: _ClassVar[int]
    DELETION_TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    name: str
    namespace: str
    labels: _containers.ScalarMap[str, str]
    annotations: _containers.ScalarMap[str, str]
    owner_references: _containers.RepeatedCompositeFieldContainer[OwnerReference]
    finalizers: _containers.RepeatedScalarFieldContainer[str]
    generation: int
    resource_version: str
    uid: str
    deletion_timestamp: int
    def __init__(self, name: _Optional[str] = ..., namespace: _Optional[str] = ..., labels: _Optional[_Mapping[str, str]] = ..., annotations: _Optional[_Mapping[str, str]] = ..., owner_references: _Optional[_Iterable[_Union[OwnerReference, _Mapping]]] = ..., finalizers: _Optional[_Iterable[str]] = ..., generation: _Optional[int] = ..., resource_version: _Optional[str] = ..., uid: _Optional[str] = ..., deletion_timestamp: _Optional[int] = ...) -> None: ...

class ResourceCondition(_message.Message):
    __slots__ = ("type", "status", "reason", "message", "last_transition_time", "observed_generation")
    TYPE_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    REASON_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    LAST_TRANSITION_TIME_FIELD_NUMBER: _ClassVar[int]
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    type: str
    status: str
    reason: str
    message: str
    last_transition_time: int
    observed_generation: int
    def __init__(self, type: _Optional[str] = ..., status: _Optional[str] = ..., reason: _Optional[str] = ..., message: _Optional[str] = ..., last_transition_time: _Optional[int] = ..., observed_generation: _Optional[int] = ...) -> None: ...

class CommonResourceStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions")
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: str
    conditions: _containers.RepeatedCompositeFieldContainer[ResourceCondition]
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[str] = ..., conditions: _Optional[_Iterable[_Union[ResourceCondition, _Mapping]]] = ...) -> None: ...

class ResourceRef(_message.Message):
    __slots__ = ("namespace", "name")
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    namespace: str
    name: str
    def __init__(self, namespace: _Optional[str] = ..., name: _Optional[str] = ...) -> None: ...

class NamespaceSelector(_message.Message):
    __slots__ = ("parent", "match_labels")
    class MatchLabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    PARENT_FIELD_NUMBER: _ClassVar[int]
    MATCH_LABELS_FIELD_NUMBER: _ClassVar[int]
    parent: str
    match_labels: _containers.ScalarMap[str, str]
    def __init__(self, parent: _Optional[str] = ..., match_labels: _Optional[_Mapping[str, str]] = ...) -> None: ...
