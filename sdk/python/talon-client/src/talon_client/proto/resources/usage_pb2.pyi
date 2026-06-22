from talon_client.proto.resources import common_pb2 as _common_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class UsageSelector(_message.Message):
    __slots__ = ("agent", "provider", "model")
    AGENT_FIELD_NUMBER: _ClassVar[int]
    PROVIDER_FIELD_NUMBER: _ClassVar[int]
    MODEL_FIELD_NUMBER: _ClassVar[int]
    agent: str
    provider: str
    model: str
    def __init__(self, agent: _Optional[str] = ..., provider: _Optional[str] = ..., model: _Optional[str] = ...) -> None: ...

class UsageLimit(_message.Message):
    __slots__ = ("selector", "metric", "max", "window")
    SELECTOR_FIELD_NUMBER: _ClassVar[int]
    METRIC_FIELD_NUMBER: _ClassVar[int]
    MAX_FIELD_NUMBER: _ClassVar[int]
    WINDOW_FIELD_NUMBER: _ClassVar[int]
    selector: UsageSelector
    metric: str
    max: int
    window: str
    def __init__(self, selector: _Optional[_Union[UsageSelector, _Mapping]] = ..., metric: _Optional[str] = ..., max: _Optional[int] = ..., window: _Optional[str] = ...) -> None: ...

class UsagePolicySpec(_message.Message):
    __slots__ = ("namespace_scope", "hard")
    NAMESPACE_SCOPE_FIELD_NUMBER: _ClassVar[int]
    HARD_FIELD_NUMBER: _ClassVar[int]
    namespace_scope: str
    hard: _containers.RepeatedCompositeFieldContainer[UsageLimit]
    def __init__(self, namespace_scope: _Optional[str] = ..., hard: _Optional[_Iterable[_Union[UsageLimit, _Mapping]]] = ...) -> None: ...

class UsageLimitStatus(_message.Message):
    __slots__ = ("selector", "metric", "max", "window", "window_start", "reset_at", "used", "remaining", "exceeded")
    SELECTOR_FIELD_NUMBER: _ClassVar[int]
    METRIC_FIELD_NUMBER: _ClassVar[int]
    MAX_FIELD_NUMBER: _ClassVar[int]
    WINDOW_FIELD_NUMBER: _ClassVar[int]
    WINDOW_START_FIELD_NUMBER: _ClassVar[int]
    RESET_AT_FIELD_NUMBER: _ClassVar[int]
    USED_FIELD_NUMBER: _ClassVar[int]
    REMAINING_FIELD_NUMBER: _ClassVar[int]
    EXCEEDED_FIELD_NUMBER: _ClassVar[int]
    selector: UsageSelector
    metric: str
    max: int
    window: str
    window_start: int
    reset_at: int
    used: int
    remaining: int
    exceeded: bool
    def __init__(self, selector: _Optional[_Union[UsageSelector, _Mapping]] = ..., metric: _Optional[str] = ..., max: _Optional[int] = ..., window: _Optional[str] = ..., window_start: _Optional[int] = ..., reset_at: _Optional[int] = ..., used: _Optional[int] = ..., remaining: _Optional[int] = ..., exceeded: bool = ...) -> None: ...

class UsagePolicyStatus(_message.Message):
    __slots__ = ("observed_generation", "phase", "conditions", "hard")
    OBSERVED_GENERATION_FIELD_NUMBER: _ClassVar[int]
    PHASE_FIELD_NUMBER: _ClassVar[int]
    CONDITIONS_FIELD_NUMBER: _ClassVar[int]
    HARD_FIELD_NUMBER: _ClassVar[int]
    observed_generation: int
    phase: str
    conditions: _containers.RepeatedCompositeFieldContainer[_common_pb2.ResourceCondition]
    hard: _containers.RepeatedCompositeFieldContainer[UsageLimitStatus]
    def __init__(self, observed_generation: _Optional[int] = ..., phase: _Optional[str] = ..., conditions: _Optional[_Iterable[_Union[_common_pb2.ResourceCondition, _Mapping]]] = ..., hard: _Optional[_Iterable[_Union[UsageLimitStatus, _Mapping]]] = ...) -> None: ...

class UsagePolicy(_message.Message):
    __slots__ = ("metadata", "spec", "status")
    METADATA_FIELD_NUMBER: _ClassVar[int]
    SPEC_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    metadata: _common_pb2.ResourceMeta
    spec: UsagePolicySpec
    status: UsagePolicyStatus
    def __init__(self, metadata: _Optional[_Union[_common_pb2.ResourceMeta, _Mapping]] = ..., spec: _Optional[_Union[UsagePolicySpec, _Mapping]] = ..., status: _Optional[_Union[UsagePolicyStatus, _Mapping]] = ...) -> None: ...
