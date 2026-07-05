from talon_client.proto.data import api_keys_pb2 as _api_keys_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class GetSsoConfigRequest(_message.Message):
    __slots__ = ()
    def __init__(self) -> None: ...

class GetSsoConfigResponse(_message.Message):
    __slots__ = ("google_sso_enabled", "google_web_client_id")
    GOOGLE_SSO_ENABLED_FIELD_NUMBER: _ClassVar[int]
    GOOGLE_WEB_CLIENT_ID_FIELD_NUMBER: _ClassVar[int]
    google_sso_enabled: bool
    google_web_client_id: str
    def __init__(self, google_sso_enabled: bool = ..., google_web_client_id: _Optional[str] = ...) -> None: ...

class ExchangeOidcTokenRequest(_message.Message):
    __slots__ = ("id_token", "trust", "client_type")
    ID_TOKEN_FIELD_NUMBER: _ClassVar[int]
    TRUST_FIELD_NUMBER: _ClassVar[int]
    CLIENT_TYPE_FIELD_NUMBER: _ClassVar[int]
    id_token: str
    trust: str
    client_type: str
    def __init__(self, id_token: _Optional[str] = ..., trust: _Optional[str] = ..., client_type: _Optional[str] = ...) -> None: ...

class ExchangeOidcTokenResponse(_message.Message):
    __slots__ = ("access_token", "token_type", "expires_in", "subject", "email", "trust", "client_type")
    ACCESS_TOKEN_FIELD_NUMBER: _ClassVar[int]
    TOKEN_TYPE_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_IN_FIELD_NUMBER: _ClassVar[int]
    SUBJECT_FIELD_NUMBER: _ClassVar[int]
    EMAIL_FIELD_NUMBER: _ClassVar[int]
    TRUST_FIELD_NUMBER: _ClassVar[int]
    CLIENT_TYPE_FIELD_NUMBER: _ClassVar[int]
    access_token: str
    token_type: str
    expires_in: int
    subject: str
    email: str
    trust: str
    client_type: str
    def __init__(self, access_token: _Optional[str] = ..., token_type: _Optional[str] = ..., expires_in: _Optional[int] = ..., subject: _Optional[str] = ..., email: _Optional[str] = ..., trust: _Optional[str] = ..., client_type: _Optional[str] = ...) -> None: ...

class MintAccessTokenRequest(_message.Message):
    __slots__ = ("namespace", "agent", "session", "channel", "expires_in", "origins", "sub")
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_IN_FIELD_NUMBER: _ClassVar[int]
    ORIGINS_FIELD_NUMBER: _ClassVar[int]
    SUB_FIELD_NUMBER: _ClassVar[int]
    namespace: str
    agent: str
    session: str
    channel: str
    expires_in: int
    origins: _containers.RepeatedScalarFieldContainer[str]
    sub: str
    def __init__(self, namespace: _Optional[str] = ..., agent: _Optional[str] = ..., session: _Optional[str] = ..., channel: _Optional[str] = ..., expires_in: _Optional[int] = ..., origins: _Optional[_Iterable[str]] = ..., sub: _Optional[str] = ...) -> None: ...

class MintAccessTokenResponse(_message.Message):
    __slots__ = ("access_token", "token_type", "expires_in", "expires_at")
    ACCESS_TOKEN_FIELD_NUMBER: _ClassVar[int]
    TOKEN_TYPE_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_IN_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    access_token: str
    token_type: str
    expires_in: int
    expires_at: int
    def __init__(self, access_token: _Optional[str] = ..., token_type: _Optional[str] = ..., expires_in: _Optional[int] = ..., expires_at: _Optional[int] = ...) -> None: ...

