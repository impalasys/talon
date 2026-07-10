from talon_client.proto.data import data_pb2 as _data_pb2
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class GetCasObjectRequest(_message.Message):
    __slots__ = ("key",)
    KEY_FIELD_NUMBER: _ClassVar[int]
    key: str
    def __init__(self, key: _Optional[str] = ...) -> None: ...

class GetCasObjectResponse(_message.Message):
    __slots__ = ("object", "data", "signed_url", "signed_url_expires_at_unix_seconds")
    OBJECT_FIELD_NUMBER: _ClassVar[int]
    DATA_FIELD_NUMBER: _ClassVar[int]
    SIGNED_URL_FIELD_NUMBER: _ClassVar[int]
    SIGNED_URL_EXPIRES_AT_UNIX_SECONDS_FIELD_NUMBER: _ClassVar[int]
    object: _data_pb2.ObjectRef
    data: bytes
    signed_url: str
    signed_url_expires_at_unix_seconds: int
    def __init__(self, object: _Optional[_Union[_data_pb2.ObjectRef, _Mapping]] = ..., data: _Optional[bytes] = ..., signed_url: _Optional[str] = ..., signed_url_expires_at_unix_seconds: _Optional[int] = ...) -> None: ...
