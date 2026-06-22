from talon_client.proto.data import data_pb2 as _data_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class CreateWorkflowRunRequest(_message.Message):
    __slots__ = ("ns", "workflow", "input_json", "labels")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    NS_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    INPUT_JSON_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    ns: str
    workflow: str
    input_json: str
    labels: _containers.ScalarMap[str, str]
    def __init__(self, ns: _Optional[str] = ..., workflow: _Optional[str] = ..., input_json: _Optional[str] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class GetWorkflowRunRequest(_message.Message):
    __slots__ = ("ns", "workflow", "run_id")
    NS_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    ns: str
    workflow: str
    run_id: str
    def __init__(self, ns: _Optional[str] = ..., workflow: _Optional[str] = ..., run_id: _Optional[str] = ...) -> None: ...

class ListWorkflowRunsRequest(_message.Message):
    __slots__ = ("ns", "workflow", "page_size", "before_run_id")
    NS_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    PAGE_SIZE_FIELD_NUMBER: _ClassVar[int]
    BEFORE_RUN_ID_FIELD_NUMBER: _ClassVar[int]
    ns: str
    workflow: str
    page_size: int
    before_run_id: str
    def __init__(self, ns: _Optional[str] = ..., workflow: _Optional[str] = ..., page_size: _Optional[int] = ..., before_run_id: _Optional[str] = ...) -> None: ...

class ResumeWorkflowRunRequest(_message.Message):
    __slots__ = ("ns", "workflow", "run_id", "step_id", "resume_json")
    NS_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    STEP_ID_FIELD_NUMBER: _ClassVar[int]
    RESUME_JSON_FIELD_NUMBER: _ClassVar[int]
    ns: str
    workflow: str
    run_id: str
    step_id: str
    resume_json: str
    def __init__(self, ns: _Optional[str] = ..., workflow: _Optional[str] = ..., run_id: _Optional[str] = ..., step_id: _Optional[str] = ..., resume_json: _Optional[str] = ...) -> None: ...

class CancelWorkflowRunRequest(_message.Message):
    __slots__ = ("ns", "workflow", "run_id")
    NS_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    ns: str
    workflow: str
    run_id: str
    def __init__(self, ns: _Optional[str] = ..., workflow: _Optional[str] = ..., run_id: _Optional[str] = ...) -> None: ...

class StreamWorkflowEventsRequest(_message.Message):
    __slots__ = ("ns", "workflow", "run_id")
    NS_FIELD_NUMBER: _ClassVar[int]
    WORKFLOW_FIELD_NUMBER: _ClassVar[int]
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    ns: str
    workflow: str
    run_id: str
    def __init__(self, ns: _Optional[str] = ..., workflow: _Optional[str] = ..., run_id: _Optional[str] = ...) -> None: ...

class WorkflowRunResponse(_message.Message):
    __slots__ = ("run", "steps")
    RUN_FIELD_NUMBER: _ClassVar[int]
    STEPS_FIELD_NUMBER: _ClassVar[int]
    run: _data_pb2.WorkflowRun
    steps: _containers.RepeatedCompositeFieldContainer[_data_pb2.WorkflowStepRun]
    def __init__(self, run: _Optional[_Union[_data_pb2.WorkflowRun, _Mapping]] = ..., steps: _Optional[_Iterable[_Union[_data_pb2.WorkflowStepRun, _Mapping]]] = ...) -> None: ...

class ListWorkflowRunsResponse(_message.Message):
    __slots__ = ("runs", "has_more", "next_before_run_id")
    RUNS_FIELD_NUMBER: _ClassVar[int]
    HAS_MORE_FIELD_NUMBER: _ClassVar[int]
    NEXT_BEFORE_RUN_ID_FIELD_NUMBER: _ClassVar[int]
    runs: _containers.RepeatedCompositeFieldContainer[_data_pb2.WorkflowRun]
    has_more: bool
    next_before_run_id: str
    def __init__(self, runs: _Optional[_Iterable[_Union[_data_pb2.WorkflowRun, _Mapping]]] = ..., has_more: bool = ..., next_before_run_id: _Optional[str] = ...) -> None: ...
