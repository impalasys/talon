from talon_client.proto.data import data_pb2 as _data_pb2
from talon_client.proto.talon.v1 import search_pb2 as _search_pb2
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
    __slots__ = ("agent", "ns", "query", "limit", "mode", "sort")
    AGENT_FIELD_NUMBER: _ClassVar[int]
    NS_FIELD_NUMBER: _ClassVar[int]
    QUERY_FIELD_NUMBER: _ClassVar[int]
    LIMIT_FIELD_NUMBER: _ClassVar[int]
    MODE_FIELD_NUMBER: _ClassVar[int]
    SORT_FIELD_NUMBER: _ClassVar[int]
    agent: str
    ns: str
    query: str
    limit: int
    mode: _search_pb2.SearchMode
    sort: _search_pb2.SearchSort
    def __init__(self, agent: _Optional[str] = ..., ns: _Optional[str] = ..., query: _Optional[str] = ..., limit: _Optional[int] = ..., mode: _Optional[_Union[_search_pb2.SearchMode, str]] = ..., sort: _Optional[_Union[_search_pb2.SearchSort, str]] = ...) -> None: ...

class SearchKnowledgeResponse(_message.Message):
    __slots__ = ("results", "search_results", "next_page_token")
    RESULTS_FIELD_NUMBER: _ClassVar[int]
    SEARCH_RESULTS_FIELD_NUMBER: _ClassVar[int]
    NEXT_PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    results: _containers.RepeatedCompositeFieldContainer[_data_pb2.KnowledgeSearchResult]
    search_results: _containers.RepeatedCompositeFieldContainer[_search_pb2.SearchResult]
    next_page_token: str
    def __init__(self, results: _Optional[_Iterable[_Union[_data_pb2.KnowledgeSearchResult, _Mapping]]] = ..., search_results: _Optional[_Iterable[_Union[_search_pb2.SearchResult, _Mapping]]] = ..., next_page_token: _Optional[str] = ...) -> None: ...
