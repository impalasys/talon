from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class ApiKeyGrant(_message.Message):
    __slots__ = ("kind", "namespace", "agent", "session", "channel")
    KIND_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    kind: str
    namespace: str
    agent: str
    session: str
    channel: str
    def __init__(self, kind: _Optional[str] = ..., namespace: _Optional[str] = ..., agent: _Optional[str] = ..., session: _Optional[str] = ..., channel: _Optional[str] = ...) -> None: ...

class ApiKeyRecord(_message.Message):
    __slots__ = ("id", "name", "prefix", "secret_hash", "grants", "created_at", "last_used_at", "expires_at", "revoked_at")
    ID_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    PREFIX_FIELD_NUMBER: _ClassVar[int]
    SECRET_HASH_FIELD_NUMBER: _ClassVar[int]
    GRANTS_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    LAST_USED_AT_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    REVOKED_AT_FIELD_NUMBER: _ClassVar[int]
    id: str
    name: str
    prefix: str
    secret_hash: str
    grants: _containers.RepeatedCompositeFieldContainer[ApiKeyGrant]
    created_at: int
    last_used_at: int
    expires_at: int
    revoked_at: int
    def __init__(self, id: _Optional[str] = ..., name: _Optional[str] = ..., prefix: _Optional[str] = ..., secret_hash: _Optional[str] = ..., grants: _Optional[_Iterable[_Union[ApiKeyGrant, _Mapping]]] = ..., created_at: _Optional[int] = ..., last_used_at: _Optional[int] = ..., expires_at: _Optional[int] = ..., revoked_at: _Optional[int] = ...) -> None: ...
