from talon_client.proto.data import data_pb2 as _data_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class SystemAction(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    SYSTEM_ACTION_UNSPECIFIED: _ClassVar[SystemAction]
    SYSTEM_ACTION_CREATE: _ClassVar[SystemAction]
    SYSTEM_ACTION_UPDATE: _ClassVar[SystemAction]
    SYSTEM_ACTION_DELETE: _ClassVar[SystemAction]
    SYSTEM_ACTION_SUSPEND: _ClassVar[SystemAction]
    SYSTEM_ACTION_RESUME: _ClassVar[SystemAction]

class MessageDirection(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    MESSAGE_DIRECTION_UNSPECIFIED: _ClassVar[MessageDirection]
    MESSAGE_DIRECTION_INBOUND: _ClassVar[MessageDirection]
    MESSAGE_DIRECTION_OUTBOUND: _ClassVar[MessageDirection]

class SessionMessagePartEventKind(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    SESSION_MESSAGE_PART_EVENT_KIND_UNSPECIFIED: _ClassVar[SessionMessagePartEventKind]
    SESSION_MESSAGE_PART_EVENT_KIND_DELTA: _ClassVar[SessionMessagePartEventKind]
    SESSION_MESSAGE_PART_EVENT_KIND_DONE: _ClassVar[SessionMessagePartEventKind]
    SESSION_MESSAGE_PART_EVENT_KIND_ERROR: _ClassVar[SessionMessagePartEventKind]

class ChannelEventKind(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    CHANNEL_EVENT_KIND_UNSPECIFIED: _ClassVar[ChannelEventKind]
    CHANNEL_EVENT_KIND_MESSAGE_CREATED: _ClassVar[ChannelEventKind]
    CHANNEL_EVENT_KIND_SESSION_ROUTED: _ClassVar[ChannelEventKind]
    CHANNEL_EVENT_KIND_PUBLISH_SKIPPED: _ClassVar[ChannelEventKind]
    CHANNEL_EVENT_KIND_ERROR: _ClassVar[ChannelEventKind]

class ResourceChangeType(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    RESOURCE_CHANGE_TYPE_UNSPECIFIED: _ClassVar[ResourceChangeType]
    RESOURCE_CHANGE_TYPE_CREATED: _ClassVar[ResourceChangeType]
    RESOURCE_CHANGE_TYPE_UPDATED: _ClassVar[ResourceChangeType]
    RESOURCE_CHANGE_TYPE_DELETED: _ClassVar[ResourceChangeType]

class IndexOperation(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    INDEX_OPERATION_UNSPECIFIED: _ClassVar[IndexOperation]
    INDEX_OPERATION_UPSERT: _ClassVar[IndexOperation]
    INDEX_OPERATION_DELETE: _ClassVar[IndexOperation]
SYSTEM_ACTION_UNSPECIFIED: SystemAction
SYSTEM_ACTION_CREATE: SystemAction
SYSTEM_ACTION_UPDATE: SystemAction
SYSTEM_ACTION_DELETE: SystemAction
SYSTEM_ACTION_SUSPEND: SystemAction
SYSTEM_ACTION_RESUME: SystemAction
MESSAGE_DIRECTION_UNSPECIFIED: MessageDirection
MESSAGE_DIRECTION_INBOUND: MessageDirection
MESSAGE_DIRECTION_OUTBOUND: MessageDirection
SESSION_MESSAGE_PART_EVENT_KIND_UNSPECIFIED: SessionMessagePartEventKind
SESSION_MESSAGE_PART_EVENT_KIND_DELTA: SessionMessagePartEventKind
SESSION_MESSAGE_PART_EVENT_KIND_DONE: SessionMessagePartEventKind
SESSION_MESSAGE_PART_EVENT_KIND_ERROR: SessionMessagePartEventKind
CHANNEL_EVENT_KIND_UNSPECIFIED: ChannelEventKind
CHANNEL_EVENT_KIND_MESSAGE_CREATED: ChannelEventKind
CHANNEL_EVENT_KIND_SESSION_ROUTED: ChannelEventKind
CHANNEL_EVENT_KIND_PUBLISH_SKIPPED: ChannelEventKind
CHANNEL_EVENT_KIND_ERROR: ChannelEventKind
RESOURCE_CHANGE_TYPE_UNSPECIFIED: ResourceChangeType
RESOURCE_CHANGE_TYPE_CREATED: ResourceChangeType
RESOURCE_CHANGE_TYPE_UPDATED: ResourceChangeType
RESOURCE_CHANGE_TYPE_DELETED: ResourceChangeType
INDEX_OPERATION_UNSPECIFIED: IndexOperation
INDEX_OPERATION_UPSERT: IndexOperation
INDEX_OPERATION_DELETE: IndexOperation

class LifecycleEvent(_message.Message):
    __slots__ = ("resource_type", "name", "ns", "action", "timestamp")
    RESOURCE_TYPE_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    ACTION_FIELD_NUMBER: _ClassVar[int]
    TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    resource_type: str
    name: str
    ns: str
    action: SystemAction
    timestamp: int
    def __init__(self, resource_type: _Optional[str] = ..., name: _Optional[str] = ..., ns: _Optional[str] = ..., action: _Optional[_Union[SystemAction, str]] = ..., timestamp: _Optional[int] = ...) -> None: ...

class SessionMessageEvent(_message.Message):
    __slots__ = ("session_id", "message_id", "direction", "timestamp", "agent", "message", "ns", "submission_id")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    DIRECTION_FIELD_NUMBER: _ClassVar[int]
    TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    SUBMISSION_ID_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    message_id: str
    direction: MessageDirection
    timestamp: int
    agent: str
    message: str
    ns: str
    submission_id: str
    def __init__(self, session_id: _Optional[str] = ..., message_id: _Optional[str] = ..., direction: _Optional[_Union[MessageDirection, str]] = ..., timestamp: _Optional[int] = ..., agent: _Optional[str] = ..., message: _Optional[str] = ..., ns: _Optional[str] = ..., submission_id: _Optional[str] = ...) -> None: ...

class SessionControlEvent(_message.Message):
    __slots__ = ("session_id", "agent", "ns", "action", "timestamp")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    ACTION_FIELD_NUMBER: _ClassVar[int]
    TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    agent: str
    ns: str
    action: str
    timestamp: int
    def __init__(self, session_id: _Optional[str] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ..., action: _Optional[str] = ..., timestamp: _Optional[int] = ...) -> None: ...

class SessionMessagePartEvent(_message.Message):
    __slots__ = ("session_id", "kind", "part", "timestamp", "agent", "ns", "message_id")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    PART_FIELD_NUMBER: _ClassVar[int]
    TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    kind: SessionMessagePartEventKind
    part: _data_pb2.SessionMessagePart
    timestamp: int
    agent: str
    ns: str
    message_id: str
    def __init__(self, session_id: _Optional[str] = ..., kind: _Optional[_Union[SessionMessagePartEventKind, str]] = ..., part: _Optional[_Union[_data_pb2.SessionMessagePart, _Mapping]] = ..., timestamp: _Optional[int] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ..., message_id: _Optional[str] = ...) -> None: ...

class ChannelEvent(_message.Message):
    __slots__ = ("ns", "channel", "kind", "message", "session_id", "agent", "subscription", "error", "timestamp")
    NS_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SUBSCRIPTION_FIELD_NUMBER: _ClassVar[int]
    ERROR_FIELD_NUMBER: _ClassVar[int]
    TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    ns: str
    channel: str
    kind: ChannelEventKind
    message: _data_pb2.ChannelMessage
    session_id: str
    agent: str
    subscription: str
    error: str
    timestamp: int
    def __init__(self, ns: _Optional[str] = ..., channel: _Optional[str] = ..., kind: _Optional[_Union[ChannelEventKind, str]] = ..., message: _Optional[_Union[_data_pb2.ChannelMessage, _Mapping]] = ..., session_id: _Optional[str] = ..., agent: _Optional[str] = ..., subscription: _Optional[str] = ..., error: _Optional[str] = ..., timestamp: _Optional[int] = ...) -> None: ...

class WorkflowDispatchEvent(_message.Message):
    __slots__ = ("ns", "workflow", "run_id", "reason", "step_id", "child_session_id", "timestamp")
    NS_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    REASON_FIELD_NUMBER: _ClassVar[int]
    STEP_ID_FIELD_NUMBER: _ClassVar[int]
    CHILD_SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    ns: str
    workflow: str
    run_id: str
    reason: str
    step_id: str
    child_session_id: str
    timestamp: int
    def __init__(self, ns: _Optional[str] = ..., workflow: _Optional[str] = ..., run_id: _Optional[str] = ..., reason: _Optional[str] = ..., step_id: _Optional[str] = ..., child_session_id: _Optional[str] = ..., timestamp: _Optional[int] = ...) -> None: ...

class ResourceChangedEvent(_message.Message):
    __slots__ = ("namespace", "resource_kind", "name", "uid", "resource_version", "generation", "change_type", "changed_sections", "timestamp")
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    RESOURCE_KIND_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    UID_FIELD_NUMBER: _ClassVar[int]
    RESOURCE_VERSION_FIELD_NUMBER: _ClassVar[int]
    GENERATION_FIELD_NUMBER: _ClassVar[int]
    CHANGE_TYPE_FIELD_NUMBER: _ClassVar[int]
    CHANGED_SECTIONS_FIELD_NUMBER: _ClassVar[int]
    TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    namespace: str
    resource_kind: str
    name: str
    uid: str
    resource_version: str
    generation: int
    change_type: ResourceChangeType
    changed_sections: _containers.RepeatedScalarFieldContainer[str]
    timestamp: int
    def __init__(self, namespace: _Optional[str] = ..., resource_kind: _Optional[str] = ..., name: _Optional[str] = ..., uid: _Optional[str] = ..., resource_version: _Optional[str] = ..., generation: _Optional[int] = ..., change_type: _Optional[_Union[ResourceChangeType, str]] = ..., changed_sections: _Optional[_Iterable[str]] = ..., timestamp: _Optional[int] = ...) -> None: ...

class IndexEvent(_message.Message):
    __slots__ = ("id", "operation", "created_at", "updated_at", "resource", "session_message", "session")
    ID_FIELD_NUMBER: _ClassVar[int]
    OPERATION_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    UPDATED_AT_FIELD_NUMBER: _ClassVar[int]
    RESOURCE_FIELD_NUMBER: _ClassVar[int]
    SESSION_MESSAGE_FIELD_NUMBER: _ClassVar[int]
    SESSION_FIELD_NUMBER: _ClassVar[int]
    id: str
    operation: IndexOperation
    created_at: int
    updated_at: int
    resource: IndexResourceTarget
    session_message: IndexSessionMessageTarget
    session: IndexSessionTarget
    def __init__(self, id: _Optional[str] = ..., operation: _Optional[_Union[IndexOperation, str]] = ..., created_at: _Optional[int] = ..., updated_at: _Optional[int] = ..., resource: _Optional[_Union[IndexResourceTarget, _Mapping]] = ..., session_message: _Optional[_Union[IndexSessionMessageTarget, _Mapping]] = ..., session: _Optional[_Union[IndexSessionTarget, _Mapping]] = ...) -> None: ...

class IndexResourceTarget(_message.Message):
    __slots__ = ("resource_key", "source_generation")
    RESOURCE_KEY_FIELD_NUMBER: _ClassVar[int]
    SOURCE_GENERATION_FIELD_NUMBER: _ClassVar[int]
    resource_key: str
    source_generation: int
    def __init__(self, resource_key: _Optional[str] = ..., source_generation: _Optional[int] = ...) -> None: ...

class IndexSessionMessageTarget(_message.Message):
    __slots__ = ("namespace", "agent", "session_id", "message_id", "source_generation")
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    SOURCE_GENERATION_FIELD_NUMBER: _ClassVar[int]
    namespace: str
    agent: str
    session_id: str
    message_id: str
    source_generation: int
    def __init__(self, namespace: _Optional[str] = ..., agent: _Optional[str] = ..., session_id: _Optional[str] = ..., message_id: _Optional[str] = ..., source_generation: _Optional[int] = ...) -> None: ...

class IndexSessionTarget(_message.Message):
    __slots__ = ("namespace", "agent", "session_id")
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    namespace: str
    agent: str
    session_id: str
    def __init__(self, namespace: _Optional[str] = ..., agent: _Optional[str] = ..., session_id: _Optional[str] = ...) -> None: ...
