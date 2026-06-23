from talon_client.proto.resources import agents_pb2 as _agents_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class ChatImageUrl(_message.Message):
    __slots__ = ("url", "detail")
    URL_FIELD_NUMBER: _ClassVar[int]
    DETAIL_FIELD_NUMBER: _ClassVar[int]
    url: str
    detail: str
    def __init__(self, url: _Optional[str] = ..., detail: _Optional[str] = ...) -> None: ...

class ChatImageData(_message.Message):
    __slots__ = ("media_type", "data_base64", "detail")
    MEDIA_TYPE_FIELD_NUMBER: _ClassVar[int]
    DATA_BASE64_FIELD_NUMBER: _ClassVar[int]
    DETAIL_FIELD_NUMBER: _ClassVar[int]
    media_type: str
    data_base64: str
    detail: str
    def __init__(self, media_type: _Optional[str] = ..., data_base64: _Optional[str] = ..., detail: _Optional[str] = ...) -> None: ...

class ChatContentPart(_message.Message):
    __slots__ = ("text", "image_url", "image_data")
    TEXT_FIELD_NUMBER: _ClassVar[int]
    IMAGE_URL_FIELD_NUMBER: _ClassVar[int]
    IMAGE_DATA_FIELD_NUMBER: _ClassVar[int]
    text: str
    image_url: ChatImageUrl
    image_data: ChatImageData
    def __init__(self, text: _Optional[str] = ..., image_url: _Optional[_Union[ChatImageUrl, _Mapping]] = ..., image_data: _Optional[_Union[ChatImageData, _Mapping]] = ...) -> None: ...

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
    __slots__ = ("role", "content_parts", "tool_calls", "tool_call_id")
    ROLE_FIELD_NUMBER: _ClassVar[int]
    CONTENT_PARTS_FIELD_NUMBER: _ClassVar[int]
    TOOL_CALLS_FIELD_NUMBER: _ClassVar[int]
    TOOL_CALL_ID_FIELD_NUMBER: _ClassVar[int]
    role: str
    content_parts: _containers.RepeatedCompositeFieldContainer[ChatContentPart]
    tool_calls: _containers.RepeatedCompositeFieldContainer[ToolCall]
    tool_call_id: str
    def __init__(self, role: _Optional[str] = ..., content_parts: _Optional[_Iterable[_Union[ChatContentPart, _Mapping]]] = ..., tool_calls: _Optional[_Iterable[_Union[ToolCall, _Mapping]]] = ..., tool_call_id: _Optional[str] = ...) -> None: ...

class ChatUsage(_message.Message):
    __slots__ = ("input_tokens", "output_tokens", "reasoning_tokens", "total_tokens")
    INPUT_TOKENS_FIELD_NUMBER: _ClassVar[int]
    OUTPUT_TOKENS_FIELD_NUMBER: _ClassVar[int]
    REASONING_TOKENS_FIELD_NUMBER: _ClassVar[int]
    TOTAL_TOKENS_FIELD_NUMBER: _ClassVar[int]
    input_tokens: int
    output_tokens: int
    reasoning_tokens: int
    total_tokens: int
    def __init__(self, input_tokens: _Optional[int] = ..., output_tokens: _Optional[int] = ..., reasoning_tokens: _Optional[int] = ..., total_tokens: _Optional[int] = ...) -> None: ...

class ChatResponse(_message.Message):
    __slots__ = ("content", "tool_calls", "usage")
    CONTENT_FIELD_NUMBER: _ClassVar[int]
    TOOL_CALLS_FIELD_NUMBER: _ClassVar[int]
    USAGE_FIELD_NUMBER: _ClassVar[int]
    content: str
    tool_calls: _containers.RepeatedCompositeFieldContainer[ToolCall]
    usage: ChatUsage
    def __init__(self, content: _Optional[str] = ..., tool_calls: _Optional[_Iterable[_Union[ToolCall, _Mapping]]] = ..., usage: _Optional[_Union[ChatUsage, _Mapping]] = ...) -> None: ...

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
    __slots__ = ("messages", "tools", "thinking")
    MESSAGES_FIELD_NUMBER: _ClassVar[int]
    TOOLS_FIELD_NUMBER: _ClassVar[int]
    THINKING_FIELD_NUMBER: _ClassVar[int]
    messages: _containers.RepeatedCompositeFieldContainer[ChatMessage]
    tools: _containers.RepeatedCompositeFieldContainer[Tool]
    thinking: _agents_pb2.ThinkingConfig
    def __init__(self, messages: _Optional[_Iterable[_Union[ChatMessage, _Mapping]]] = ..., tools: _Optional[_Iterable[_Union[Tool, _Mapping]]] = ..., thinking: _Optional[_Union[_agents_pb2.ThinkingConfig, _Mapping]] = ...) -> None: ...

class ChatStreamEvent(_message.Message):
    __slots__ = ("text_delta", "reasoning_delta", "tool_call_delta", "usage")
    TEXT_DELTA_FIELD_NUMBER: _ClassVar[int]
    REASONING_DELTA_FIELD_NUMBER: _ClassVar[int]
    TOOL_CALL_DELTA_FIELD_NUMBER: _ClassVar[int]
    USAGE_FIELD_NUMBER: _ClassVar[int]
    text_delta: str
    reasoning_delta: str
    tool_call_delta: ToolCallDelta
    usage: ChatUsage
    def __init__(self, text_delta: _Optional[str] = ..., reasoning_delta: _Optional[str] = ..., tool_call_delta: _Optional[_Union[ToolCallDelta, _Mapping]] = ..., usage: _Optional[_Union[ChatUsage, _Mapping]] = ...) -> None: ...
