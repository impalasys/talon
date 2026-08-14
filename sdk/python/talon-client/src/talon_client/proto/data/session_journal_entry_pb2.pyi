from talon_client.proto.data import data_pb2 as _data_pb2
from talon_client.proto.harness import llm_pb2 as _llm_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class SessionExecutionPhase(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    SESSION_EXECUTION_PHASE_UNSPECIFIED: _ClassVar[SessionExecutionPhase]
    SESSION_EXECUTION_PHASE_LLM_RESPONSE: _ClassVar[SessionExecutionPhase]
    SESSION_EXECUTION_PHASE_TOOL_RESULT: _ClassVar[SessionExecutionPhase]
    SESSION_EXECUTION_PHASE_COMMITTED: _ClassVar[SessionExecutionPhase]
    SESSION_EXECUTION_PHASE_COMPACTION: _ClassVar[SessionExecutionPhase]
    SESSION_EXECUTION_PHASE_STEER_INPUT: _ClassVar[SessionExecutionPhase]
SESSION_EXECUTION_PHASE_UNSPECIFIED: SessionExecutionPhase
SESSION_EXECUTION_PHASE_LLM_RESPONSE: SessionExecutionPhase
SESSION_EXECUTION_PHASE_TOOL_RESULT: SessionExecutionPhase
SESSION_EXECUTION_PHASE_COMMITTED: SessionExecutionPhase
SESSION_EXECUTION_PHASE_COMPACTION: SessionExecutionPhase
SESSION_EXECUTION_PHASE_STEER_INPUT: SessionExecutionPhase

class SessionJournalEntryPayloadLlmResponse(_message.Message):
    __slots__ = ("response",)
    RESPONSE_FIELD_NUMBER: _ClassVar[int]
    response: _llm_pb2.ChatResponse
    def __init__(self, response: _Optional[_Union[_llm_pb2.ChatResponse, _Mapping]] = ...) -> None: ...

class SessionJournalEntryPayloadToolResult(_message.Message):
    __slots__ = ("tool_call_id", "name", "output", "object", "tool_output")
    TOOL_CALL_ID_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    OUTPUT_FIELD_NUMBER: _ClassVar[int]
    OBJECT_FIELD_NUMBER: _ClassVar[int]
    TOOL_OUTPUT_FIELD_NUMBER: _ClassVar[int]
    tool_call_id: str
    name: str
    output: str
    object: _data_pb2.ObjectRef
    tool_output: _llm_pb2.ToolOutput
    def __init__(self, tool_call_id: _Optional[str] = ..., name: _Optional[str] = ..., output: _Optional[str] = ..., object: _Optional[_Union[_data_pb2.ObjectRef, _Mapping]] = ..., tool_output: _Optional[_Union[_llm_pb2.ToolOutput, _Mapping]] = ...) -> None: ...

class SessionJournalEntryPayloadCommit(_message.Message):
    __slots__ = ("committed_message_id",)
    COMMITTED_MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    committed_message_id: str
    def __init__(self, committed_message_id: _Optional[str] = ...) -> None: ...

class SessionJournalEntryPayloadCompaction(_message.Message):
    __slots__ = ("summary",)
    SUMMARY_FIELD_NUMBER: _ClassVar[int]
    summary: _data_pb2.ObjectRef
    def __init__(self, summary: _Optional[_Union[_data_pb2.ObjectRef, _Mapping]] = ...) -> None: ...

class SessionJournalEntryPayloadSteerInput(_message.Message):
    __slots__ = ("message_ids",)
    MESSAGE_IDS_FIELD_NUMBER: _ClassVar[int]
    message_ids: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, message_ids: _Optional[_Iterable[str]] = ...) -> None: ...

class SessionJournalEntryPayload(_message.Message):
    __slots__ = ("llm_response", "tool_result", "commit", "compaction", "steer_input")
    LLM_RESPONSE_FIELD_NUMBER: _ClassVar[int]
    TOOL_RESULT_FIELD_NUMBER: _ClassVar[int]
    COMMIT_FIELD_NUMBER: _ClassVar[int]
    COMPACTION_FIELD_NUMBER: _ClassVar[int]
    STEER_INPUT_FIELD_NUMBER: _ClassVar[int]
    llm_response: SessionJournalEntryPayloadLlmResponse
    tool_result: SessionJournalEntryPayloadToolResult
    commit: SessionJournalEntryPayloadCommit
    compaction: SessionJournalEntryPayloadCompaction
    steer_input: SessionJournalEntryPayloadSteerInput
    def __init__(self, llm_response: _Optional[_Union[SessionJournalEntryPayloadLlmResponse, _Mapping]] = ..., tool_result: _Optional[_Union[SessionJournalEntryPayloadToolResult, _Mapping]] = ..., commit: _Optional[_Union[SessionJournalEntryPayloadCommit, _Mapping]] = ..., compaction: _Optional[_Union[SessionJournalEntryPayloadCompaction, _Mapping]] = ..., steer_input: _Optional[_Union[SessionJournalEntryPayloadSteerInput, _Mapping]] = ...) -> None: ...

class SessionJournalEntry(_message.Message):
    __slots__ = ("submission_id", "journal_entry_id", "attempt_id", "phase", "payload", "created_at", "updated_at", "committed_at", "committed_message_id")
    SUBMISSION_ID_FIELD_NUMBER: _ClassVar[int]
    JOURNAL_ENTRY_ID_FIELD_NUMBER: _ClassVar[int]
    ATTEMPT_ID_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    PAYLOAD_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    UPDATED_AT_FIELD_NUMBER: _ClassVar[int]
    COMMITTED_AT_FIELD_NUMBER: _ClassVar[int]
    COMMITTED_MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    submission_id: str
    journal_entry_id: str
    attempt_id: str
    phase: SessionExecutionPhase
    payload: SessionJournalEntryPayload
    created_at: int
    updated_at: int
    committed_at: int
    committed_message_id: str
    def __init__(self, submission_id: _Optional[str] = ..., journal_entry_id: _Optional[str] = ..., attempt_id: _Optional[str] = ..., phase: _Optional[_Union[SessionExecutionPhase, str]] = ..., payload: _Optional[_Union[SessionJournalEntryPayload, _Mapping]] = ..., created_at: _Optional[int] = ..., updated_at: _Optional[int] = ..., committed_at: _Optional[int] = ..., committed_message_id: _Optional[str] = ...) -> None: ...
