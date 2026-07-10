from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional

DESCRIPTOR: _descriptor.FileDescriptor

class GetCasObjectRequest(_message.Message):
    __slots__ = ("key",)
    KEY_FIELD_NUMBER: _ClassVar[int]
    key: str
    def __init__(self, key: _Optional[str] = ...) -> None: ...

class GetCasObjectResponse(_message.Message):
    __slots__ = ("data", "signed_url", "signed_url_expires_at_unix_seconds", "metadata", "media_type", "size_bytes", "sha256", "filename", "content_encoding")
    class MetadataEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    DATA_FIELD_NUMBER: _ClassVar[int]
    SIGNED_URL_FIELD_NUMBER: _ClassVar[int]
    SIGNED_URL_EXPIRES_AT_UNIX_SECONDS_FIELD_NUMBER: _ClassVar[int]
    METADATA_FIELD_NUMBER: _ClassVar[int]
    MEDIA_TYPE_FIELD_NUMBER: _ClassVar[int]
    SIZE_BYTES_FIELD_NUMBER: _ClassVar[int]
    SHA256_FIELD_NUMBER: _ClassVar[int]
    FILENAME_FIELD_NUMBER: _ClassVar[int]
    CONTENT_ENCODING_FIELD_NUMBER: _ClassVar[int]
    data: bytes
    signed_url: str
    signed_url_expires_at_unix_seconds: int
    metadata: _containers.ScalarMap[str, str]
    media_type: str
    size_bytes: int
    sha256: str
    filename: str
    content_encoding: str
    def __init__(self, data: _Optional[bytes] = ..., signed_url: _Optional[str] = ..., signed_url_expires_at_unix_seconds: _Optional[int] = ..., metadata: _Optional[_Mapping[str, str]] = ..., media_type: _Optional[str] = ..., size_bytes: _Optional[int] = ..., sha256: _Optional[str] = ..., filename: _Optional[str] = ..., content_encoding: _Optional[str] = ...) -> None: ...
