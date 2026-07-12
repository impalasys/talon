from talon_client.proto.resources import common_pb2 as _common_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class FilePurpose(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    FILE_PURPOSE_UNSPECIFIED: _ClassVar[FilePurpose]
    FILE_PURPOSE_MEMORY: _ClassVar[FilePurpose]
    FILE_PURPOSE_ARTIFACT: _ClassVar[FilePurpose]

class FileIndexPolicy(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    FILE_INDEX_POLICY_UNSPECIFIED: _ClassVar[FileIndexPolicy]
    FILE_INDEX_POLICY_NONE: _ClassVar[FileIndexPolicy]
    FILE_INDEX_POLICY_SEARCH: _ClassVar[FileIndexPolicy]
    FILE_INDEX_POLICY_RETRIEVAL: _ClassVar[FileIndexPolicy]

class FileRetention(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    FILE_RETENTION_UNSPECIFIED: _ClassVar[FileRetention]
    FILE_RETENTION_RETAINED: _ClassVar[FileRetention]
FILE_PURPOSE_UNSPECIFIED: FilePurpose
FILE_PURPOSE_MEMORY: FilePurpose
FILE_PURPOSE_ARTIFACT: FilePurpose
FILE_INDEX_POLICY_UNSPECIFIED: FileIndexPolicy
FILE_INDEX_POLICY_NONE: FileIndexPolicy
FILE_INDEX_POLICY_SEARCH: FileIndexPolicy
FILE_INDEX_POLICY_RETRIEVAL: FileIndexPolicy
FILE_RETENTION_UNSPECIFIED: FileRetention
FILE_RETENTION_RETAINED: FileRetention

class File(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: FileSpec
    status: FileStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[FileSpec, _Mapping]] = ..., status: _Optional[_Union[FileStatus, _Mapping]] = ...) -> None: ...

class FileSpec(_message.Message):
    __slots__ = ("path", "media_type", "purpose", "index_policy", "retention")
    PATH_FIELD_NUMBER: _ClassVar[int]
    MEDIA_TYPE_FIELD_NUMBER: _ClassVar[int]
    PURPOSE_FIELD_NUMBER: _ClassVar[int]
    INDEX_POLICY_FIELD_NUMBER: _ClassVar[int]
    RETENTION_FIELD_NUMBER: _ClassVar[int]
    path: str
    media_type: str
    purpose: FilePurpose
    index_policy: FileIndexPolicy
    retention: FileRetention
    def __init__(self, path: _Optional[str] = ..., media_type: _Optional[str] = ..., purpose: _Optional[_Union[FilePurpose, str]] = ..., index_policy: _Optional[_Union[FileIndexPolicy, str]] = ..., retention: _Optional[_Union[FileRetention, str]] = ...) -> None: ...

class FileStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions", "object_ref", "updated_at")
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    OBJECT_REF_FIELD_NUMBER: _ClassVar[int]
    UPDATED_AT_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: str
    conditions: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceCondition]
    object_ref: FileObjectRef
    updated_at: int
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[str] = ..., conditions: _Optional[_Iterable[_Union[_common_pb2.ResourceCondition, _Mapping]]] = ..., object_ref: _Optional[_Union[FileObjectRef, _Mapping]] = ..., updated_at: _Optional[int] = ...) -> None: ...

class FileObjectRef(_message.Message):
    __slots__ = ("key", "media_type", "size_bytes", "sha256", "filename", "metadata")
    class MetadataEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    KEY_FIELD_NUMBER: _ClassVar[int]
    MEDIA_TYPE_FIELD_NUMBER: _ClassVar[int]
    SIZE_BYTES_FIELD_NUMBER: _ClassVar[int]
    SHA256_FIELD_NUMBER: _ClassVar[int]
    FILENAME_FIELD_NUMBER: _ClassVar[int]
    METADATA_FIELD_NUMBER: _ClassVar[int]
    key: str
    media_type: str
    size_bytes: int
    sha256: str
    filename: str
    metadata: _containers.ScalarMap[str, str]
    def __init__(self, key: _Optional[str] = ..., media_type: _Optional[str] = ..., size_bytes: _Optional[int] = ..., sha256: _Optional[str] = ..., filename: _Optional[str] = ..., metadata: _Optional[_Mapping[str, str]] = ...) -> None: ...
