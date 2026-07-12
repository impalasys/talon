from talon_client.proto.data import data_pb2 as _data_pb2
from talon_client.proto.resources import files_pb2 as _files_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class FileRef(_message.Message):
    __slots__ = ("namespace", "name", "path", "handle")
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    PATH_FIELD_NUMBER: _ClassVar[int]
    HANDLE_FIELD_NUMBER: _ClassVar[int]
    namespace: str
    name: str
    path: str
    handle: str
    def __init__(self, namespace: _Optional[str] = ..., name: _Optional[str] = ..., path: _Optional[str] = ..., handle: _Optional[str] = ...) -> None: ...

class CreateFileRequest(_message.Message):
    __slots__ = ("namespace", "path", "media_type", "purpose", "index_policy", "retention", "content")
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    PATH_FIELD_NUMBER: _ClassVar[int]
    MEDIA_TYPE_FIELD_NUMBER: _ClassVar[int]
    PURPOSE_FIELD_NUMBER: _ClassVar[int]
    INDEX_POLICY_FIELD_NUMBER: _ClassVar[int]
    RETENTION_FIELD_NUMBER: _ClassVar[int]
    CONTENT_FIELD_NUMBER: _ClassVar[int]
    namespace: str
    path: str
    media_type: str
    purpose: _files_pb2.FilePurpose
    index_policy: _files_pb2.FileIndexPolicy
    retention: _files_pb2.FileRetention
    content: bytes
    def __init__(self, namespace: _Optional[str] = ..., path: _Optional[str] = ..., media_type: _Optional[str] = ..., purpose: _Optional[_Union[_files_pb2.FilePurpose, str]] = ..., index_policy: _Optional[_Union[_files_pb2.FileIndexPolicy, str]] = ..., retention: _Optional[_Union[_files_pb2.FileRetention, str]] = ..., content: _Optional[bytes] = ...) -> None: ...

class ReadFileRequest(_message.Message):
    __slots__ = ("file",)
    FILE_FIELD_NUMBER: _ClassVar[int]
    file: FileRef
    def __init__(self, file: _Optional[_Union[FileRef, _Mapping]] = ...) -> None: ...

class ReadFileResponse(_message.Message):
    __slots__ = ("file", "content", "signed_url", "signed_url_expires_at_unix_seconds")
    FILE_FIELD_NUMBER: _ClassVar[int]
    CONTENT_FIELD_NUMBER: _ClassVar[int]
    SIGNED_URL_FIELD_NUMBER: _ClassVar[int]
    SIGNED_URL_EXPIRES_AT_UNIX_SECONDS_FIELD_NUMBER: _ClassVar[int]
    file: _files_pb2.File
    content: bytes
    signed_url: str
    signed_url_expires_at_unix_seconds: int
    def __init__(self, file: _Optional[_Union[_files_pb2.File, _Mapping]] = ..., content: _Optional[bytes] = ..., signed_url: _Optional[str] = ..., signed_url_expires_at_unix_seconds: _Optional[int] = ...) -> None: ...

class UpdateFileRequest(_message.Message):
    __slots__ = ("file", "media_type", "content")
    FILE_FIELD_NUMBER: _ClassVar[int]
    MEDIA_TYPE_FIELD_NUMBER: _ClassVar[int]
    CONTENT_FIELD_NUMBER: _ClassVar[int]
    file: FileRef
    media_type: str
    content: bytes
    def __init__(self, file: _Optional[_Union[FileRef, _Mapping]] = ..., media_type: _Optional[str] = ..., content: _Optional[bytes] = ...) -> None: ...

class GetFileMetadataRequest(_message.Message):
    __slots__ = ("file",)
    FILE_FIELD_NUMBER: _ClassVar[int]
    file: FileRef
    def __init__(self, file: _Optional[_Union[FileRef, _Mapping]] = ...) -> None: ...

