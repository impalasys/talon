from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from typing import ClassVar as _ClassVar, Optional as _Optional

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
