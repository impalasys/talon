from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class TalonConfig(_message.Message):
    __slots__ = ("providers", "database", "server", "default_provider", "workspace_dir", "control_plane", "controllers", "trust")
    class ProvidersEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: LlmProviderConfig
        def __init__(self, key: _Optional[str] = ..., value: _Optional[_Union[LlmProviderConfig, _Mapping]] = ...) -> None: ...
    class ControllersEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: ControllerConfig
        def __init__(self, key: _Optional[str] = ..., value: _Optional[_Union[ControllerConfig, _Mapping]] = ...) -> None: ...
    PROVIDERS_FIELD_NUMBER: _ClassVar[int]
    DATABASE_FIELD_NUMBER: _ClassVar[int]
    SERVER_FIELD_NUMBER: _ClassVar[int]
    DEFAULT_PROVIDER_FIELD_NUMBER: _ClassVar[int]
    WORKSPACE_DIR_FIELD_NUMBER: _ClassVar[int]
    CONTROL_PLANE_FIELD_NUMBER: _ClassVar[int]
    CONTROLLERS_FIELD_NUMBER: _ClassVar[int]
    TRUST_FIELD_NUMBER: _ClassVar[int]
    providers: _containers.MessageMap[str, LlmProviderConfig]
    database: DatabaseConfig
    server: ServerConfig
    default_provider: str
    workspace_dir: str
    control_plane: ControlPlaneConfig
    controllers: _containers.MessageMap[str, ControllerConfig]
    trust: TrustConfig
    def __init__(self, providers: _Optional[_Mapping[str, LlmProviderConfig]] = ..., database: _Optional[_Union[DatabaseConfig, _Mapping]] = ..., server: _Optional[_Union[ServerConfig, _Mapping]] = ..., default_provider: _Optional[str] = ..., workspace_dir: _Optional[str] = ..., control_plane: _Optional[_Union[ControlPlaneConfig, _Mapping]] = ..., controllers: _Optional[_Mapping[str, ControllerConfig]] = ..., trust: _Optional[_Union[TrustConfig, _Mapping]] = ...) -> None: ...

class TrustConfig(_message.Message):
    __slots__ = ("oidc",)
    OIDC_FIELD_NUMBER: _ClassVar[int]
    oidc: _containers.RepeatedCompositeFieldContainer[OidcTrustEntry]
    def __init__(self, oidc: _Optional[_Iterable[_Union[OidcTrustEntry, _Mapping]]] = ...) -> None: ...

class OidcTrustEntry(_message.Message):
    __slots__ = ("name", "issuer", "audiences", "allowed_domains", "allowed_emails", "jwks_url", "clock_skew_seconds", "grants")
    NAME_FIELD_NUMBER: _ClassVar[int]
    ISSUER_FIELD_NUMBER: _ClassVar[int]
    AUDIENCES_FIELD_NUMBER: _ClassVar[int]
    ALLOWED_DOMAINS_FIELD_NUMBER: _ClassVar[int]
    ALLOWED_EMAILS_FIELD_NUMBER: _ClassVar[int]
    JWKS_URL_FIELD_NUMBER: _ClassVar[int]
    CLOCK_SKEW_SECONDS_FIELD_NUMBER: _ClassVar[int]
    GRANTS_FIELD_NUMBER: _ClassVar[int]
    name: str
    issuer: str
    audiences: _containers.RepeatedScalarFieldContainer[str]
    allowed_domains: _containers.RepeatedScalarFieldContainer[str]
    allowed_emails: _containers.RepeatedScalarFieldContainer[str]
    jwks_url: str
    clock_skew_seconds: int
    grants: _containers.RepeatedCompositeFieldContainer[OidcTrustGrant]
    def __init__(self, name: _Optional[str] = ..., issuer: _Optional[str] = ..., audiences: _Optional[_Iterable[str]] = ..., allowed_domains: _Optional[_Iterable[str]] = ..., allowed_emails: _Optional[_Iterable[str]] = ..., jwks_url: _Optional[str] = ..., clock_skew_seconds: _Optional[int] = ..., grants: _Optional[_Iterable[_Union[OidcTrustGrant, _Mapping]]] = ...) -> None: ...

