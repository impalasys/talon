from talon_client.proto.resources import common_pb2 as _common_pb2
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class SessionMessageConsumer(_message.Message):
    __slots__ = ("agent", "continuity")
    AGENT_FIELD_NUMBER: _ClassVar[int]
    CONTINUITY_FIELD_NUMBER: _ClassVar[int]
    agent: _common_pb2.ResourceRef
    continuity: str
    def __init__(self, agent: _Optional[_Union[_common_pb2.ResourceRef, _Mapping]] = ..., continuity: _Optional[str] = ...) -> None: ...

class ChannelMessageConsumer(_message.Message):
    __slots__ = ("channel", "agent", "continuity", "reply_policy")
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    CONTINUITY_FIELD_NUMBER: _ClassVar[int]
    REPLY_POLICY_FIELD_NUMBER: _ClassVar[int]
    channel: _common_pb2.ResourceRef
    agent: _common_pb2.ResourceRef
    continuity: str
    reply_policy: str
    def __init__(self, channel: _Optional[_Union[_common_pb2.ResourceRef, _Mapping]] = ..., agent: _Optional[_Union[_common_pb2.ResourceRef, _Mapping]] = ..., continuity: _Optional[str] = ..., reply_policy: _Optional[str] = ...) -> None: ...

class MessageConsumer(_message.Message):
    __slots__ = ("session", "channel")
    SESSION_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    session: SessionMessageConsumer
    channel: ChannelMessageConsumer
    def __init__(self, session: _Optional[_Union[SessionMessageConsumer, _Mapping]] = ..., channel: _Optional[_Union[ChannelMessageConsumer, _Mapping]] = ...) -> None: ...