class ApiKeyInfo(_message.Message):
    __slots__ = ("id", "name", "prefix", "grants", "created_at", "last_used_at", "expires_at", "revoked_at")
    ID_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    PREFIX_FIELD_NUMBER: _ClassVar[int]
    GRANTS_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    LAST_USED_AT_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    REVOKED_AT_FIELD_NUMBER: _ClassVar[int]
    id: str
    name: str
    prefix: str
    grants: _containers.RepeatedCompositeFieldContainer[_api_keys_pb2.ApiKeyGrant]
    created_at: int
    last_used_at: int
    expires_at: int
    revoked_at: int
    def __init__(self, id: _Optional[str] = ..., name: _Optional[str] = ..., prefix: _Optional[str] = ..., grants: _Optional[_Iterable[_Union[_api_keys_pb2.ApiKeyGrant, _Mapping]]] = ..., created_at: _Optional[int] = ..., last_used_at: _Optional[int] = ..., expires_at: _Optional[int] = ..., revoked_at: _Optional[int] = ...) -> None: ...

class CreateApiKeyRequest(_message.Message):
    __slots__ = ("name", "grants", "expires_at")
    NAME_FIELD_NUMBER: _ClassVar[int]
    GRANTS_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    name: str
    grants: _containers.RepeatedCompositeFieldContainer[_api_keys_pb2.ApiKeyGrant]
    expires_at: int
    def __init__(self, name: _Optional[str] = ..., grants: _Optional[_Iterable[_Union[_api_keys_pb2.ApiKeyGrant, _Mapping]]] = ..., expires_at: _Optional[int] = ...) -> None: ...

class CreateApiKeyResponse(_message.Message):
    __slots__ = ("api_key", "secret")
    API_KEY_FIELD_NUMBER: _ClassVar[int]
    SECRET_FIELD_NUMBER: _ClassVar[int]
    api_key: ApiKeyInfo
    secret: str
    def __init__(self, api_key: _Optional[_Union[ApiKeyInfo, _Mapping]] = ..., secret: _Optional[str] = ...) -> None: ...

class ListApiKeysRequest(_message.Message):
    __slots__ = ()
    def __init__(self) -> None: ...

class ListApiKeysResponse(_message.Message):
    __slots__ = ("api_keys",)
    API_KEYS_FIELD_NUMBER: _ClassVar[int]
    api_keys: _containers.RepeatedCompositeFieldContainer[ApiKeyInfo]
    def __init__(self, api_keys: _Optional[_Iterable[_Union[ApiKeyInfo, _Mapping]]] = ...) -> None: ...

class RevokeApiKeyRequest(_message.Message):
    __slots__ = ("id",)
    ID_FIELD_NUMBER: _ClassVar[int]
    id: str
    def __init__(self, id: _Optional[str] = ...) -> None: ...

class RevokeApiKeyResponse(_message.Message):
    __slots__ = ("api_key",)
    API_KEY_FIELD_NUMBER: _ClassVar[int]
    api_key: ApiKeyInfo
    def __init__(self, api_key: _Optional[_Union[ApiKeyInfo, _Mapping]] = ...) -> None: ...

class ExchangeApiKeyRequest(_message.Message):
    __slots__ = ("api_key", "grant", "expires_in")
    API_KEY_FIELD_NUMBER: _ClassVar[int]
    GRANT_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_IN_FIELD_NUMBER: _ClassVar[int]
    api_key: str
    grant: _api_keys_pb2.ApiKeyGrant
    expires_in: int
    def __init__(self, api_key: _Optional[str] = ..., grant: _Optional[_Union[_api_keys_pb2.ApiKeyGrant, _Mapping]] = ..., expires_in: _Optional[int] = ...) -> None: ...

class ExchangeApiKeyResponse(_message.Message):
    __slots__ = ("access_token", "token_type", "expires_in", "expires_at")
    ACCESS_TOKEN_FIELD_NUMBER: _ClassVar[int]
    TOKEN_TYPE_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_IN_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    access_token: str
    token_type: str
    expires_in: int
    expires_at: int
    def __init__(self, access_token: _Optional[str] = ..., token_type: _Optional[str] = ..., expires_in: _Optional[int] = ..., expires_at: _Optional[int] = ...) -> None: ...
