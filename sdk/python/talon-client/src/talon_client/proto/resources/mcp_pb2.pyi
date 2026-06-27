from talon_client.proto.resources import common_pb2 as _common_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class McpServer(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: McpServerSpec
    status: _common_pb2.CommonResourceStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[McpServerSpec, _Mapping]] = ..., status: _Optional[_Union[_common_pb2.CommonResourceStatus, _Mapping]] = ...) -> None: ...

class McpServerSpec(_message.Message):
    __slots__ = ("transport", "target", "args", "headers", "disabled", "auth_broker", "policy")
    class HeadersEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    TRANSPORT_FIELD_NUMBER: _ClassVar[int]
    TARGET_FIELD_NUMBER: _ClassVar[int]
    ARGS_FIELD_NUMBER: _ClassVar[int]
    HEADERS_FIELD_NUMBER: _ClassVar[int]
    DISABLED_FIELD_NUMBER: _ClassVar[int]
    AUTH_BROKER_FIELD_NUMBER: _ClassVar[int]
    POLICY_FIELD_NUMBER: _ClassVar[int]
    transport: str
    target: str
    args: _containers.RepeatedScalarFieldContainer[str]
    headers: _containers.ScalarMap[str, str]
    disabled: bool
    auth_broker: McpAuthBrokerSpec
    policy: McpServerPolicy
    def __init__(self, transport: _Optional[str] = ..., target: _Optional[str] = ..., args: _Optional[_Iterable[str]] = ..., headers: _Optional[_Mapping[str, str]] = ..., disabled: bool = ..., auth_broker: _Optional[_Union[McpAuthBrokerSpec, _Mapping]] = ..., policy: _Optional[_Union[McpServerPolicy, _Mapping]] = ...) -> None: ...

class McpAuthBrokerSpec(_message.Message):
    __slots__ = ("kind", "url", "cache_ttl_seconds", "audience")
    KIND_FIELD_NUMBER: _ClassVar[int]
    URL_FIELD_NUMBER: _ClassVar[int]
    CACHE_TTL_SECONDS_FIELD_NUMBER: _ClassVar[int]
    AUDIENCE_FIELD_NUMBER: _ClassVar[int]
    kind: str
    url: str
    cache_ttl_seconds: int
    audience: str
    def __init__(self, kind: _Optional[str] = ..., url: _Optional[str] = ..., cache_ttl_seconds: _Optional[int] = ..., audience: _Optional[str] = ...) -> None: ...

class McpServerPolicy(_message.Message):
    __slots__ = ("tools",)
    TOOLS_FIELD_NUMBER: _ClassVar[int]
    tools: McpToolPolicy
    def __init__(self, tools: _Optional[_Union[McpToolPolicy, _Mapping]] = ...) -> None: ...

class McpToolPolicy(_message.Message):
    __slots__ = ("allowlist",)
    ALLOWLIST_FIELD_NUMBER: _ClassVar[int]
    allowlist: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, allowlist: _Optional[_Iterable[str]] = ...) -> None: ...
