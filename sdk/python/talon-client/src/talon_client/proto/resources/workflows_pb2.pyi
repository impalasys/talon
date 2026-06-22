from talon_client.proto.resources import common_pb2 as _common_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class WorkflowStepOutputPolicy(_message.Message):
    __slots__ = ("format", "schema_json")
    FORMAT_FIELD_NUMBER: _ClassVar[int]
    SCHEMA_JSON_FIELD_NUMBER: _ClassVar[int]
    format: str
    schema_json: str
    def __init__(self, format: _Optional[str] = ..., schema_json: _Optional[str] = ...) -> None: ...

class WorkflowStepRetryPolicy(_message.Message):
    __slots__ = ("max_attempts", "initial_backoff_seconds", "max_backoff_seconds", "multiplier")
    MAX_ATTEMPTS_FIELD_NUMBER: _ClassVar[int]
    INITIAL_BACKOFF_SECONDS_FIELD_NUMBER: _ClassVar[int]
    MAX_BACKOFF_SECONDS_FIELD_NUMBER: _ClassVar[int]
    MULTIPLIER_FIELD_NUMBER: _ClassVar[int]
    max_attempts: int
    initial_backoff_seconds: int
    max_backoff_seconds: int
    multiplier: float
    def __init__(self, max_attempts: _Optional[int] = ..., initial_backoff_seconds: _Optional[int] = ..., max_backoff_seconds: _Optional[int] = ..., multiplier: _Optional[float] = ...) -> None: ...

class WorkflowStep(_message.Message):
    __slots__ = ("id", "type", "after", "when_json", "agent", "prompt", "tool", "input_json", "workflow", "output", "resume_schema_json", "retry", "timeout", "wait_duration", "wait_until")
    ID_FIELD_NUMBER: _ClassVar[int]
    TYPE_FIELD_NUMBER: _ClassVar[int]
    AFTER_FIELD_NUMBER: _ClassVar[int]
    WHEN_JSON_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    PROMPT_FIELD_NUMBER: _ClassVar[int]
    TOOL_FIELD_NUMBER: _ClassVar[int]
    INPUT_JSON_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    OUTPUT_FIELD_NUMBER: _ClassVar[int]
    RESUME_SCHEMA_JSON_FIELD_NUMBER: _ClassVar[int]
    RETRY_FIELD_NUMBER: _ClassVar[int]
    TIMEOUT_FIELD_NUMBER: _ClassVar[int]
    WAIT_DURATION_FIELD_NUMBER: _ClassVar[int]
    WAIT_UNTIL_FIELD_NUMBER: _ClassVar[int]
    id: str
    type: str
    after: _containers.RepeatedScalarFieldContainer[str]
    when_json: str
    agent: str
    prompt: str
    tool: str
    input_json: str
    workflow: str
    output: WorkflowStepOutputPolicy
    resume_schema_json: str
    retry: WorkflowStepRetryPolicy
    timeout: str
    wait_duration: str
    wait_until: str
    def __init__(self, id: _Optional[str] = ..., type: _Optional[str] = ..., after: _Optional[_Iterable[str]] = ..., when_json: _Optional[str] = ..., agent: _Optional[str] = ..., prompt: _Optional[str] = ..., tool: _Optional[str] = ..., input_json: _Optional[str] = ..., workflow: _Optional[str] = ..., output: _Optional[_Union[WorkflowStepOutputPolicy, _Mapping]] = ..., resume_schema_json: _Optional[str] = ..., retry: _Optional[_Union[WorkflowStepRetryPolicy, _Mapping]] = ..., timeout: _Optional[str] = ..., wait_duration: _Optional[str] = ..., wait_until: _Optional[str] = ...) -> None: ...

class WorkflowSpec(_message.Message):
    __slots__ = ("description", "input_schema_json", "output_schema_json", "steps", "output_json", "concurrency")
    DESCRIPTION_FIELD_NUMBER: _ClassVar[int]
    INPUT_SCHEMA_JSON_FIELD_NUMBER: _ClassVar[int]
    OUTPUT_SCHEMA_JSON_FIELD_NUMBER: _ClassVar[int]
    STEPS_FIELD_NUMBER: _ClassVar[int]
    OUTPUT_JSON_FIELD_NUMBER: _ClassVar[int]
    CONCURRENCY_FIELD_NUMBER: _ClassVar[int]
    description: str
    input_schema_json: str
    output_schema_json: str
    steps: _containers.RepeatedCompositeFieldContainer[WorkflowStep]
    output_json: str
    concurrency: int
    def __init__(self, description: _Optional[str] = ..., input_schema_json: _Optional[str] = ..., output_schema_json: _Optional[str] = ..., steps: _Optional[_Iterable[_Union[WorkflowStep, _Mapping]]] = ..., output_json: _Optional[str] = ..., concurrency: _Optional[int] = ...) -> None: ...

class Workflow(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: WorkflowSpec
    status: WorkflowStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[WorkflowSpec, _Mapping]] = ..., status: _Optional[_Union[WorkflowStatus, _Mapping]] = ...) -> None: ...

class WorkflowStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions")
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: str
    conditions: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceCondition]
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[str] = ..., conditions: _Optional[_Iterable[_Union[_common_pb2.ResourceCondition, _Mapping]]] = ...) -> None: ...
