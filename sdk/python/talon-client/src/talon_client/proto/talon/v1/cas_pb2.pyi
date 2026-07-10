from talon_client.proto.data import data_pb2 as _data_pb2
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class GetCasObjectRequest(_message.Message):
    __slots__ = ("ns", "agent", "session_id", "key")
    NS_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    KEY_FIELD_NUMBER: _ClassVar[int]
    ns: str
    agent: str
    session_id: str
    key: str
    def __init__(self, ns: _Optional[str] = ..., agent: _Optional[str] = ..., session_id: _Optional[str] = ..., key: _Optional[str] = ...) -> None: ...

class GetCasObjectResponse(_message.Message):
    __slots__ = ("object", "data")
    OBJECT_FIELD_NUMBER: _ClassVar[int]
    DATA_FIELD_NUMBER: _ClassVar[int]
    object: _data_pb2.ObjectRef
    data: bytes
    def __init__(self, object: _Optional[_Union[_data_pb2.ObjectRef, _Mapping]] = ..., data: _Optional[bytes] = ...) -> None: ...
