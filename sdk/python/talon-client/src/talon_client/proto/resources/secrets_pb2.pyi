from talon_client.proto.resources import common_pb2 as _common_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class Secret(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: SecretSpec
    status: _common_pb2.CommonResourceStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[SecretSpec, _Mapping]] = ..., status: _Optional[_Union[_common_pb2.CommonResourceStatus, _Mapping]] = ...) -> None: ...

class SecretSpec(_message.Message):
    __slots__ = ("type", "data", "string_data")
    class DataEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    class StringDataEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    TYPE_FIELD_NUMBER: _ClassVar[int]
    DATA_FIELD_NUMBER: _ClassVar[int]
    STRING_DATA_FIELD_NUMBER: _ClassVar[int]
    type: str
    data: _containers.ScalarMap[str, str]
    string_data: _containers.ScalarMap[str, str]
    def __init__(self, type: _Optional[str] = ..., data: _Optional[_Mapping[str, str]] = ..., string_data: _Optional[_Mapping[str, str]] = ...) -> None: ...
