from talon_client.proto.resources import routing_pb2 as _routing_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class ConnectorActor(_message.Message):
    __slots__ = ("external_user_id", "external_address", "display_name", "kind")
    EXTERNAL_USER_ID_FIELD_NUMBER: _ClassVar[int]
    EXTERNAL_ADDRESS_FIELD_NUMBER: _ClassVar[int]
    DISPLAY_NAME_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    external_user_id: str
    external_address: str
    display_name: str
    kind: str
    def __init__(self, external_user_id: _Optional[str] = ..., external_address: _Optional[str] = ..., display_name: _Optional[str] = ..., kind: _Optional[str] = ...) -> None: ...

class ConnectorAttachment(_message.Message):
    __slots__ = ("id", "kind", "media_type", "filename", "size_bytes", "object_key", "external_url", "expires_at")
    ID_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    MEDIA_TYPE_FIELD_NUMBER: _ClassVar[int]
    FILENAME_FIELD_NUMBER: _ClassVar[int]
    SIZE_BYTES_FIELD_NUMBER: _ClassVar[int]
    OBJECT_KEY_FIELD_NUMBER: _ClassVar[int]
    EXTERNAL_URL_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    id: str
    kind: str
    media_type: str
    filename: str
    size_bytes: int
    object_key: str
    external_url: str
    expires_at: int
    def __init__(self, id: _Optional[str] = ..., kind: _Optional[str] = ..., media_type: _Optional[str] = ..., filename: _Optional[str] = ..., size_bytes: _Optional[int] = ..., object_key: _Optional[str] = ..., external_url: _Optional[str] = ..., expires_at: _Optional[int] = ...) -> None: ...