class ListFilesRequest(_message.Message):
    __slots__ = ("namespace", "prefix", "purpose", "index_policy", "limit", "page_token")
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    PREFIX_FIELD_NUMBER: _ClassVar[int]
    PURPOSE_FIELD_NUMBER: _ClassVar[int]
    INDEX_POLICY_FIELD_NUMBER: _ClassVar[int]
    LIMIT_FIELD_NUMBER: _ClassVar[int]
    PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    namespace: str
    prefix: str
    purpose: _files_pb2.FilePurpose
    index_policy: _files_pb2.FileIndexPolicy
    limit: int
    page_token: str
    def __init__(self, namespace: _Optional[str] = ..., prefix: _Optional[str] = ..., purpose: _Optional[_Union[_files_pb2.FilePurpose, str]] = ..., index_policy: _Optional[_Union[_files_pb2.FileIndexPolicy, str]] = ..., limit: _Optional[int] = ..., page_token: _Optional[str] = ...) -> None: ...

class ListFilesResponse(_message.Message):
    __slots__ = ("files", "next_page_token")
    FILES_FIELD_NUMBER: _ClassVar[int]
    NEXT_PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    files: _containers.RepeatedCompositeFieldContainer[_files_pb2.File]
    next_page_token: str
    def __init__(self, files: _Optional[_Iterable[_Union[_files_pb2.File, _Mapping]]] = ..., next_page_token: _Optional[str] = ...) -> None: ...

class DeleteFileRequest(_message.Message):
    __slots__ = ("file",)
    FILE_FIELD_NUMBER: _ClassVar[int]
    file: FileRef
    def __init__(self, file: _Optional[_Union[FileRef, _Mapping]] = ...) -> None: ...

class DeleteFileResponse(_message.Message):
    __slots__ = ("success",)
    SUCCESS_FIELD_NUMBER: _ClassVar[int]
    success: bool
    def __init__(self, success: bool = ...) -> None: ...

class PromoteArtifactRequest(_message.Message):
    __slots__ = ("artifact_handle", "target_path", "media_type", "purpose", "index_policy", "retention")
    ARTIFACT_HANDLE_FIELD_NUMBER: _ClassVar[int]
    TARGET_PATH_FIELD_NUMBER: _ClassVar[int]
    MEDIA_TYPE_FIELD_NUMBER: _ClassVar[int]
    PURPOSE_FIELD_NUMBER: _ClassVar[int]
    INDEX_POLICY_FIELD_NUMBER: _ClassVar[int]
    RETENTION_FIELD_NUMBER: _ClassVar[int]
    artifact_handle: str
    target_path: str
    media_type: str
    purpose: _files_pb2.FilePurpose
    index_policy: _files_pb2.FileIndexPolicy
    retention: _files_pb2.FileRetention
    def __init__(self, artifact_handle: _Optional[str] = ..., target_path: _Optional[str] = ..., media_type: _Optional[str] = ..., purpose: _Optional[_Union[_files_pb2.FilePurpose, str]] = ..., index_policy: _Optional[_Union[_files_pb2.FileIndexPolicy, str]] = ..., retention: _Optional[_Union[_files_pb2.FileRetention, str]] = ...) -> None: ...

class FileResponse(_message.Message):
    __slots__ = ("file", "file_handle")
    FILE_FIELD_NUMBER: _ClassVar[int]
    FILE_HANDLE_FIELD_NUMBER: _ClassVar[int]
    file: _files_pb2.File
    file_handle: str
    def __init__(self, file: _Optional[_Union[_files_pb2.File, _Mapping]] = ..., file_handle: _Optional[str] = ...) -> None: ...

class ReadArtifactRequest(_message.Message):
    __slots__ = ("artifact_handle",)
    ARTIFACT_HANDLE_FIELD_NUMBER: _ClassVar[int]
    artifact_handle: str
    def __init__(self, artifact_handle: _Optional[str] = ...) -> None: ...

