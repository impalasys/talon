from talon_client.proto.data import data_pb2 as _data_pb2
from talon_client.proto.resources import agents_pb2 as _agents_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class ChatContentPart(_message.Message):
    __slots__ = ("text", "object_ref")
    TEXT_FIELD_NUMBER: _ClassVar[int]
    OBJECT_REF_FIELD_NUMBER: _ClassVar[int]
    text: str
    object_ref: _data_pb2.ObjectRef
    def __init__(self, text: _Optional[str] = ..., object_ref: _Optional[_Union[_data_pb2.ObjectRef, _Mapping]] = ...) -> None: ...

class ToolOutput(_message.Message):
    __slots__ = ("content_parts", "summary", "content_descriptor")
    CONTENT_PARTS_FIELD_NUMBER: _ClassVar[int]
    SUMMARY_FIELD_NUMBER: _ClassVar[int]
    CONTENT_DESCRIPTOR_FIELD_NUMBER: _ClassVar[int]
    content_parts: _containers.RepeatedCompositeFieldContainer[ChatContentPart]
    summary: str
    content_descriptor: ToolOutputContentDescriptor
    def __init__(self, content_parts: _Optional[_Iterable[_Union[ChatContentPart, _Mapping]]] = ..., summary: _Optional[str] = ..., content_descriptor: _Optional[_Union[ToolOutputContentDescriptor, _Mapping]] = ...) -> None: ...

class ToolOutputLineSelection(_message.Message):
    __slots__ = ("start_line", "end_line")
    START_LINE_FIELD_NUMBER: _ClassVar[int]
    END_LINE_FIELD_NUMBER: _ClassVar[int]
    start_line: int
    end_line: int
    def __init__(self, start_line: _Optional[int] = ..., end_line: _Optional[int] = ...) -> None: ...

class ToolOutputByteRange(_message.Message):
    __slots__ = ("start", "end", "next_byte")
    START_FIELD_NUMBER: _ClassVar[int]
    END_FIELD_NUMBER: _ClassVar[int]
    NEXT_BYTE_FIELD_NUMBER: _ClassVar[int]
    start: int
    end: int
    next_byte: int
    def __init__(self, start: _Optional[int] = ..., end: _Optional[int] = ..., next_byte: _Optional[int] = ...) -> None: ...

class ToolOutputContentDescriptor(_message.Message):
    __slots__ = ("section_readable", "captured_size_bytes", "line_count", "capture_truncated", "selection", "byte_range")
    SECTION_READABLE_FIELD_NUMBER: _ClassVar[int]
    CAPTURED_SIZE_BYTES_FIELD_NUMBER: _ClassVar[int]
    LINE_COUNT_FIELD_NUMBER: _ClassVar[int]
    CAPTURE_TRUNCATED_FIELD_NUMBER: _ClassVar[int]
    SELECTION_FIELD_NUMBER: _ClassVar[int]
    BYTE_RANGE_FIELD_NUMBER: _ClassVar[int]
    section_readable: bool
    captured_size_bytes: int
    line_count: int
    capture_truncated: bool
    selection: ToolOutputLineSelection
    byte_range: ToolOutputByteRange
    def __init__(self, section_readable: bool = ..., captured_size_bytes: _Optional[int] = ..., line_count: _Optional[int] = ..., capture_truncated: bool = ..., selection: _Optional[_Union[ToolOutputLineSelection, _Mapping]] = ..., byte_range: _Optional[_Union[ToolOutputByteRange, _Mapping]] = ...) -> None: ...

class ToolCall(_message.Message):
    __slots__ = ("id", "name", "arguments")
    ID_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    ARGUMENTS_FIELD_NUMBER: _ClassVar[int]
    id: str
    name: str
    arguments: str
    def __init__(self, id: _Optional[str] = ..., name: _Optional[str] = ..., arguments: _Optional[str] = ...) -> None: ...

class ToolCallDelta(_message.Message):
    __slots__ = ("index", "id", "name", "arguments")
    INDEX_FIELD_NUMBER: _ClassVar[int]
    ID_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    ARGUMENTS_FIELD_NUMBER: _ClassVar[int]
    index: int
    id: str
    name: str
    arguments: str
    def __init__(self, index: _Optional[int] = ..., id: _Optional[str] = ..., name: _Optional[str] = ..., arguments: _Optional[str] = ...) -> None: ...

