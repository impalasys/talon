from talon_client.proto.resources import common_pb2 as _common_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class SessionSpec(_message.Message):
    __slots__ = ("agent", "labels")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    AGENT_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    agent: str
    labels: _containers.ScalarMap[str, str]
    def __init__(self, agent: _Optional[str] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class Session(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: SessionSpec
    status: SessionStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[SessionSpec, _Mapping]] = ..., status: _Optional[_Union[SessionStatus, _Mapping]] = ...) -> None: ...

class SessionStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions", "created_at", "last_active", "acp_session_id", "sandbox_ref")
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    LAST_ACTIVE_FIELD_NUMBER: _ClassVar[int]
    ACP_SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    SANDBOX_REF_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: str
    conditions: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceCondition]
    created_at: int
    last_active: int
    acp_session_id: str
    sandbox_ref: str
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[str] = ..., conditions: _Optional[_Iterable[_Union[_common_pb2.ResourceCondition, _Mapping]]] = ..., created_at: _Optional[int] = ..., last_active: _Optional[int] = ..., acp_session_id: _Optional[str] = ..., sandbox_ref: _Optional[str] = ...) -> None: ...