class ReadArtifactResponse(_message.Message):
    __slots__ = ("artifact", "content", "signed_url", "signed_url_expires_at_unix_seconds")
    ARTIFACT_FIELD_NUMBER: _ClassVar[int]
    CONTENT_FIELD_NUMBER: _ClassVar[int]
    SIGNED_URL_FIELD_NUMBER: _ClassVar[int]
    SIGNED_URL_EXPIRES_AT_UNIX_SECONDS_FIELD_NUMBER: _ClassVar[int]
    artifact: _data_pb2.Artifact
    content: bytes
    signed_url: str
    signed_url_expires_at_unix_seconds: int
    def __init__(self, artifact: _Optional[_Union[_data_pb2.Artifact, _Mapping]] = ..., content: _Optional[bytes] = ..., signed_url: _Optional[str] = ..., signed_url_expires_at_unix_seconds: _Optional[int] = ...) -> None: ...

class GetArtifactMetadataRequest(_message.Message):
    __slots__ = ("artifact_handle",)
    ARTIFACT_HANDLE_FIELD_NUMBER: _ClassVar[int]
    artifact_handle: str
    def __init__(self, artifact_handle: _Optional[str] = ...) -> None: ...

class ListArtifactsRequest(_message.Message):
    __slots__ = ("namespace", "agent", "session_id", "prefix", "limit", "page_token")
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    PREFIX_FIELD_NUMBER: _ClassVar[int]
    LIMIT_FIELD_NUMBER: _ClassVar[int]
    PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    namespace: str
    agent: str
    session_id: str
    prefix: str
    limit: int
    page_token: str
    def __init__(self, namespace: _Optional[str] = ..., agent: _Optional[str] = ..., session_id: _Optional[str] = ..., prefix: _Optional[str] = ..., limit: _Optional[int] = ..., page_token: _Optional[str] = ...) -> None: ...

class ListArtifactsResponse(_message.Message):
    __slots__ = ("artifacts", "next_page_token")
    ARTIFACTS_FIELD_NUMBER: _ClassVar[int]
    NEXT_PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    artifacts: _containers.RepeatedCompositeFieldContainer[_data_pb2.Artifact]
    next_page_token: str
    def __init__(self, artifacts: _Optional[_Iterable[_Union[_data_pb2.Artifact, _Mapping]]] = ..., next_page_token: _Optional[str] = ...) -> None: ...

class GrantArtifactRequest(_message.Message):
    __slots__ = ("artifact_handle", "target_agent", "target_session_id", "operations", "ttl_seconds")
    ARTIFACT_HANDLE_FIELD_NUMBER: _ClassVar[int]
    TARGET_AGENT_FIELD_NUMBER: _ClassVar[int]
    TARGET_SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    OPERATIONS_FIELD_NUMBER: _ClassVar[int]
    TTL_SECONDS_FIELD_NUMBER: _ClassVar[int]
    artifact_handle: str
    target_agent: str
    target_session_id: str
    operations: _containers.RepeatedScalarFieldContainer[str]
    ttl_seconds: int
    def __init__(self, artifact_handle: _Optional[str] = ..., target_agent: _Optional[str] = ..., target_session_id: _Optional[str] = ..., operations: _Optional[_Iterable[str]] = ..., ttl_seconds: _Optional[int] = ...) -> None: ...

class ArtifactResponse(_message.Message):
    __slots__ = ("artifact", "artifact_handle")
    ARTIFACT_FIELD_NUMBER: _ClassVar[int]
    ARTIFACT_HANDLE_FIELD_NUMBER: _ClassVar[int]
    artifact: _data_pb2.Artifact
    artifact_handle: str
    def __init__(self, artifact: _Optional[_Union[_data_pb2.Artifact, _Mapping]] = ..., artifact_handle: _Optional[str] = ...) -> None: ...

class ArtifactHandleResponse(_message.Message):
    __slots__ = ("artifact_handle",)
    ARTIFACT_HANDLE_FIELD_NUMBER: _ClassVar[int]
    artifact_handle: str
    def __init__(self, artifact_handle: _Optional[str] = ...) -> None: ...
