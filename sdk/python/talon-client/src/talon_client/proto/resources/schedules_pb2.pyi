from talon_client.proto.resources import common_pb2 as _common_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class ScheduleTarget(_message.Message):
    __slots__ = ("agent", "session_mode", "session_id", "workflow")
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_MODE_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    agent: str
    session_mode: str
    session_id: str
    workflow: str
    def __init__(self, agent: _Optional[str] = ..., session_mode: _Optional[str] = ..., session_id: _Optional[str] = ..., workflow: _Optional[str] = ...) -> None: ...

class ScheduleSpec(_message.Message):
    __slots__ = ("kind", "cron", "interval_seconds", "run_at", "timezone", "target", "input_message", "enabled", "input_json")
    KIND_FIELD_NUMBER: _ClassVar[int]
    CRON_FIELD_NUMBER: _ClassVar[int]
    INTERVAL_SECONDS_FIELD_NUMBER: _ClassVar[int]
    RUN_AT_FIELD_NUMBER: _ClassVar[int]
    TIMEZONE_FIELD_NUMBER: _ClassVar[int]
    TARGET_FIELD_NUMBER: _ClassVar[int]
    INPUT_MESSAGE_FIELD_NUMBER: _ClassVar[int]
    ENABLED_FIELD_NUMBER: _ClassVar[int]
    INPUT_JSON_FIELD_NUMBER: _ClassVar[int]
    kind: str
    cron: str
    interval_seconds: int
    run_at: str
    timezone: str
    target: ScheduleTarget
    input_message: str
    enabled: bool
    input_json: str
    def __init__(self, kind: _Optional[str] = ..., cron: _Optional[str] = ..., interval_seconds: _Optional[int] = ..., run_at: _Optional[str] = ..., timezone: _Optional[str] = ..., target: _Optional[_Union[ScheduleTarget, _Mapping]] = ..., input_message: _Optional[str] = ..., enabled: bool = ..., input_json: _Optional[str] = ...) -> None: ...

class ScheduleStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions", "revision", "next_run_at", "backend_handle", "backend_armed", "last_run_at", "last_session_id", "last_error", "claimed_run_at", "claim_expires_at", "recent_events")
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    REVISION_FIELD_NUMBER: _ClassVar[int]
    NEXT_RUN_AT_FIELD_NUMBER: _ClassVar[int]
    BACKEND_HANDLE_FIELD_NUMBER: _ClassVar[int]
    BACKEND_ARMED_FIELD_NUMBER: _ClassVar[int]
    LAST_RUN_AT_FIELD_NUMBER: _ClassVar[int]
    LAST_SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    LAST_ERROR_FIELD_NUMBER: _ClassVar[int]
    CLAIMED_RUN_AT_FIELD_NUMBER: _ClassVar[int]
    CLAIM_EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    RECENT_EVENTS_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: str
    conditions: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceCondition]
    revision: int
    next_run_at: int
    backend_handle: str
    backend_armed: bool
    last_run_at: int
    last_session_id: str
    last_error: str
    claimed_run_at: int
    claim_expires_at: int
    recent_events: _containers.RepeatedCompositeFieldContainer[ScheduleEvent]
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[str] = ..., conditions: _Optional[_Iterable[_Union[_common_pb2.ResourceCondition, _Mapping]]] = ..., revision: _Optional[int] = ..., next_run_at: _Optional[int] = ..., backend_handle: _Optional[str] = ..., backend_armed: bool = ..., last_run_at: _Optional[int] = ..., last_session_id: _Optional[str] = ..., last_error: _Optional[str] = ..., claimed_run_at: _Optional[int] = ..., claim_expires_at: _Optional[int] = ..., recent_events: _Optional[_Iterable[_Union[ScheduleEvent, _Mapping]]] = ...) -> None: ...

class ScheduleEvent(_message.Message):
    __slots__ = ("timestamp", "phase", "outcome", "detail")
    TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    OUTCOME_FIELD_NUMBER: _ClassVar[int]
    DETAIL_FIELD_NUMBER: _ClassVar[int]
    timestamp: int
    phase: str
    outcome: str
    detail: str
    def __init__(self, timestamp: _Optional[int] = ..., phase: _Optional[str] = ..., outcome: _Optional[str] = ..., detail: _Optional[str] = ...) -> None: ...

class Schedule(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: ScheduleSpec
    status: ScheduleStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[ScheduleSpec, _Mapping]] = ..., status: _Optional[_Union[ScheduleStatus, _Mapping]] = ...) -> None: ...