class ConnectorMessageEvent(_message.Message):
    __slots__ = ("event_id", "event_kind", "registration_id", "connector_class", "match_fields", "external_conversation_id", "external_thread_id", "external_message_id", "conversation_type", "sender", "text", "attachments", "event_time_ms", "labels")
    class MatchFieldsEntry(_message.Message):
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
    EVENT_ID_FIELD_NUMBER: _ClassVar[int]
    EVENT_KIND_FIELD_NUMBER: _ClassVar[int]
    REGISTRATION_ID_FIELD_NUMBER: _ClassVar[int]
    CONNECTOR_CLASS_FIELD_NUMBER: _ClassVar[int]
    MATCH_FIELDS_FIELD_NUMBER: _ClassVar[int]
    EXTERNAL_CONVERSATION_ID_FIELD_NUMBER: _ClassVar[int]
    EXTERNAL_THREAD_ID_FIELD_NUMBER: _ClassVar[int]
    EXTERNAL_MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    CONVERSATION_TYPE_FIELD_NUMBER: _ClassVar[int]
    SENDER_FIELD_NUMBER: _ClassVar[int]
    TEXT_FIELD_NUMBER: _ClassVar[int]
    ATTACHMENTS_FIELD_NUMBER: _ClassVar[int]
    EVENT_TIME_MS_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    event_id: str
    event_kind: str
    registration_id: str
    connector_class: str
    match_fields: _containers.ScalarMap[str, str]
    external_conversation_id: str
    external_thread_id: str
    external_message_id: str
    conversation_type: str
    sender: ConnectorActor
    text: str
    attachments: _containers.RepeatedCompositeFieldContainer[ConnectorAttachment]
    event_time_ms: int
    labels: _containers.ScalarMap[str, str]
    def __init__(self, event_id: _Optional[str] = ..., event_kind: _Optional[str] = ..., registration_id: _Optional[str] = ..., connector_class: _Optional[str] = ..., match_fields: _Optional[_Mapping[str, str]] = ..., external_conversation_id: _Optional[str] = ..., external_thread_id: _Optional[str] = ..., external_message_id: _Optional[str] = ..., conversation_type: _Optional[str] = ..., sender: _Optional[_Union[ConnectorActor, _Mapping]] = ..., text: _Optional[str] = ..., attachments: _Optional[_Iterable[_Union[ConnectorAttachment, _Mapping]]] = ..., event_time_ms: _Optional[int] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class ConnectorMessageEventResponse(_message.Message):
    __slots__ = ("accepted", "duplicate", "disposition", "namespace", "connector_name", "consumer")
    ACCEPTED_FIELD_NUMBER: _ClassVar[int]
    DUPLICATE_FIELD_NUMBER: _ClassVar[int]
    DISPOSITION_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    CONNECTOR_NAME_FIELD_NUMBER: _ClassVar[int]
    CONSUMER_FIELD_NUMBER: _ClassVar[int]
    accepted: bool
    duplicate: bool
    disposition: str
    namespace: str
    connector_name: str
    consumer: _routing_pb2.MessageConsumer
    def __init__(self, accepted: bool = ..., duplicate: bool = ..., disposition: _Optional[str] = ..., namespace: _Optional[str] = ..., connector_name: _Optional[str] = ..., consumer: _Optional[_Union[_routing_pb2.MessageConsumer, _Mapping]] = ...) -> None: ...

class ConnectorDeliveryRequest(_message.Message):
    __slots__ = ("delivery_id", "registration_id", "connector_class", "namespace", "connector_name", "match_fields", "external_conversation_id", "external_thread_id", "reply_to_external_message_id", "text", "attachments", "labels")
    class MatchFieldsEntry(_message.Message):
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
    DELIVERY_ID_FIELD_NUMBER: _ClassVar[int]
    REGISTRATION_ID_FIELD_NUMBER: _ClassVar[int]
    CONNECTOR_CLASS_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    CONNECTOR_NAME_FIELD_NUMBER: _ClassVar[int]
    MATCH_FIELDS_FIELD_NUMBER: _ClassVar[int]
    EXTERNAL_CONVERSATION_ID_FIELD_NUMBER: _ClassVar[int]
    EXTERNAL_THREAD_ID_FIELD_NUMBER: _ClassVar[int]
    REPLY_TO_EXTERNAL_MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    TEXT_FIELD_NUMBER: _ClassVar[int]
    ATTACHMENTS_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    delivery_id: str
    registration_id: str
    connector_class: str
    namespace: str
    connector_name: str
    match_fields: _containers.ScalarMap[str, str]
    external_conversation_id: str
    external_thread_id: str
    reply_to_external_message_id: str
    text: str
    attachments: _containers.RepeatedCompositeFieldContainer[ConnectorAttachment]
    labels: _containers.ScalarMap[str, str]
    def __init__(self, delivery_id: _Optional[str] = ..., registration_id: _Optional[str] = ..., connector_class: _Optional[str] = ..., namespace: _Optional[str] = ..., connector_name: _Optional[str] = ..., match_fields: _Optional[_Mapping[str, str]] = ..., external_conversation_id: _Optional[str] = ..., external_thread_id: _Optional[str] = ..., reply_to_external_message_id: _Optional[str] = ..., text: _Optional[str] = ..., attachments: _Optional[_Iterable[_Union[ConnectorAttachment, _Mapping]]] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class ConnectorDeliveryResponse(_message.Message):
    __slots__ = ("accepted", "disposition", "error")
    ACCEPTED_FIELD_NUMBER: _ClassVar[int]
    DISPOSITION_FIELD_NUMBER: _ClassVar[int]
    ERROR_FIELD_NUMBER: _ClassVar[int]
    accepted: bool
    disposition: str
    error: str
    def __init__(self, accepted: bool = ..., disposition: _Optional[str] = ..., error: _Optional[str] = ...) -> None: ...

class ConnectorStatusEvent(_message.Message):
    __slots__ = ("registration_id", "match_fields", "status", "reason")
    class MatchFieldsEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    REGISTRATION_ID_FIELD_NUMBER: _ClassVar[int]
    MATCH_FIELDS_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    REASON_FIELD_NUMBER: _ClassVar[int]
    registration_id: str
    match_fields: _containers.ScalarMap[str, str]
    status: str
    reason: str
    def __init__(self, registration_id: _Optional[str] = ..., match_fields: _Optional[_Mapping[str, str]] = ..., status: _Optional[str] = ..., reason: _Optional[str] = ...) -> None: ...

class ConnectorAckResponse(_message.Message):
    __slots__ = ("accepted", "disposition")
    ACCEPTED_FIELD_NUMBER: _ClassVar[int]
    DISPOSITION_FIELD_NUMBER: _ClassVar[int]
    accepted: bool
    disposition: str
    def __init__(self, accepted: bool = ..., disposition: _Optional[str] = ...) -> None: ...
