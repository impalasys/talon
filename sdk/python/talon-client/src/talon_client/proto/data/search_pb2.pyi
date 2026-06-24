from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class DocumentSource(_message.Message):
    __slots__ = ("namespace", "key", "kind", "name", "parent_kind", "parent_key", "uid", "generation", "resource_version")
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    KEY_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    PARENT_KIND_FIELD_NUMBER: _ClassVar[int]
    PARENT_KEY_FIELD_NUMBER: _ClassVar[int]
    UID_FIELD_NUMBER: _ClassVar[int]
    GENERATION_FIELD_NUMBER: _ClassVar[int]
    RESOURCE_VERSION_FIELD_NUMBER: _ClassVar[int]
    namespace: str
    key: str
    kind: str
    name: str
    parent_kind: str
    parent_key: str
    uid: str
    generation: int
    resource_version: str
    def __init__(self, namespace: _Optional[str] = ..., key: _Optional[str] = ..., kind: _Optional[str] = ..., name: _Optional[str] = ..., parent_kind: _Optional[str] = ..., parent_key: _Optional[str] = ..., uid: _Optional[str] = ..., generation: _Optional[int] = ..., resource_version: _Optional[str] = ...) -> None: ...

class DocumentRef(_message.Message):
    __slots__ = ("id", "source", "document_kind", "subdocument_id", "attributes", "title", "labels", "metadata_json", "acl_scope_json", "created_at", "updated_at", "indexed_at", "generation", "embedding_ref")
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
    labels: _containers.ScalarMap[str, str]
    metadata_json: str
    acl_scope_json: str
    created_at: int
    updated_at: int
    indexed_at: int
    generation: int
    embedding_ref: str
    def __init__(self, id: _Optional[str] = ..., source: _Optional[_Union[DocumentSource, _Mapping]] = ..., document_kind: _Optional[str] = ..., subdocument_id: _Optional[str] = ..., attributes: _Optional[_Mapping[str, str]] = ..., title: _Optional[str] = ..., labels: _Optional[_Mapping[str, str]] = ..., metadata_json: _Optional[str] = ..., acl_scope_json: _Optional[str] = ..., created_at: _Optional[int] = ..., updated_at: _Optional[int] = ..., indexed_at: _Optional[int] = ..., generation: _Optional[int] = ..., embedding_ref: _Optional[str] = ...) -> None: ...

class Document(_message.Message):
    __slots__ = ("ref", "text")
    REF_FIELD_NUMBER: _ClassVar[int]
    TEXT_FIELD_NUMBER: _ClassVar[int]
    ref: DocumentRef
    text: str
    def __init__(self, ref: _Optional[_Union[DocumentRef, _Mapping]] = ..., text: _Optional[str] = ...) -> None: ...
