from talon_client.proto.data import data_pb2 as _data_pb2
from talon_client.proto.data import routing_pb2 as _routing_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class ConnectorMessageEventKind(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    CONNECTOR_MESSAGE_EVENT_KIND_UNSPECIFIED: _ClassVar[ConnectorMessageEventKind]
    CONNECTOR_MESSAGE_EVENT_KIND_CREATED: _ClassVar[ConnectorMessageEventKind]
    CONNECTOR_MESSAGE_EVENT_KIND_UPDATED: _ClassVar[ConnectorMessageEventKind]
    CONNECTOR_MESSAGE_EVENT_KIND_DELETED: _ClassVar[ConnectorMessageEventKind]

class ConnectorMessageEventStatus(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    CONNECTOR_MESSAGE_EVENT_STATUS_UNSPECIFIED: _ClassVar[ConnectorMessageEventStatus]
    CONNECTOR_MESSAGE_EVENT_STATUS_ACCEPTED: _ClassVar[ConnectorMessageEventStatus]
    CONNECTOR_MESSAGE_EVENT_STATUS_DUPLICATE: _ClassVar[ConnectorMessageEventStatus]
    CONNECTOR_MESSAGE_EVENT_STATUS_UNMATCHED: _ClassVar[ConnectorMessageEventStatus]
    CONNECTOR_MESSAGE_EVENT_STATUS_IGNORED: _ClassVar[ConnectorMessageEventStatus]
    CONNECTOR_MESSAGE_EVENT_STATUS_REJECTED: _ClassVar[ConnectorMessageEventStatus]
CONNECTOR_MESSAGE_EVENT_KIND_UNSPECIFIED: ConnectorMessageEventKind
CONNECTOR_MESSAGE_EVENT_KIND_CREATED: ConnectorMessageEventKind
CONNECTOR_MESSAGE_EVENT_KIND_UPDATED: ConnectorMessageEventKind
CONNECTOR_MESSAGE_EVENT_KIND_DELETED: ConnectorMessageEventKind
CONNECTOR_MESSAGE_EVENT_STATUS_UNSPECIFIED: ConnectorMessageEventStatus
CONNECTOR_MESSAGE_EVENT_STATUS_ACCEPTED: ConnectorMessageEventStatus
CONNECTOR_MESSAGE_EVENT_STATUS_DUPLICATE: ConnectorMessageEventStatus
CONNECTOR_MESSAGE_EVENT_STATUS_UNMATCHED: ConnectorMessageEventStatus
CONNECTOR_MESSAGE_EVENT_STATUS_IGNORED: ConnectorMessageEventStatus
CONNECTOR_MESSAGE_EVENT_STATUS_REJECTED: ConnectorMessageEventStatus

class RegisterClusterRequest(_message.Message):
    __slots__ = ("cluster_id", "registration_id", "namespace", "connector_class", "callback_base_url", "callback_auth_kind", "callback_auth_key", "protocol_version")
    CLUSTER_ID_FIELD_NUMBER: _ClassVar[int]
    REGISTRATION_ID_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    CONNECTOR_CLASS_FIELD_NUMBER: _ClassVar[int]
    CALLBACK_BASE_URL_FIELD_NUMBER: _ClassVar[int]
    CALLBACK_AUTH_KIND_FIELD_NUMBER: _ClassVar[int]
    CALLBACK_AUTH_KEY_FIELD_NUMBER: _ClassVar[int]
    PROTOCOL_VERSION_FIELD_NUMBER: _ClassVar[int]
    cluster_id: str
    registration_id: str
    namespace: str
    connector_class: str
    callback_base_url: str
    callback_auth_kind: str
    callback_auth_key: str
    protocol_version: str
    def __init__(self, cluster_id: _Optional[str] = ..., registration_id: _Optional[str] = ..., namespace: _Optional[str] = ..., connector_class: _Optional[str] = ..., callback_base_url: _Optional[str] = ..., callback_auth_kind: _Optional[str] = ..., callback_auth_key: _Optional[str] = ..., protocol_version: _Optional[str] = ...) -> None: ...

class RegisterClusterResponse(_message.Message):
    __slots__ = ("registration_id",)
    REGISTRATION_ID_FIELD_NUMBER: _ClassVar[int]
    registration_id: str
    def __init__(self, registration_id: _Optional[str] = ...) -> None: ...

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
    event_kind: ConnectorMessageEventKind
    registration_id: str
    connector_class: str
    match_fields: _containers.ScalarMap[str, str]
    external_conversation_id: str
    external_thread_id: str
    external_message_id: str
    conversation_type: str
    sender: _data_pb2.Principal
    text: str
    attachments: _containers.RepeatedCompositeFieldContainer[_data_pb2.ObjectRef]
    event_time_ms: int
    labels: _containers.ScalarMap[str, str]
    def __init__(self, event_id: _Optional[str] = ..., event_kind: _Optional[_Union[ConnectorMessageEventKind, str]] = ..., registration_id: _Optional[str] = ..., connector_class: _Optional[str] = ..., match_fields: _Optional[_Mapping[str, str]] = ..., external_conversation_id: _Optional[str] = ..., external_thread_id: _Optional[str] = ..., external_message_id: _Optional[str] = ..., conversation_type: _Optional[str] = ..., sender: _Optional[_Union[_data_pb2.Principal, _Mapping]] = ..., text: _Optional[str] = ..., attachments: _Optional[_Iterable[_Union[_data_pb2.ObjectRef, _Mapping]]] = ..., event_time_ms: _Optional[int] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class ConnectorMessageEventResponse(_message.Message):
    __slots__ = ("status", "reason", "namespace", "connector_name", "consumer")
    STATUS_FIELD_NUMBER: _ClassVar[int]
    REASON_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    CONNECTOR_NAME_FIELD_NUMBER: _ClassVar[int]
    CONSUMER_FIELD_NUMBER: _ClassVar[int]
    status: ConnectorMessageEventStatus
    reason: str
    namespace: str
    connector_name: str
    consumer: _routing_pb2.MessageConsumer
    def __init__(self, status: _Optional[_Union[ConnectorMessageEventStatus, str]] = ..., reason: _Optional[str] = ..., namespace: _Optional[str] = ..., connector_name: _Optional[str] = ..., consumer: _Optional[_Union[_routing_pb2.MessageConsumer, _Mapping]] = ...) -> None: ...

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
    attachments: _containers.RepeatedCompositeFieldContainer[_data_pb2.ObjectRef]
    labels: _containers.ScalarMap[str, str]
    def __init__(self, delivery_id: _Optional[str] = ..., registration_id: _Optional[str] = ..., connector_class: _Optional[str] = ..., namespace: _Optional[str] = ..., connector_name: _Optional[str] = ..., match_fields: _Optional[_Mapping[str, str]] = ..., external_conversation_id: _Optional[str] = ..., external_thread_id: _Optional[str] = ..., reply_to_external_message_id: _Optional[str] = ..., text: _Optional[str] = ..., attachments: _Optional[_Iterable[_Union[_data_pb2.ObjectRef, _Mapping]]] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

class ConnectorDeliveryResponse(_message.Message):
    __slots__ = ("accepted", "disposition", "error")
    ACCEPTED_FIELD_NUMBER: _ClassVar[int]
    DISPOSITION_FIELD_NUMBER: _ClassVar[int]
    ERROR_FIELD_NUMBER: _ClassVar[int]
    accepted: bool
    disposition: str
    error: str
    def __init__(self, accepted: bool = ..., disposition: _Optional[str] = ..., error: _Optional[str] = ...) -> None: ...

class ConnectorActivityRequest(_message.Message):
    __slots__ = ("activity_id", "registration_id", "connector_class", "namespace", "connector_name", "match_fields", "external_conversation_id", "external_thread_id", "kind", "phase", "status_text", "labels")
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
    ACTIVITY_ID_FIELD_NUMBER: _ClassVar[int]
    REGISTRATION_ID_FIELD_NUMBER: _ClassVar[int]
    CONNECTOR_CLASS_FIELD_NUMBER: _ClassVar[int]
    NAMESPACE_FIELD_NUMBER: _ClassVar[int]
    CONNECTOR_NAME_FIELD_NUMBER: _ClassVar[int]
    MATCH_FIELDS_FIELD_NUMBER: _ClassVar[int]
    EXTERNAL_CONVERSATION_ID_FIELD_NUMBER: _ClassVar[int]
    EXTERNAL_THREAD_ID_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    STATUS_TEXT_FIELD_NUMBER: _ClassVar[int]
    LABELS_FIELD_NUMBER: _ClassVar[int]
    activity_id: str
    registration_id: str
    connector_class: str
    namespace: str
    connector_name: str
    match_fields: _containers.ScalarMap[str, str]
    external_conversation_id: str
    external_thread_id: str
    kind: str
    phase: str
    status_text: str
    labels: _containers.ScalarMap[str, str]
    def __init__(self, activity_id: _Optional[str] = ..., registration_id: _Optional[str] = ..., connector_class: _Optional[str] = ..., namespace: _Optional[str] = ..., connector_name: _Optional[str] = ..., match_fields: _Optional[_Mapping[str, str]] = ..., external_conversation_id: _Optional[str] = ..., external_thread_id: _Optional[str] = ..., kind: _Optional[str] = ..., phase: _Optional[str] = ..., status_text: _Optional[str] = ..., labels: _Optional[_Mapping[str, str]] = ...) -> None: ...

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
