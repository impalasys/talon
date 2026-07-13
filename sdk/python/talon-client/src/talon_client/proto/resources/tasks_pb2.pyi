from talon_client.proto.resources import common_pb2 as _common_pb2
from talon_client.proto.resources import files_pb2 as _files_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class TaskPhase(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    TASK_PHASE_UNSPECIFIED: _ClassVar[TaskPhase]
    TASK_PHASE_QUEUED: _ClassVar[TaskPhase]
    TASK_PHASE_RUNNING: _ClassVar[TaskPhase]
    TASK_PHASE_BLOCKED: _ClassVar[TaskPhase]
    TASK_PHASE_NEEDS_REVIEW: _ClassVar[TaskPhase]
    TASK_PHASE_SUCCEEDED: _ClassVar[TaskPhase]
    TASK_PHASE_FAILED: _ClassVar[TaskPhase]
    TASK_PHASE_CANCELED: _ClassVar[TaskPhase]
    TASK_PHASE_EXPIRED: _ClassVar[TaskPhase]
TASK_PHASE_UNSPECIFIED: TaskPhase
TASK_PHASE_QUEUED: TaskPhase
TASK_PHASE_RUNNING: TaskPhase
TASK_PHASE_BLOCKED: TaskPhase
TASK_PHASE_NEEDS_REVIEW: TaskPhase
TASK_PHASE_SUCCEEDED: TaskPhase
TASK_PHASE_FAILED: TaskPhase
TASK_PHASE_CANCELED: TaskPhase
TASK_PHASE_EXPIRED: TaskPhase

class Task(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: TaskSpec
    status: TaskStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[TaskSpec, _Mapping]] = ..., status: _Optional[_Union[TaskStatus, _Mapping]] = ...) -> None: ...

class TaskSpec(_message.Message):
    __slots__ = ("title", "description", "type", "requester", "assignee", "execution_ref", "parent_task_name")
    TITLE_FIELD_NUMBER: _ClassVar[int]
    DESCRIPTION_FIELD_NUMBER: _ClassVar[int]
    TYPE_FIELD_NUMBER: _ClassVar[int]
    REQUESTER_FIELD_NUMBER: _ClassVar[int]
    ASSIGNEE_FIELD_NUMBER: _ClassVar[int]
    EXECUTION_REF_FIELD_NUMBER: _ClassVar[int]
    PARENT_TASK_NAME_FIELD_NUMBER: _ClassVar[int]
    title: str
    description: str
    type: str
    requester: TaskParticipant
    assignee: TaskParticipant
    execution_ref: TaskExecutionRef
    parent_task_name: str
    def __init__(self, title: _Optional[str] = ..., description: _Optional[str] = ..., type: _Optional[str] = ..., requester: _Optional[_Union[TaskParticipant, _Mapping]] = ..., assignee: _Optional[_Union[TaskParticipant, _Mapping]] = ..., execution_ref: _Optional[_Union[TaskExecutionRef, _Mapping]] = ..., parent_task_name: _Optional[str] = ...) -> None: ...

class TaskParticipant(_message.Message):
    __slots__ = ("namespace", "agent", "session_id")
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    namespace: str
    agent: str
    session_id: str
    def __init__(self, namespace: _Optional[str] = ..., agent: _Optional[str] = ..., session_id: _Optional[str] = ...) -> None: ...

class TaskExecutionRef(_message.Message):
    __slots__ = ("kind", "namespace", "agent", "session_id", "run_id")
    KIND_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    kind: str
    namespace: str
    agent: str
    session_id: str
    run_id: str
    def __init__(self, kind: _Optional[str] = ..., namespace: _Optional[str] = ..., agent: _Optional[str] = ..., session_id: _Optional[str] = ..., run_id: _Optional[str] = ...) -> None: ...

class TaskStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions", "progress_summary", "result_artifacts", "created_at", "updated_at", "completed_at", "expires_at")
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    PROGRESS_SUMMARY_FIELD_NUMBER: _ClassVar[int]
    RESULT_ARTIFACTS_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    UPDATED_AT_FIELD_NUMBER: _ClassVar[int]
    COMPLETED_AT_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: TaskPhase
    conditions: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceCondition]
    progress_summary: str
    result_artifacts: _containers.RepeatedCompositeFieldContainer[_files_pb2.FileObjectRef]
    created_at: int
    updated_at: int
    completed_at: int
    expires_at: int
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[_Union[TaskPhase, str]] = ..., conditions: _Optional[_Iterable[_Union[_common_pb2.ResourceCondition, _Mapping]]] = ..., progress_summary: _Optional[str] = ..., result_artifacts: _Optional[_Iterable[_Union[_files_pb2.FileObjectRef, _Mapping]]] = ..., created_at: _Optional[int] = ..., updated_at: _Optional[int] = ..., completed_at: _Optional[int] = ..., expires_at: _Optional[int] = ...) -> None: ...
