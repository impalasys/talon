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
    __slots__ = ("query", "source", "attributes", "labels", "start_time", "end_time", "limit", "page_token", "mode", "sort")
    class AttributesEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    QUERY_FIELD_NUMBER: _ClassVar[int]
    SOURCE_FIELD_NUMBER: _ClassVar[int]
    ATTRIBUTES_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    START_TIME_FIELD_NUMBER: _ClassVar[int]
    END_TIME_FIELD_NUMBER: _ClassVar[int]
    LIMIT_FIELD_NUMBER: _ClassVar[int]
    PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    MODE_FIELD_NUMBER: _ClassVar[int]
    SORT_FIELD_NUMBER: _ClassVar[int]
    query: str
    source: SearchSourceFilter
    attributes: _containers.ScalarMap[str, str]
    labels: _containers.ScalarMap[str, str]
    start_time: int
    end_time: int
    limit: int
    page_token: str
    mode: SearchMode
    sort: SearchSort
    def __init__(self, query: _Optional[str] = ..., source: _Optional[_Union[SearchSourceFilter, _Mapping]] = ..., attributes: _Optional[_Mapping[str, str]] = ..., labels: _Optional[_Mapping[str, str]] = ..., start_time: _Optional[int] = ..., end_time: _Optional[int] = ..., limit: _Optional[int] = ..., page_token: _Optional[str] = ..., mode: _Optional[_Union[SearchMode, str]] = ..., sort: _Optional[_Union[SearchSort, str]] = ...) -> None: ...

class SearchSourceFilter(_message.Message):
    __slots__ = ("key", "key_prefix", "kinds", "parent_key", "namespaces")
    KEY_FIELD_NUMBER: _ClassVar[int]
    KEY_PREFIX_FIELD_NUMBER: _ClassVar[int]
    KINDS_FIELD_NUMBER: _ClassVar[int]
    PARENT_KEY_FIELD_NUMBER: _ClassVar[int]
    NAMESPACES_FIELD_NUMBER: _ClassVar[int]
    key: str
    key_prefix: str
    kinds: _containers.RepeatedScalarFieldContainer[str]
    parent_key: str
    namespaces: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, key: _Optional[str] = ..., key_prefix: _Optional[str] = ..., kinds: _Optional[_Iterable[str]] = ..., parent_key: _Optional[str] = ..., namespaces: _Optional[_Iterable[str]] = ...) -> None: ...

class Document(_message.Message):
    __slots__ = ("id", "source", "document_kind", "subdocument_id", "attributes", "title", "snippet", "labels", "metadata_json", "acl_scope_json", "created_at", "updated_at", "indexed_at", "generation", "embedding_ref")
    class AttributesEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    class LabelsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    ID_FIELD_NUMBER: _ClassVar[int]
    SOURCE_FIELD_NUMBER: _ClassVar[int]
    DOCUMENT_KIND_FIELD_NUMBER: _ClassVar[int]
    SUBDOCUMENT_ID_FIELD_NUMBER: _ClassVar[int]
    ATTRIBUTES_FIELD_NUMBER: _ClassVar[int]
    TITLE_FIELD_NUMBER: _ClassVar[int]
    SNIPPET_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    METADATA_JSON_FIELD_NUMBER: _ClassVar[int]
    ACL_SCOPE_JSON_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    UPDATED_AT_FIELD_NUMBER: _ClassVar[int]
    INDEXED_AT_FIELD_NUMBER: _ClassVar[int]
    GENERATION_FIELD_NUMBER: _ClassVar[int]
    EMBEDDING_REF_FIELD_NUMBER: _ClassVar[int]
    id: str
    source: DocumentSource
    document_kind: str
    subdocument_id: str
    attributes: _containers.ScalarMap[str, str]
    title: str
    snippet: str
    labels: _containers.ScalarMap[str, str]
    metadata_json: str
    acl_scope_json: str
    created_at: int
    updated_at: int
    indexed_at: int
    generation: int
    embedding_ref: str
    def __init__(self, id: _Optional[str] = ..., source: _Optional[_Union[DocumentSource, _Mapping]] = ..., document_kind: _Optional[str] = ..., subdocument_id: _Optional[str] = ..., attributes: _Optional[_Mapping[str, str]] = ..., title: _Optional[str] = ..., snippet: _Optional[str] = ..., labels: _Optional[_Mapping[str, str]] = ..., metadata_json: _Optional[str] = ..., acl_scope_json: _Optional[str] = ..., created_at: _Optional[int] = ..., updated_at: _Optional[int] = ..., indexed_at: _Optional[int] = ..., generation: _Optional[int] = ..., embedding_ref: _Optional[str] = ...) -> None: ...

class DocumentSource(_message.Message):
    __slots__ = ("key", "namespace", "kind", "name", "parent_kind", "parent_key")
    KEY_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    PARENT_KIND_FIELD_NUMBER: _ClassVar[int]
    PARENT_KEY_FIELD_NUMBER: _ClassVar[int]
    key: str
    namespace: str
    kind: str
    name: str
    parent_kind: str
    parent_key: str
    def __init__(self, key: _Optional[str] = ..., namespace: _Optional[str] = ..., kind: _Optional[str] = ..., name: _Optional[str] = ..., parent_kind: _Optional[str] = ..., parent_key: _Optional[str] = ...) -> None: ...

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