class ChatMessage(_message.Message):
    __slots__ = ("role", "content_parts", "tool_calls", "tool_call_id", "encrypted_reasoning")
    ROLE_FIELD_NUMBER: _ClassVar[int]
    CONTENT_PARTS_FIELD_NUMBER: _ClassVar[int]
    TOOL_CALLS_FIELD_NUMBER: _ClassVar[int]
    TOOL_CALL_ID_FIELD_NUMBER: _ClassVar[int]
    ENCRYPTED_REASONING_FIELD_NUMBER: _ClassVar[int]
    role: str
    content_parts: _containers.RepeatedCompositeFieldContainer[ChatContentPart]
    tool_calls: _containers.RepeatedCompositeFieldContainer[ToolCall]
    tool_call_id: str
    encrypted_reasoning: _data_pb2.ObjectRef
    def __init__(self, role: _Optional[str] = ..., content_parts: _Optional[_Iterable[_Union[ChatContentPart, _Mapping]]] = ..., tool_calls: _Optional[_Iterable[_Union[ToolCall, _Mapping]]] = ..., tool_call_id: _Optional[str] = ..., encrypted_reasoning: _Optional[_Union[_data_pb2.ObjectRef, _Mapping]] = ...) -> None: ...

class ChatResponse(_message.Message):
    __slots__ = ("content", "tool_calls", "usage", "encrypted_reasoning")
    CONTENT_FIELD_NUMBER: _ClassVar[int]
    TOOL_CALLS_FIELD_NUMBER: _ClassVar[int]
    USAGE_FIELD_NUMBER: _ClassVar[int]
    ENCRYPTED_REASONING_FIELD_NUMBER: _ClassVar[int]
    content: str
    tool_calls: _containers.RepeatedCompositeFieldContainer[ToolCall]
    usage: _data_pb2.TokenCounter
    encrypted_reasoning: _data_pb2.ObjectRef
    def __init__(self, content: _Optional[str] = ..., tool_calls: _Optional[_Iterable[_Union[ToolCall, _Mapping]]] = ..., usage: _Optional[_Union[_data_pb2.TokenCounter, _Mapping]] = ..., encrypted_reasoning: _Optional[_Union[_data_pb2.ObjectRef, _Mapping]] = ...) -> None: ...

class Tool(_message.Message):
    __slots__ = ("name", "description", "input_schema_json")
    NAME_FIELD_NUMBER: _ClassVar[int]
    DESCRIPTION_FIELD_NUMBER: _ClassVar[int]
    INPUT_SCHEMA_JSON_FIELD_NUMBER: _ClassVar[int]
    name: str
    description: str
    input_schema_json: str
    def __init__(self, name: _Optional[str] = ..., description: _Optional[str] = ..., input_schema_json: _Optional[str] = ...) -> None: ...

class ChatRequest(_message.Message):
    __slots__ = ("messages", "tools", "thinking", "previous_response_id", "zero_data_retention")
    MESSAGES_FIELD_NUMBER: _ClassVar[int]
    TOOLS_FIELD_NUMBER: _ClassVar[int]
    THINKING_FIELD_NUMBER: _ClassVar[int]
    PREVIOUS_RESPONSE_ID_FIELD_NUMBER: _ClassVar[int]
    ZERO_DATA_RETENTION_FIELD_NUMBER: _ClassVar[int]
    messages: _containers.RepeatedCompositeFieldContainer[ChatMessage]
    tools: _containers.RepeatedCompositeFieldContainer[Tool]
    thinking: _agents_pb2.ThinkingConfig
    previous_response_id: str
    zero_data_retention: bool
    def __init__(self, messages: _Optional[_Iterable[_Union[ChatMessage, _Mapping]]] = ..., tools: _Optional[_Iterable[_Union[Tool, _Mapping]]] = ..., thinking: _Optional[_Union[_agents_pb2.ThinkingConfig, _Mapping]] = ..., previous_response_id: _Optional[str] = ..., zero_data_retention: bool = ...) -> None: ...

class ChatStreamEvent(_message.Message):
    __slots__ = ("text_delta", "reasoning_delta", "tool_call_delta", "usage", "encrypted_reasoning")
    TEXT_DELTA_FIELD_NUMBER: _ClassVar[int]
    REASONING_DELTA_FIELD_NUMBER: _ClassVar[int]
    TOOL_CALL_DELTA_FIELD_NUMBER: _ClassVar[int]
    USAGE_FIELD_NUMBER: _ClassVar[int]
    ENCRYPTED_REASONING_FIELD_NUMBER: _ClassVar[int]
    text_delta: str
    reasoning_delta: str
    tool_call_delta: ToolCallDelta
    usage: _data_pb2.TokenCounter
    encrypted_reasoning: str
    def __init__(self, text_delta: _Optional[str] = ..., reasoning_delta: _Optional[str] = ..., tool_call_delta: _Optional[_Union[ToolCallDelta, _Mapping]] = ..., usage: _Optional[_Union[_data_pb2.TokenCounter, _Mapping]] = ..., encrypted_reasoning: _Optional[str] = ...) -> None: ...
