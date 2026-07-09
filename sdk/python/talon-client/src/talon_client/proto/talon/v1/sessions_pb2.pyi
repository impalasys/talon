from talon_client.proto.data import data_pb2 as _data_pb2
from talon_client.proto import events_pb2 as _events_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class CreateSessionRequest(_message.Message):
    __slots__ = ("agent", "ns", "labels")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    agent: str
    ns: str
    labels: _containers.ScalarMap[str, str]
    def __init__(self, agent: _Optional[str] = ..., ns: _Optional[str] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class GetSessionRequest(_message.Message):
    __slots__ = ("session_id", "agent", "ns", "message_limit")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_LIMIT_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    ns: str
    message_limit: int
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ..., message_limit: _Optional[int] = ...) -> None: ...

class ListSessionMessagesRequest(_message.Message):
    __slots__ = ("session_id", "agent", "ns", "page_size", "before_message_id")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    PAGE_SIZE_FIELD_NUMBER: _ClassVar[int]
    BEFORE_MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    ns: str
    page_size: int
    before_message_id: str
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ..., page_size: _Optional[int] = ..., before_message_id: _Optional[str] = ...) -> None: ...

class ListSessionMessagesResponseItem(_message.Message):
    __slots__ = ("message",)
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    message: _data_pb2.SessionMessage
    def __init__(self, message: _Optional[_Union[_data_pb2.SessionMessage, _Mapping]] = ...) -> None: ...

class ListSessionMessagesResponse(_message.Message):
    __slots__ = ("session_id", "agent", "state", "items", "has_more", "next_before_message_id")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    STATE_FIELD_NUMBER: _ClassVar[int]
    ITEMS_FIELD_NUMBER: _ClassVar[int]
    HAS_MORE_FIELD_NUMBER: _ClassVar[int]
    NEXT_BEFORE_MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    state: str
    items: _containers.RepeatedCompositeFieldContainer[ListSessionMessagesResponseItem]
    has_more: bool
    next_before_message_id: str
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., state: _Optional[str] = ..., items: _Optional[_Iterable[_Union[ListSessionMessagesResponseItem, _Mapping]]] = ..., has_more: bool = ..., next_before_message_id: _Optional[str] = ...) -> None: ...

class ListSessionsRequest(_message.Message):
    __slots__ = ("agent", "ns")
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    agent: str
    ns: str
    def __init__(self, agent: _Optional[str] = ..., ns: _Optional[str] = ...) -> None: ...

class SessionListItem(_message.Message):
    __slots__ = ("session_id", "updated_at", "labels")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    UPDATED_AT_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    updated_at: int
    labels: _containers.ScalarMap[str, str]
    def __init__(self, session_id: _Optional[str] = ..., updated_at: _Optional[int] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class ListSessionsResponse(_message.Message):
    __slots__ = ("session_ids", "sessions")
    SESSION_IDS_FIELD_NUMBER: _ClassVar[int]
    SESSIONS_FIELD_NUMBER: _ClassVar[int]
    session_ids: _containers.RepeatedScalarFieldContainer[str]
    sessions: _containers.RepeatedCompositeFieldContainer[SessionListItem]
    def __init__(self, session_ids: _Optional[_Iterable[str]] = ..., sessions: _Optional[_Iterable[_Union[SessionListItem, _Mapping]]] = ...) -> None: ...

class SessionResponse(_message.Message):
    __slots__ = ("session_id", "agent", "state", "messages", "labels")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    STATE_FIELD_NUMBER: _ClassVar[int]
    MESSAGES_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    state: str
    messages: _containers.RepeatedCompositeFieldContainer[_data_pb2.SessionMessage]
    labels: _containers.ScalarMap[str, str]
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., state: _Optional[str] = ..., messages: _Optional[_Iterable[_Union[_data_pb2.SessionMessage, _Mapping]]] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class DeleteSessionRequest(_message.Message):
    __slots__ = ("session_id", "agent", "ns")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    ns: str
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ...) -> None: ...

class DeleteSessionResponse(_message.Message):
    __slots__ = ("success",)
    SUCCESS_FIELD_NUMBER: _ClassVar[int]
    success: bool
    def __init__(self, success: bool = ...) -> None: ...

class ClearSessionRequest(_message.Message):
    __slots__ = ("session_id", "agent", "ns")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    ns: str
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ...) -> None: ...