class OidcTrustGrant(_message.Message):
    __slots__ = ("kind", "namespace", "agent", "session", "channel")
    class Kind(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
        __slots__ = ()
        KIND_UNSPECIFIED: _ClassVar[OidcTrustGrant.Kind]
        READ: _ClassVar[OidcTrustGrant.Kind]
        READWRITE: _ClassVar[OidcTrustGrant.Kind]
    KIND_UNSPECIFIED: OidcTrustGrant.Kind
    READ: OidcTrustGrant.Kind
    READWRITE: OidcTrustGrant.Kind
    KIND_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    AGENT_FIELD_NUMBER: _ClassVar[int]
    SESSION_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    kind: OidcTrustGrant.Kind
    namespace: str
    agent: str
    session: str
    channel: str
    def __init__(self, kind: _Optional[_Union[OidcTrustGrant.Kind, str]] = ..., namespace: _Optional[str] = ..., agent: _Optional[str] = ..., session: _Optional[str] = ..., channel: _Optional[str] = ...) -> None: ...

class ControllerConfig(_message.Message):
    __slots__ = ("enabled", "workers")
    ENABLED_FIELD_NUMBER: _ClassVar[int]
    WORKERS_FIELD_NUMBER: _ClassVar[int]
    enabled: bool
    workers: int
    def __init__(self, enabled: bool = ..., workers: _Optional[int] = ...) -> None: ...

class LlmProviderConfig(_message.Message):
    __slots__ = ("openai", "anthropic", "google", "openai_compatible")
    OPENAI_FIELD_NUMBER: _ClassVar[int]
    ANTHROPIC_FIELD_NUMBER: _ClassVar[int]
    GOOGLE_FIELD_NUMBER: _ClassVar[int]
    OPENAI_COMPATIBLE_FIELD_NUMBER: _ClassVar[int]
    openai: OpenAiConfig
    anthropic: AnthropicConfig
    google: GoogleConfig
    openai_compatible: GenericConfig
    def __init__(self, openai: _Optional[_Union[OpenAiConfig, _Mapping]] = ..., anthropic: _Optional[_Union[AnthropicConfig, _Mapping]] = ..., google: _Optional[_Union[GoogleConfig, _Mapping]] = ..., openai_compatible: _Optional[_Union[GenericConfig, _Mapping]] = ...) -> None: ...

class OpenAiConfig(_message.Message):
    __slots__ = ("model", "api_key", "org_id")
    MODEL_FIELD_NUMBER: _ClassVar[int]
    API_KEY_FIELD_NUMBER: _ClassVar[int]
    ORG_ID_FIELD_NUMBER: _ClassVar[int]
    model: str
    api_key: Secret
    org_id: str
    def __init__(self, model: _Optional[str] = ..., api_key: _Optional[_Union[Secret, _Mapping]] = ..., org_id: _Optional[str] = ...) -> None: ...

class AnthropicConfig(_message.Message):
    __slots__ = ("model", "api_key")
    MODEL_FIELD_NUMBER: _ClassVar[int]
    API_KEY_FIELD_NUMBER: _ClassVar[int]
    model: str
    api_key: Secret
    def __init__(self, model: _Optional[str] = ..., api_key: _Optional[_Union[Secret, _Mapping]] = ...) -> None: ...

class GoogleConfig(_message.Message):
    __slots__ = ("model", "api_key")
    MODEL_FIELD_NUMBER: _ClassVar[int]
    API_KEY_FIELD_NUMBER: _ClassVar[int]
    model: str
    api_key: Secret
    def __init__(self, model: _Optional[str] = ..., api_key: _Optional[_Union[Secret, _Mapping]] = ...) -> None: ...

class GenericConfig(_message.Message):
    __slots__ = ("name", "base_url", "model", "api_key")
    NAME_FIELD_NUMBER: _ClassVar[int]
    BASE_URL_FIELD_NUMBER: _ClassVar[int]
    MODEL_FIELD_NUMBER: _ClassVar[int]
    API_KEY_FIELD_NUMBER: _ClassVar[int]
    name: str
    base_url: str
    model: str
    api_key: Secret
    def __init__(self, name: _Optional[str] = ..., base_url: _Optional[str] = ..., model: _Optional[str] = ..., api_key: _Optional[_Union[Secret, _Mapping]] = ...) -> None: ...

class Secret(_message.Message):
    __slots__ = ("plain", "ref")
    PLAIN_FIELD_NUMBER: _ClassVar[int]
    REF_FIELD_NUMBER: _ClassVar[int]
    plain: str
    ref: SecretRef
    def __init__(self, plain: _Optional[str] = ..., ref: _Optional[_Union[SecretRef, _Mapping]] = ...) -> None: ...

class SecretRef(_message.Message):
    __slots__ = ("source", "key")
    class Source(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
        __slots__ = ()
        ENV: _ClassVar[SecretRef.Source]
        GCP: _ClassVar[SecretRef.Source]
        KEYCHAIN: _ClassVar[SecretRef.Source]
        AWS: _ClassVar[SecretRef.Source]
        AZURE: _ClassVar[SecretRef.Source]
    ENV: SecretRef.Source
    GCP: SecretRef.Source
    KEYCHAIN: SecretRef.Source
    AWS: SecretRef.Source
    AZURE: SecretRef.Source
    SOURCE_FIELD_NUMBER: _ClassVar[int]
    KEY_FIELD_NUMBER: _ClassVar[int]
    source: SecretRef.Source
    key: str
    def __init__(self, source: _Optional[_Union[SecretRef.Source, str]] = ..., key: _Optional[str] = ...) -> None: ...

class DatabaseConfig(_message.Message):
    __slots__ = ("data_dir", "driver", "url")
    DATA_DIR_FIELD_NUMBER: _ClassVar[int]
    DRIVER_FIELD_NUMBER: _ClassVar[int]
    URL_FIELD_NUMBER: _ClassVar[int]
    data_dir: str
    driver: str
    url: Secret
    def __init__(self, data_dir: _Optional[str] = ..., driver: _Optional[str] = ..., url: _Optional[_Union[Secret, _Mapping]] = ...) -> None: ...

class MessageBrokerConfig(_message.Message):
    __slots__ = ("driver",)
    DRIVER_FIELD_NUMBER: _ClassVar[int]
    driver: str
    def __init__(self, driver: _Optional[str] = ...) -> None: ...

class LocalObjectStoreConfig(_message.Message):
    __slots__ = ("path",)
    PATH_FIELD_NUMBER: _ClassVar[int]
    path: str
    def __init__(self, path: _Optional[str] = ...) -> None: ...

class GcsObjectStoreConfig(_message.Message):
    __slots__ = ("bucket", "prefix", "api_base_url")
    BUCKET_FIELD_NUMBER: _ClassVar[int]
    PREFIX_FIELD_NUMBER: _ClassVar[int]
    API_BASE_URL_FIELD_NUMBER: _ClassVar[int]
    bucket: str
    prefix: str
    api_base_url: str
    def __init__(self, bucket: _Optional[str] = ..., prefix: _Optional[str] = ..., api_base_url: _Optional[str] = ...) -> None: ...

class S3ObjectStoreConfig(_message.Message):
    __slots__ = ("bucket", "prefix", "region", "endpoint_url", "force_path_style")
    BUCKET_FIELD_NUMBER: _ClassVar[int]
    PREFIX_FIELD_NUMBER: _ClassVar[int]
    REGION_FIELD_NUMBER: _ClassVar[int]
    ENDPOINT_URL_FIELD_NUMBER: _ClassVar[int]
    FORCE_PATH_STYLE_FIELD_NUMBER: _ClassVar[int]
    bucket: str
    prefix: str
    region: str
    endpoint_url: str
    force_path_style: bool
    def __init__(self, bucket: _Optional[str] = ..., prefix: _Optional[str] = ..., region: _Optional[str] = ..., endpoint_url: _Optional[str] = ..., force_path_style: bool = ...) -> None: ...

class ObjectStoreConfig(_message.Message):
    __slots__ = ("local", "gcs", "s3")
    LOCAL_FIELD_NUMBER: _ClassVar[int]
    GCS_FIELD_NUMBER: _ClassVar[int]
    S3_FIELD_NUMBER: _ClassVar[int]
    local: LocalObjectStoreConfig
    gcs: GcsObjectStoreConfig
    s3: S3ObjectStoreConfig
    def __init__(self, local: _Optional[_Union[LocalObjectStoreConfig, _Mapping]] = ..., gcs: _Optional[_Union[GcsObjectStoreConfig, _Mapping]] = ..., s3: _Optional[_Union[S3ObjectStoreConfig, _Mapping]] = ...) -> None: ...

class SchedulerCallbackAuthConfig(_message.Message):
    __slots__ = ("shared_secret", "google_oidc")
    SHARED_SECRET_FIELD_NUMBER: _ClassVar[int]
    GOOGLE_OIDC_FIELD_NUMBER: _ClassVar[int]
    shared_secret: Secret
    google_oidc: GoogleOidcAuthConfig
    def __init__(self, shared_secret: _Optional[_Union[Secret, _Mapping]] = ..., google_oidc: _Optional[_Union[GoogleOidcAuthConfig, _Mapping]] = ...) -> None: ...

class GoogleOidcAuthConfig(_message.Message):
    __slots__ = ("audience", "service_account_email")
    AUDIENCE_FIELD_NUMBER: _ClassVar[int]
    SERVICE_ACCOUNT_EMAIL_FIELD_NUMBER: _ClassVar[int]
    audience: str
    service_account_email: str
    def __init__(self, audience: _Optional[str] = ..., service_account_email: _Optional[str] = ...) -> None: ...

class CloudTasksSchedulerConfig(_message.Message):
    __slots__ = ("project_id", "location", "queue", "target_url", "callback_auth")
    PROJECT_ID_FIELD_NUMBER: _ClassVar[int]
    LOCATION_FIELD_NUMBER: _ClassVar[int]
    QUEUE_FIELD_NUMBER: _ClassVar[int]
    TARGET_URL_FIELD_NUMBER: _ClassVar[int]
    CALLBACK_AUTH_FIELD_NUMBER: _ClassVar[int]
    project_id: str
    location: str
    queue: str
    target_url: str
    callback_auth: SchedulerCallbackAuthConfig
    def __init__(self, project_id: _Optional[str] = ..., location: _Optional[str] = ..., queue: _Optional[str] = ..., target_url: _Optional[str] = ..., callback_auth: _Optional[_Union[SchedulerCallbackAuthConfig, _Mapping]] = ...) -> None: ...

class SchedulerConfig(_message.Message):
    __slots__ = ("cloud_tasks",)
    CLOUD_TASKS_FIELD_NUMBER: _ClassVar[int]
    cloud_tasks: CloudTasksSchedulerConfig
    def __init__(self, cloud_tasks: _Optional[_Union[CloudTasksSchedulerConfig, _Mapping]] = ...) -> None: ...

class ControlPlaneConfig(_message.Message):
    __slots__ = ("database", "message_broker", "scheduler", "object_store", "documents")
    DATABASE_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_BROKER_FIELD_NUMBER: _ClassVar[int]
    SCHEDULER_FIELD_NUMBER: _ClassVar[int]
    OBJECT_STORE_FIELD_NUMBER: _ClassVar[int]
    DOCUMENTS_FIELD_NUMBER: _ClassVar[int]
    database: DatabaseConfig
    message_broker: MessageBrokerConfig
    scheduler: SchedulerConfig
    object_store: ObjectStoreConfig
    documents: DatabaseConfig
    def __init__(self, database: _Optional[_Union[DatabaseConfig, _Mapping]] = ..., message_broker: _Optional[_Union[MessageBrokerConfig, _Mapping]] = ..., scheduler: _Optional[_Union[SchedulerConfig, _Mapping]] = ..., object_store: _Optional[_Union[ObjectStoreConfig, _Mapping]] = ..., documents: _Optional[_Union[DatabaseConfig, _Mapping]] = ...) -> None: ...

class ServerConfig(_message.Message):
    __slots__ = ("host", "port")
    HOST_FIELD_NUMBER: _ClassVar[int]
    PORT_FIELD_NUMBER: _ClassVar[int]
    host: str
    port: int
    def __init__(self, host: _Optional[str] = ..., port: _Optional[int] = ...) -> None: ...
