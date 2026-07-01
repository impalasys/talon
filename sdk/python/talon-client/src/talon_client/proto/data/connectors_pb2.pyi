from talon_client.proto.resources import common_pb2 as _common_pb2
from talon_client.proto.resources import routing_pb2 as _routing_pb2
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class Route(_message.Message):
    __slots__ = ("connector_uid", "connector", "consumer")
    CONNECTOR_UID_FIELD_NUMBER: _ClassVar[int]
    CONNECTOR_FIELD_NUMBER: _ClassVar[int]
    CONSUMER_FIELD_NUMBER: _ClassVar[int]
    connector_uid: str
    connector: _common_pb2.ResourceRef
    consumer: _routing_pb2.MessageConsumer
    def __init__(self, connector_uid: _Optional[str] = ..., connector: _Optional[_Union[_common_pb2.ResourceRef, _Mapping]] = ..., consumer: _Optional[_Union[_routing_pb2.MessageConsumer, _Mapping]] = ...) -> None: ...