class ClearSessionResponse(_message.Message):
    __slots__ = ("success",)
    SUCCESS_FIELD_NUMBER: _ClassVar[int]
    success: bool
    def __init__(self, success: bool = ...) -> None: ...

class SubmitSessionTurnRequest(_message.Message):
    __slots__ = ("session_id", "agent", "ns", "message", "labels")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    ns: str
    message: _data_pb2.SessionMessage
    labels: _containers.ScalarMap[str, str]
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ..., message: _Optional[_Union[_data_pb2.SessionMessage, _Mapping]] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class SendMessageRequest(_message.Message):
    __slots__ = ("session_id", "agent", "ns", "message", "labels")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    ns: str
    message: str
    labels: _containers.ScalarMap[str, str]
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ..., message: _Optional[str] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class SendMessageResponse(_message.Message):
    __slots__ = ("reply", "session_id")
    REPLY_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    reply: str
    session_id: str
    def __init__(self, reply: _Optional[str] = ..., session_id: _Optional[str] = ...) -> None: ...

class AppendSessionMessageRequest(_message.Message):
    __slots__ = ("session_id", "agent", "ns", "message")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    ns: str
    message: _data_pb2.SessionMessage
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ..., message: _Optional[_Union[_data_pb2.SessionMessage, _Mapping]] = ...) -> None: ...

class AppendSessionMessageResponse(_message.Message):
    __slots__ = ("session_id", "message")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    message: _data_pb2.SessionMessage
    def __init__(self, session_id: _Optional[str] = ..., message: _Optional[_Union[_data_pb2.SessionMessage, _Mapping]] = ...) -> None: ...

class UpdateSessionMessageRequest(_message.Message):
    __slots__ = ("session_id", "agent", "ns", "message_id", "parts", "labels")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    PARTS_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    ns: str
    message_id: str
    parts: _containers.RepeatedCompositeFieldContainer[_data_pb2.SessionMessagePart]
    labels: _containers.ScalarMap[str, str]
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ..., message_id: _Optional[str] = ..., parts: _Optional[_Iterable[_Union[_data_pb2.SessionMessagePart, _Mapping]]] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class UpdateSessionMessageResponse(_message.Message):
    __slots__ = ("session_id", "message")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    message: _data_pb2.SessionMessage
    def __init__(self, session_id: _Optional[str] = ..., message: _Optional[_Union[_data_pb2.SessionMessage, _Mapping]] = ...) -> None: ...

class AnswerSessionPermissionRequest(_message.Message):
    __slots__ = ("session_id", "agent", "ns", "request_id", "outcome", "option_id", "decided_by")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    REQUEST_ID_FIELD_NUMBER: _ClassVar[int]
    OUTCOME_FIELD_NUMBER: _ClassVar[int]
    OPTION_ID_FIELD_NUMBER: _ClassVar[int]
    DECIDED_BY_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    ns: str
    request_id: str
    outcome: str
    option_id: str
    decided_by: str
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ..., request_id: _Optional[str] = ..., outcome: _Optional[str] = ..., option_id: _Optional[str] = ..., decided_by: _Optional[str] = ...) -> None: ...

class AnswerSessionPermissionResponse(_message.Message):
    __slots__ = ("session_id", "request_id", "outcome", "option_id")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    REQUEST_ID_FIELD_NUMBER: _ClassVar[int]
    OUTCOME_FIELD_NUMBER: _ClassVar[int]
    OPTION_ID_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    request_id: str
    outcome: str
    option_id: str
    def __init__(self, session_id: _Optional[str] = ..., request_id: _Optional[str] = ..., outcome: _Optional[str] = ..., option_id: _Optional[str] = ...) -> None: ...

class StopSessionGenerationRequest(_message.Message):
    __slots__ = ("session_id", "agent", "ns")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    ns: str
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ...) -> None: ...

class StopSessionGenerationResponse(_message.Message):
    __slots__ = ("success",)
    SUCCESS_FIELD_NUMBER: _ClassVar[int]
    success: bool
    def __init__(self, success: bool = ...) -> None: ...

class StreamSessionPartsRequest(_message.Message):
    __slots__ = ("session_id", "agent", "ns")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    ns: str
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ...) -> None: ...

class StreamSessionPartsBatchRequest(_message.Message):
    __slots__ = ("session_names",)
    SESSION_NAMES_FIELD_NUMBER: _ClassVar[int]
    session_names: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, session_names: _Optional[_Iterable[str]] = ...) -> None: ...
