from talon_client.proto.data import data_pb2 as _data_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class GetKnowledgeRequest(_message.Message):
    __slots__ = ("agent", "ns", "path")
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    PATH_FIELD_NUMBER: _ClassVar[int]
    agent: str
    ns: str
    path: str
    def __init__(self, agent: _Optional[str] = ..., ns: _Optional[str] = ..., path: _Optional[str] = ...) -> None: ...

class KnowledgeResponse(_message.Message):
    __slots__ = ("modules",)
    MODULES_FIELD_NUMBER: _ClassVar[int]
    modules: _containers.RepeatedCompositeFieldContainer[_data_pb2.Knowledge]
    def __init__(self, modules: _Optional[_Iterable[_Union[_data_pb2.Knowledge, _Mapping]]] = ...) -> None: ...

class SearchKnowledgeRequest(_message.Message):
    __slots__ = ("agent", "ns", "query")
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    QUERY_FIELD_NUMBER: _ClassVar[int]
    agent: str
    ns: str
    query: str
    def __init__(self, agent: _Optional[str] = ..., ns: _Optional[str] = ..., query: _Optional[str] = ...) -> None: ...

class SearchKnowledgeResponse(_message.Message):
    __slots__ = ("results",)
    RESULTS_FIELD_NUMBER: _ClassVar[int]
    results: _containers.RepeatedCompositeFieldContainer[_data_pb2.KnowledgeSearchResult]
    def __init__(self, results: _Optional[_Iterable[_Union[_data_pb2.KnowledgeSearchResult, _Mapping]]] = ...) -> None: ...
