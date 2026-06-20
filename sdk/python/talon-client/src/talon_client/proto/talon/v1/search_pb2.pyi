from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class SearchMode(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    SEARCH_MODE_UNSPECIFIED: _ClassVar[SearchMode]
    SEARCH_MODE_KEYWORD: _ClassVar[SearchMode]
    SEARCH_MODE_SEMANTIC: _ClassVar[SearchMode]
    SEARCH_MODE_HYBRID: _ClassVar[SearchMode]

class SearchSort(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    SEARCH_SORT_UNSPECIFIED: _ClassVar[SearchSort]
    SEARCH_SORT_RELEVANCE: _ClassVar[SearchSort]
    SEARCH_SORT_RECENCY: _ClassVar[SearchSort]
SEARCH_MODE_UNSPECIFIED: SearchMode
SEARCH_MODE_KEYWORD: SearchMode
SEARCH_MODE_SEMANTIC: SearchMode
SEARCH_MODE_HYBRID: SearchMode
SEARCH_SORT_UNSPECIFIED: SearchSort
SEARCH_SORT_RELEVANCE: SearchSort
SEARCH_SORT_RECENCY: SearchSort

class SearchRequest(_message.Message):
    __slots__ = ("ns", "query", "resource_kinds", "agent", "session_id", "channel", "role", "part_type", "labels", "start_time", "end_time", "limit", "page_token", "mode", "sort")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    NS_FIELD_NUMBER: _ClassVar[int]
    QUERY_FIELD_NUMBER: _ClassVar[int]
    RESOURCE_KINDS_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    ROLE_FIELD_NUMBER: _ClassVar[int]
    PART_TYPE_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    START_TIME_FIELD_NUMBER: _ClassVar[int]
    END_TIME_FIELD_NUMBER: _ClassVar[int]
    LIMIT_FIELD_NUMBER: _ClassVar[int]
    PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    MODE_FIELD_NUMBER: _ClassVar[int]
    SORT_FIELD_NUMBER: _ClassVar[int]
    ns: str
    query: str
    resource_kinds: _containers.RepeatedScalarFieldContainer[str]
    agent: str
    session_id: str
    channel: str
    role: str
    part_type: str
    labels: _containers.ScalarMap[str, str]
    start_time: int
    end_time: int
    limit: int
    page_token: str
    mode: SearchMode
    sort: SearchSort
    def __init__(self, ns: _Optional[str] = ..., query: _Optional[str] = ..., resource_kinds: _Optional[_Iterable[str]] = ..., agent: _Optional[str] = ..., session_id: _Optional[str] = ..., channel: _Optional[str] = ..., role: _Optional[str] = ..., part_type: _Optional[str] = ..., labels: _Optional[_Mapping[str, str]] = ..., start_time: _Optional[int] = ..., end_time: _Optional[int] = ..., limit: _Optional[int] = ..., page_token: _Optional[str] = ..., mode: _Optional[_Union[SearchMode, str]] = ..., sort: _Optional[_Union[SearchSort, str]] = ...) -> None: ...

class Document(_message.Message):
    __slots__ = ("id", "namespace", "resource_kind", "resource_key", "parent_kind", "parent_key", "agent", "session_id", "channel", "message_id", "run_id", "part_id", "part_type", "role", "title", "snippet", "labels", "metadata_json", "acl_scope_json", "created_at", "updated_at", "indexed_at", "source_generation", "embedding_ref", "document_kind")
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    ID_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    RESOURCE_KIND_FIELD_NUMBER: _ClassVar[int]
    RESOURCE_KEY_FIELD_NUMBER: _ClassVar[int]
    PARENT_KIND_FIELD_NUMBER: _ClassVar[int]
    PARENT_KEY_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    PART_ID_FIELD_NUMBER: _ClassVar[int]
    PART_TYPE_FIELD_NUMBER: _ClassVar[int]
    ROLE_FIELD_NUMBER: _ClassVar[int]
    TITLE_FIELD_NUMBER: _ClassVar[int]
    SNIPPET_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    METADATA_JSON_FIELD_NUMBER: _ClassVar[int]
    ACL_SCOPE_JSON_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    UPDATED_AT_FIELD_NUMBER: _ClassVar[int]
    INDEXED_AT_FIELD_NUMBER: _ClassVar[int]
    SOURCE_GENERATION_FIELD_NUMBER: _ClassVar[int]
    EMBEDDING_REF_FIELD_NUMBER: _ClassVar[int]
    DOCUMENT_KIND_FIELD_NUMBER: _ClassVar[int]
    id: str
    namespace: str
    resource_kind: str
    resource_key: str
    parent_kind: str
    parent_key: str
    agent: str
    session_id: str
    channel: str
    message_id: str
    run_id: str
    part_id: str
    part_type: str
    role: str
    title: str
    snippet: str
    labels: _containers.ScalarMap[str, str]
    metadata_json: str
    acl_scope_json: str
    created_at: int
    updated_at: int
    indexed_at: int
    source_generation: int
    embedding_ref: str
    document_kind: str
    def __init__(self, id: _Optional[str] = ..., namespace: _Optional[str] = ..., resource_kind: _Optional[str] = ..., resource_key: _Optional[str] = ..., parent_kind: _Optional[str] = ..., parent_key: _Optional[str] = ..., agent: _Optional[str] = ..., session_id: _Optional[str] = ..., channel: _Optional[str] = ..., message_id: _Optional[str] = ..., run_id: _Optional[str] = ..., part_id: _Optional[str] = ..., part_type: _Optional[str] = ..., role: _Optional[str] = ..., title: _Optional[str] = ..., snippet: _Optional[str] = ..., labels: _Optional[_Mapping[str, str]] = ..., metadata_json: _Optional[str] = ..., acl_scope_json: _Optional[str] = ..., created_at: _Optional[int] = ..., updated_at: _Optional[int] = ..., indexed_at: _Optional[int] = ..., source_generation: _Optional[int] = ..., embedding_ref: _Optional[str] = ..., document_kind: _Optional[str] = ...) -> None: ...

class SearchResult(_message.Message):
    __slots__ = ("document", "score")
    DOCUMENT_FIELD_NUMBER: _ClassVar[int]
    SCORE_FIELD_NUMBER: _ClassVar[int]
    document: Document
    score: float
    def __init__(self, document: _Optional[_Union[Document, _Mapping]] = ..., score: _Optional[float] = ...) -> None: ...

class SearchResponse(_message.Message):
    __slots__ = ("results", "next_page_token")
    RESULTS_FIELD_NUMBER: _ClassVar[int]
    NEXT_PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    results: _containers.RepeatedCompositeFieldContainer[SearchResult]
    next_page_token: str
    def __init__(self, results: _Optional[_Iterable[_Union[SearchResult, _Mapping]]] = ..., next_page_token: _Optional[str] = ...) -> None: ...

class GetSearchResultRequest(_message.Message):
    __slots__ = ("ns", "document_id")
    NS_FIELD_NUMBER: _ClassVar[int]
    DOCUMENT_ID_FIELD_NUMBER: _ClassVar[int]
    ns: str
    document_id: str
    def __init__(self, ns: _Optional[str] = ..., document_id: _Optional[str] = ...) -> None: ...

class GetSearchResultResponse(_message.Message):
    __slots__ = ("document", "content")
    DOCUMENT_FIELD_NUMBER: _ClassVar[int]
    CONTENT_FIELD_NUMBER: _ClassVar[int]
    document: Document
    content: str
    def __init__(self, document: _Optional[_Union[Document, _Mapping]] = ..., content: _Optional[str] = ...) -> None: ...
