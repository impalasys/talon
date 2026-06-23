from talon_client.proto.resources import resource_pb2 as _resource_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class CreateResourceRequest(_message.Message):
    __slots__ = ("ns", "manifest")
    NS_FIELD_NUMBER: _ClassVar[int]
    MANIFEST_FIELD_NUMBER: _ClassVar[int]
    ns: str
    manifest: _resource_pb2.ResourceManifest
    def __init__(self, ns: _Optional[str] = ..., manifest: _Optional[_Union[_resource_pb2.ResourceManifest, _Mapping]] = ...) -> None: ...

class GetResourceRequest(_message.Message):
    __slots__ = ("ns", "kind", "name")
    NS_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    ns: str
    kind: str
    name: str
    def __init__(self, ns: _Optional[str] = ..., kind: _Optional[str] = ..., name: _Optional[str] = ...) -> None: ...

class ListResourcesRequest(_message.Message):
    __slots__ = ("ns", "kind")
    NS_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    ns: str
    kind: str
    def __init__(self, ns: _Optional[str] = ..., kind: _Optional[str] = ...) -> None: ...

class DeleteResourceRequest(_message.Message):
    __slots__ = ("ns", "kind", "name")
    NS_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    ns: str
    kind: str
    name: str
    def __init__(self, ns: _Optional[str] = ..., kind: _Optional[str] = ..., name: _Optional[str] = ...) -> None: ...

class ResourceResponse(_message.Message):
    __slots__ = ("resource",)
    RESOURCE_FIELD_NUMBER: _ClassVar[int]
    resource: _resource_pb2.Resource
    def __init__(self, resource: _Optional[_Union[_resource_pb2.Resource, _Mapping]] = ...) -> None: ...

class ListResourcesResponse(_message.Message):
    __slots__ = ("resources",)
    RESOURCES_FIELD_NUMBER: _ClassVar[int]
    resources: _containers.RepeatedCompositeFieldContainer[_resource_pb2.Resource]
    def __init__(self, resources: _Optional[_Iterable[_Union[_resource_pb2.Resource, _Mapping]]] = ...) -> None: ...

class DeleteResourceResponse(_message.Message):
    __slots__ = ("success",)
    SUCCESS_FIELD_NUMBER: _ClassVar[int]
    success: bool
    def __init__(self, success: bool = ...) -> None: ...
