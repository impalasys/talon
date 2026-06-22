from talon_client.proto.resources import common_pb2 as _common_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class WorkerSpec(_message.Message):
    __slots__ = ()
    def __init__(self) -> None: ...

class WorkerEndpoint(_message.Message):
    __slots__ = ("url", "protocol", "audience")
    URL_FIELD_NUMBER: _ClassVar[int]
    PROTOCOL_FIELD_NUMBER: _ClassVar[int]
    AUDIENCE_FIELD_NUMBER: _ClassVar[int]
    url: str
    protocol: str
    audience: str
    def __init__(self, url: _Optional[str] = ..., protocol: _Optional[str] = ..., audience: _Optional[str] = ...) -> None: ...

class WorkerStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions", "started_at", "heartbeat_at", "expires_at", "version", "endpoints")
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    STARTED_AT_FIELD_NUMBER: _ClassVar[int]
    HEARTBEAT_AT_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    VERSION_FIELD_NUMBER: _ClassVar[int]
    ENDPOINTS_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: str
    conditions: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceCondition]
    started_at: int
    heartbeat_at: int
    expires_at: int
    version: str
    endpoints: _containers.RepeatedCompositeFieldContainer[WorkerEndpoint]
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[str] = ..., conditions: _Optional[_Iterable[_Union[_common_pb2.ResourceCondition, _Mapping]]] = ..., started_at: _Optional[int] = ..., heartbeat_at: _Optional[int] = ..., expires_at: _Optional[int] = ..., version: _Optional[str] = ..., endpoints: _Optional[_Iterable[_Union[WorkerEndpoint, _Mapping]]] = ...) -> None: ...

class Worker(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: WorkerSpec
    status: WorkerStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[WorkerSpec, _Mapping]] = ..., status: _Optional[_Union[WorkerStatus, _Mapping]] = ...) -> None: ...
