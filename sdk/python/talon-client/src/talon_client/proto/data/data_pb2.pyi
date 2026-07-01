from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class MessageRole(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    ROLE_UNSPECIFIED: _ClassVar[MessageRole]
    ROLE_USER: _ClassVar[MessageRole]
    ROLE_ASSISTANT: _ClassVar[MessageRole]
    ROLE_SYSTEM: _ClassVar[MessageRole]

class SessionMessagePartType(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    SESSION_MESSAGE_PART_TYPE_UNSPECIFIED: _ClassVar[SessionMessagePartType]
    SESSION_MESSAGE_PART_TYPE_TEXT: _ClassVar[SessionMessagePartType]
    SESSION_MESSAGE_PART_TYPE_REASONING: _ClassVar[SessionMessagePartType]
    SESSION_MESSAGE_PART_TYPE_TOOL_CALL: _ClassVar[SessionMessagePartType]
    SESSION_MESSAGE_PART_TYPE_TOOL_RESULT: _ClassVar[SessionMessagePartType]
    SESSION_MESSAGE_PART_TYPE_USAGE: _ClassVar[SessionMessagePartType]
    SESSION_MESSAGE_PART_TYPE_ERROR: _ClassVar[SessionMessagePartType]
    SESSION_MESSAGE_PART_TYPE_IMAGE: _ClassVar[SessionMessagePartType]
    SESSION_MESSAGE_PART_TYPE_AUDIO: _ClassVar[SessionMessagePartType]
    SESSION_MESSAGE_PART_TYPE_VIDEO: _ClassVar[SessionMessagePartType]
    SESSION_MESSAGE_PART_TYPE_FILE: _ClassVar[SessionMessagePartType]
    SESSION_MESSAGE_PART_TYPE_REQUEST_PERMISSION: _ClassVar[SessionMessagePartType]
    SESSION_MESSAGE_PART_TYPE_PERMISSION_RESULT: _ClassVar[SessionMessagePartType]
ROLE_UNSPECIFIED: MessageRole
ROLE_USER: MessageRole
ROLE_ASSISTANT: MessageRole
ROLE_SYSTEM: MessageRole
SESSION_MESSAGE_PART_TYPE_UNSPECIFIED: SessionMessagePartType
SESSION_MESSAGE_PART_TYPE_TEXT: SessionMessagePartType
SESSION_MESSAGE_PART_TYPE_REASONING: SessionMessagePartType
SESSION_MESSAGE_PART_TYPE_TOOL_CALL: SessionMessagePartType
SESSION_MESSAGE_PART_TYPE_TOOL_RESULT: SessionMessagePartType
SESSION_MESSAGE_PART_TYPE_USAGE: SessionMessagePartType
SESSION_MESSAGE_PART_TYPE_ERROR: SessionMessagePartType
SESSION_MESSAGE_PART_TYPE_IMAGE: SessionMessagePartType
SESSION_MESSAGE_PART_TYPE_AUDIO: SessionMessagePartType
SESSION_MESSAGE_PART_TYPE_VIDEO: SessionMessagePartType
SESSION_MESSAGE_PART_TYPE_FILE: SessionMessagePartType
SESSION_MESSAGE_PART_TYPE_REQUEST_PERMISSION: SessionMessagePartType
SESSION_MESSAGE_PART_TYPE_PERMISSION_RESULT: SessionMessagePartType

class ObjectRef(_message.Message):
    __slots__ = ("key", "media_type", "size_bytes", "sha256", "filename", "metadata")
    class MetadataEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    KEY_FIELD_NUMBER: _ClassVar[int]
    MEDIA_TYPE_FIELD_NUMBER: _ClassVar[int]
    SIZE_BYTES_FIELD_NUMBER: _ClassVar[int]
    SHA256_FIELD_NUMBER: _ClassVar[int]
    FILENAME_FIELD_NUMBER: _ClassVar[int]
    METADATA_FIELD_NUMBER: _ClassVar[int]
    key: str
    media_type: str
    size_bytes: int
    sha256: str
    filename: str
    metadata: _containers.ScalarMap[str, str]
    def __init__(self, key: _Optional[str] = ..., media_type: _Optional[str] = ..., size_bytes: _Optional[int] = ..., sha256: _Optional[str] = ..., filename: _Optional[str] = ..., metadata: _Optional[_Mapping[str, str]] = ...) -> None: ...

class Principal(_message.Message):
    __slots__ = ("external_id", "address", "display_name", "kind", "metadata")
    class MetadataEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    EXTERNAL_ID_FIELD_NUMBER: _ClassVar[int]
    ADDRESS_FIELD_NUMBER: _ClassVar[int]
    DISPLAY_NAME_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    METADATA_FIELD_NUMBER: _ClassVar[int]
    external_id: str
    address: str
    display_name: str
    kind: str
    metadata: _containers.ScalarMap[str, str]
    def __init__(self, external_id: _Optional[str] = ..., address: _Optional[str] = ..., display_name: _Optional[str] = ..., kind: _Optional[str] = ..., metadata: _Optional[_Mapping[str, str]] = ...) -> None: ...

class SessionMessagePart(_message.Message):
    __slots__ = ("id", "part_type", "content", "name", "payload_json", "created_at", "object")
    ID_FIELD_NUMBER: _ClassVar[int]
    PART_TYPE_FIELD_NUMBER: _ClassVar[int]
    CONTENT_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    PAYLOAD_JSON_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    OBJECT_FIELD_NUMBER: _ClassVar[int]
    id: str
    part_type: SessionMessagePartType
    content: str
    name: str
    payload_json: str
    created_at: int
    object: ObjectRef
    def __init__(self, id: _Optional[str] = ..., part_type: _Optional[_Union[SessionMessagePartType, str]] = ..., content: _Optional[str] = ..., name: _Optional[str] = ..., payload_json: _Optional[str] = ..., created_at: _Optional[int] = ..., object: _Optional[_Union[ObjectRef, _Mapping]] = ...) -> None: ...

class SessionMessage(_message.Message):
    __slots__ = ("id", "role", "created_at", "labels", "parts")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    ID_FIELD_NUMBER: _ClassVar[int]
    ROLE_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    PARTS_FIELD_NUMBER: _ClassVar[int]
    id: str
    role: MessageRole
    created_at: int
    labels: _containers.ScalarMap[str, str]
    parts: _containers.RepeatedCompositeFieldContainer[SessionMessagePart]
    def __init__(self, id: _Optional[str] = ..., role: _Optional[_Union[MessageRole, str]] = ..., created_at: _Optional[int] = ..., labels: _Optional[_Mapping[str, str]] = ..., parts: _Optional[_Iterable[_Union[SessionMessagePart, _Mapping]]] = ...) -> None: ...

class Session(_message.Message):
    __slots__ = ("id", "agent", "ns", "status", "created_at", "last_active", "metadata", "labels")
    class MetadataEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    LAST_ACTIVE_FIELD_NUMBER: _ClassVar[int]
    METADATA_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    id: str
    agent: str
    ns: str
    status: str
    created_at: int
    last_active: int
    metadata: _containers.ScalarMap[str, str]
    labels: _containers.ScalarMap[str, str]
    def __init__(self, id: _Optional[str] = ..., agent: _Optional[str] = ..., ns: _Optional[str] = ..., status: _Optional[str] = ..., created_at: _Optional[int] = ..., last_active: _Optional[int] = ..., metadata: _Optional[_Mapping[str, str]] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class ChannelMessage(_message.Message):
    __slots__ = ("id", "ns", "channel", "author_kind", "author", "content", "created_at", "source_agent", "source_session_id", "labels")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    ID_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    AUTHOR_KIND_FIELD_NUMBER: _ClassVar[int]
    AUTHOR_FIELD_NUMBER: _ClassVar[int]
    CONTENT_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    SOURCE_AGENT_FIELD_NUMBER: _ClassVar[int]
    SOURCE_SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    id: str
    ns: str
    channel: str
    author_kind: str
    author: str
    content: str
    created_at: int
    source_agent: str
    source_session_id: str
    labels: _containers.ScalarMap[str, str]
    def __init__(self, id: _Optional[str] = ..., ns: _Optional[str] = ..., channel: _Optional[str] = ..., author_kind: _Optional[str] = ..., author: _Optional[str] = ..., content: _Optional[str] = ..., created_at: _Optional[int] = ..., source_agent: _Optional[str] = ..., source_session_id: _Optional[str] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class Knowledge(_message.Message):
    __slots__ = ("path", "content", "updated_at", "namespace", "name")
    PATH_FIELD_NUMBER: _ClassVar[int]
    CONTENT_FIELD_NUMBER: _ClassVar[int]
    UPDATED_AT_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    path: str
    content: str
    updated_at: int
    namespace: str
    name: str
    def __init__(self, path: _Optional[str] = ..., content: _Optional[str] = ..., updated_at: _Optional[int] = ..., namespace: _Optional[str] = ..., name: _Optional[str] = ...) -> None: ...

class KnowledgeSearchResult(_message.Message):
    __slots__ = ("path", "snippet", "score", "timestamp", "namespace")
    PATH_FIELD_NUMBER: _ClassVar[int]
    SNIPPET_FIELD_NUMBER: _ClassVar[int]
    SCORE_FIELD_NUMBER: _ClassVar[int]
    TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    path: str
    snippet: str
    score: float
    timestamp: int
    namespace: str
    def __init__(self, path: _Optional[str] = ..., snippet: _Optional[str] = ..., score: _Optional[float] = ..., timestamp: _Optional[int] = ..., namespace: _Optional[str] = ...) -> None: ...

class WorkflowRun(_message.Message):
    __slots__ = ("id", "workflow", "ns", "status", "input_json", "state_json", "output_json", "created_at", "updated_at", "labels", "claim_expires_at", "error", "spec_json", "workflow_revision", "claim_owner", "claim_attempt", "last_dispatch_reason")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    ID_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    INPUT_JSON_FIELD_NUMBER: _ClassVar[int]
    STATE_JSON_FIELD_NUMBER: _ClassVar[int]
    OUTPUT_JSON_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    UPDATED_AT_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    CLAIM_EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    ERROR_FIELD_NUMBER: _ClassVar[int]
    SPEC_JSON_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_REVISION_FIELD_NUMBER: _ClassVar[int]
    CLAIM_OWNER_FIELD_NUMBER: _ClassVar[int]
    CLAIM_ATTEMPT_FIELD_NUMBER: _ClassVar[int]
    LAST_DISPATCH_REASON_FIELD_NUMBER: _ClassVar[int]
    id: str
    workflow: str
    ns: str
    status: str
    input_json: str
    state_json: str
    output_json: str
    created_at: int
    updated_at: int
    labels: _containers.ScalarMap[str, str]
    claim_expires_at: int
    error: str
    spec_json: str
    workflow_revision: int
    claim_owner: str
    claim_attempt: int
    last_dispatch_reason: str
    def __init__(self, id: _Optional[str] = ..., workflow: _Optional[str] = ..., ns: _Optional[str] = ..., status: _Optional[str] = ..., input_json: _Optional[str] = ..., state_json: _Optional[str] = ..., output_json: _Optional[str] = ..., created_at: _Optional[int] = ..., updated_at: _Optional[int] = ..., labels: _Optional[_Mapping[str, str]] = ..., claim_expires_at: _Optional[int] = ..., error: _Optional[str] = ..., spec_json: _Optional[str] = ..., workflow_revision: _Optional[int] = ..., claim_owner: _Optional[str] = ..., claim_attempt: _Optional[int] = ..., last_dispatch_reason: _Optional[str] = ...) -> None: ...

class WorkflowStepRun(_message.Message):
    __slots__ = ("id", "step_id", "attempt", "status", "input_json", "output_json", "error", "child_session_id", "child_workflow_run_id", "resume_json", "suspend_json", "created_at", "updated_at", "next_retry_at", "timeout_at", "wait_wakeup_handle", "wait_until_at")
    ID_FIELD_NUMBER: _ClassVar[int]
    STEP_ID_FIELD_NUMBER: _ClassVar[int]
    ATTEMPT_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    INPUT_JSON_FIELD_NUMBER: _ClassVar[int]
    OUTPUT_JSON_FIELD_NUMBER: _ClassVar[int]
    ERROR_FIELD_NUMBER: _ClassVar[int]
    CHILD_SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    CHILD_WORKFLOW_RUN_ID_FIELD_NUMBER: _ClassVar[int]
    RESUME_JSON_FIELD_NUMBER: _ClassVar[int]
    SUSPEND_JSON_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    UPDATED_AT_FIELD_NUMBER: _ClassVar[int]
    NEXT_RETRY_AT_FIELD_NUMBER: _ClassVar[int]
    TIMEOUT_AT_FIELD_NUMBER: _ClassVar[int]
    WAIT_WAKEUP_HANDLE_FIELD_NUMBER: _ClassVar[int]
    WAIT_UNTIL_AT_FIELD_NUMBER: _ClassVar[int]
    id: str
    step_id: str
    attempt: int
    status: str
    input_json: str
    output_json: str
    error: str
    child_session_id: str
    child_workflow_run_id: str
    resume_json: str
    suspend_json: str
    created_at: int
    updated_at: int
    next_retry_at: int
    timeout_at: int
    wait_wakeup_handle: str
    wait_until_at: int
    def __init__(self, id: _Optional[str] = ..., step_id: _Optional[str] = ..., attempt: _Optional[int] = ..., status: _Optional[str] = ..., input_json: _Optional[str] = ..., output_json: _Optional[str] = ..., error: _Optional[str] = ..., child_session_id: _Optional[str] = ..., child_workflow_run_id: _Optional[str] = ..., resume_json: _Optional[str] = ..., suspend_json: _Optional[str] = ..., created_at: _Optional[int] = ..., updated_at: _Optional[int] = ..., next_retry_at: _Optional[int] = ..., timeout_at: _Optional[int] = ..., wait_wakeup_handle: _Optional[str] = ..., wait_until_at: _Optional[int] = ...) -> None: ...

class WorkflowRunEvent(_message.Message):
    __slots__ = ("id", "ns", "workflow", "run_id", "type", "step_id", "message", "payload_json", "timestamp")
    ID_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    TYPE_FIELD_NUMBER: _ClassVar[int]
    STEP_ID_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    PAYLOAD_JSON_FIELD_NUMBER: _ClassVar[int]
    TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    id: str
    ns: str
    workflow: str
    run_id: str
    type: str
    step_id: str
    message: str
    payload_json: str
    timestamp: int
    def __init__(self, id: _Optional[str] = ..., ns: _Optional[str] = ..., workflow: _Optional[str] = ..., run_id: _Optional[str] = ..., type: _Optional[str] = ..., step_id: _Optional[str] = ..., message: _Optional[str] = ..., payload_json: _Optional[str] = ..., timestamp: _Optional[int] = ...) -> None: ...
