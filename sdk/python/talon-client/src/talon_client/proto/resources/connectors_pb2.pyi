from talon_client.proto import config_pb2 as _config_pb2
from talon_client.proto.resources import common_pb2 as _common_pb2
from talon_client.proto.resources import routing_pb2 as _routing_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class ConnectorClassRuntimeSpec(_message.Message):
    __slots__ = ("kind", "endpoint")
    KIND_FIELD_NUMBER: _ClassVar[int]
    ENDPOINT_FIELD_NUMBER: _ClassVar[int]
    kind: str
    endpoint: str
    def __init__(self, kind: _Optional[str] = ..., endpoint: _Optional[str] = ...) -> None: ...

class ConnectorClassAuthSpec(_message.Message):
    __slots__ = ("kind", "api_key")
    KIND_FIELD_NUMBER: _ClassVar[int]
    API_KEY_FIELD_NUMBER: _ClassVar[int]
    kind: str
    api_key: _config_pb2.Secret
    def __init__(self, kind: _Optional[str] = ..., api_key: _Optional[_Union[_config_pb2.Secret, _Mapping]] = ...) -> None: ...

class ConnectorMatchIndex(_message.Message):
    __slots__ = ("name", "fields")
    NAME_FIELD_NUMBER: _ClassVar[int]
    FIELDS_FIELD_NUMBER: _ClassVar[int]
    name: str
    fields: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, name: _Optional[str] = ..., fields: _Optional[_Iterable[str]] = ...) -> None: ...

class ConnectorClassSpec(_message.Message):
    __slots__ = ("platform", "runtime", "auth", "match_indexes")
    PLATFORM_FIELD_NUMBER: _ClassVar[int]
    RUNTIME_FIELD_NUMBER: _ClassVar[int]
    AUTH_FIELD_NUMBER: _ClassVar[int]
    MATCH_INDEXES_FIELD_NUMBER: _ClassVar[int]
    platform: str
    runtime: ConnectorClassRuntimeSpec
    auth: ConnectorClassAuthSpec
    match_indexes: _containers.RepeatedCompositeFieldContainer[ConnectorMatchIndex]
    def __init__(self, platform: _Optional[str] = ..., runtime: _Optional[_Union[ConnectorClassRuntimeSpec, _Mapping]] = ..., auth: _Optional[_Union[ConnectorClassAuthSpec, _Mapping]] = ..., match_indexes: _Optional[_Iterable[_Union[ConnectorMatchIndex, _Mapping]]] = ...) -> None: ...

class ConnectorClassStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions")
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: str
    conditions: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceCondition]
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[str] = ..., conditions: _Optional[_Iterable[_Union[_common_pb2.ResourceCondition, _Mapping]]] = ...) -> None: ...

class ConnectorSpec(_message.Message):
    __slots__ = ("class_ref", "enabled", "match_fields", "consumer")
    class MatchFieldsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    CLASS_REF_FIELD_NUMBER: _ClassVar[int]
    ENABLED_FIELD_NUMBER: _ClassVar[int]
    MATCH_FIELDS_FIELD_NUMBER: _ClassVar[int]
    CONSUMER_FIELD_NUMBER: _ClassVar[int]
    class_ref: _common_pb2.ResourceRef
    enabled: bool
    match_fields: _containers.ScalarMap[str, str]
    consumer: _routing_pb2.MessageConsumer
    def __init__(self, class_ref: _Optional[_Union[_common_pb2.ResourceRef, _Mapping]] = ..., enabled: bool = ..., match_fields: _Optional[_Mapping[str, str]] = ..., consumer: _Optional[_Union[_routing_pb2.MessageConsumer, _Mapping]] = ...) -> None: ...

class ConnectorStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions", "compiled_route_ids")
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    COMPILED_ROUTE_IDS_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: str
    conditions: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceCondition]
    compiled_route_ids: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[str] = ..., conditions: _Optional[_Iterable[_Union[_common_pb2.ResourceCondition, _Mapping]]] = ..., compiled_route_ids: _Optional[_Iterable[str]] = ...) -> None: ...

class ConnectorClass(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: ConnectorClassSpec
    status: ConnectorClassStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[ConnectorClassSpec, _Mapping]] = ..., status: _Optional[_Union[ConnectorClassStatus, _Mapping]] = ...) -> None: ...

class Connector(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: ConnectorSpec
    status: ConnectorStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[ConnectorSpec, _Mapping]] = ..., status: _Optional[_Union[ConnectorStatus, _Mapping]] = ...) -> None: ...
