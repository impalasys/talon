from talon_client.proto.data import data_pb2 as _data_pb2
from talon_client.proto import events_pb2 as _events_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class PostChannelMessageRequest(_message.Message):
    __slots__ = ("ns", "channel", "author_kind", "author", "content", "subscription_names", "labels")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    NS_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    AUTHOR_KIND_FIELD_NUMBER: _ClassVar[int]
    AUTHOR_FIELD_NUMBER: _ClassVar[int]
    CONTENT_FIELD_NUMBER: _ClassVar[int]
    SUBSCRIPTION_NAMES_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    ns: str
    channel: str
    author_kind: str
    author: str
    content: str
    subscription_names: _containers.RepeatedScalarFieldContainer[str]
    labels: _containers.ScalarMap[str, str]
    def __init__(self, ns: _Optional[str] = ..., channel: _Optional[str] = ..., author_kind: _Optional[str] = ..., author: _Optional[str] = ..., content: _Optional[str] = ..., subscription_names: _Optional[_Iterable[str]] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class RoutedChannelSession(_message.Message):
    __slots__ = ("subscription", "agent", "session_id", "error")
    SUBSCRIPTION_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    ERROR_FIELD_NUMBER: _ClassVar[int]
    subscription: str
    agent: str
    session_id: str
    error: str
    def __init__(self, subscription: _Optional[str] = ..., agent: _Optional[str] = ..., session_id: _Optional[str] = ..., error: _Optional[str] = ...) -> None: ...

class PostChannelMessageResponse(_message.Message):
    __slots__ = ("message", "routed_sessions")
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    ROUTED_SESSIONS_FIELD_NUMBER: _ClassVar[int]
    message: _data_pb2.ChannelMessage
    routed_sessions: _containers.RepeatedCompositeFieldContainer[RoutedChannelSession]
    def __init__(self, message: _Optional[_Union[_data_pb2.ChannelMessage, _Mapping]] = ..., routed_sessions: _Optional[_Iterable[_Union[RoutedChannelSession, _Mapping]]] = ...) -> None: ...

class GetChannelMessageRequest(_message.Message):
    __slots__ = ("ns", "channel", "message_id")
    NS_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    ns: str
    channel: str
    message_id: str
    def __init__(self, ns: _Optional[str] = ..., channel: _Optional[str] = ..., message_id: _Optional[str] = ...) -> None: ...

class ChannelMessageResponse(_message.Message):
    __slots__ = ("message",)
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    message: _data_pb2.ChannelMessage
    def __init__(self, message: _Optional[_Union[_data_pb2.ChannelMessage, _Mapping]] = ...) -> None: ...

class ListChannelMessagesRequest(_message.Message):
    __slots__ = ("ns", "channel", "limit", "page_size", "before_message_id")
    NS_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    LIMIT_FIELD_NUMBER: _ClassVar[int]
    PAGE_SIZE_FIELD_NUMBER: _ClassVar[int]
    BEFORE_MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    ns: str
    channel: str
    limit: int
    page_size: int
    before_message_id: str
    def __init__(self, ns: _Optional[str] = ..., channel: _Optional[str] = ..., limit: _Optional[int] = ..., page_size: _Optional[int] = ..., before_message_id: _Optional[str] = ...) -> None: ...

class ListChannelMessagesResponse(_message.Message):
    __slots__ = ("messages", "has_more", "next_before_message_id")
    MESSAGES_FIELD_NUMBER: _ClassVar[int]
    HAS_MORE_FIELD_NUMBER: _ClassVar[int]
    NEXT_BEFORE_MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    messages: _containers.RepeatedCompositeFieldContainer[_data_pb2.ChannelMessage]
    has_more: bool
    next_before_message_id: str
    def __init__(self, messages: _Optional[_Iterable[_Union[_data_pb2.ChannelMessage, _Mapping]]] = ..., has_more: bool = ..., next_before_message_id: _Optional[str] = ...) -> None: ...

class StreamChannelEventsRequest(_message.Message):
    __slots__ = ("ns", "channel")
    NS_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    ns: str
    channel: str
    def __init__(self, ns: _Optional[str] = ..., channel: _Optional[str] = ...) -> None: ...
