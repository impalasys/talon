from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class ResourceRef(_message.Message):
    __slots__ = ("namespace", "name")
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    namespace: str
    name: str
    def __init__(self, namespace: _Optional[str] = ..., name: _Optional[str] = ...) -> None: ...

class SessionMessageConsumer(_message.Message):
    __slots__ = ("agent", "session_id", "continuity")
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    CONTINUITY_FIELD_NUMBER: _ClassVar[int]
    agent: ResourceRef
    session_id: str
    continuity: str
    def __init__(self, agent: _Optional[_Union[ResourceRef, _Mapping]] = ..., session_id: _Optional[str] = ..., continuity: _Optional[str] = ...) -> None: ...

class ChannelMessageConsumer(_message.Message):
    __slots__ = ("channel", "agent", "continuity", "reply_policy")
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    CONTINUITY_FIELD_NUMBER: _ClassVar[int]
    REPLY_POLICY_FIELD_NUMBER: _ClassVar[int]
    channel: ResourceRef
    agent: ResourceRef
    continuity: str
    reply_policy: str
    def __init__(self, channel: _Optional[_Union[ResourceRef, _Mapping]] = ..., agent: _Optional[_Union[ResourceRef, _Mapping]] = ..., continuity: _Optional[str] = ..., reply_policy: _Optional[str] = ...) -> None: ...

class WorkflowMessageConsumer(_message.Message):
    __slots__ = ("workflow", "reply_mode")
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    REPLY_MODE_FIELD_NUMBER: _ClassVar[int]
    workflow: ResourceRef
    reply_mode: str
    def __init__(self, workflow: _Optional[_Union[ResourceRef, _Mapping]] = ..., reply_mode: _Optional[str] = ...) -> None: ...

class MessageConsumer(_message.Message):
    __slots__ = ("session", "channel", "workflow")
    SESSION_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    session: SessionMessageConsumer
    channel: ChannelMessageConsumer
    workflow: WorkflowMessageConsumer
    def __init__(self, session: _Optional[_Union[SessionMessageConsumer, _Mapping]] = ..., channel: _Optional[_Union[ChannelMessageConsumer, _Mapping]] = ..., workflow: _Optional[_Union[WorkflowMessageConsumer, _Mapping]] = ...) -> None: ...
