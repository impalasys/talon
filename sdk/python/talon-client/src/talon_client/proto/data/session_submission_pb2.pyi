from talon_client.proto.data import session_journal_entry_pb2 as _session_journal_entry_pb2
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class SessionSubmissionStatus(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    SESSION_SUBMISSION_STATUS_UNSPECIFIED: _ClassVar[SessionSubmissionStatus]
    SESSION_SUBMISSION_STATUS_PENDING: _ClassVar[SessionSubmissionStatus]
    SESSION_SUBMISSION_STATUS_CLAIMED: _ClassVar[SessionSubmissionStatus]
    SESSION_SUBMISSION_STATUS_COMMITTED: _ClassVar[SessionSubmissionStatus]
    SESSION_SUBMISSION_STATUS_FAILED: _ClassVar[SessionSubmissionStatus]
    SESSION_SUBMISSION_STATUS_INTERRUPTED: _ClassVar[SessionSubmissionStatus]
SESSION_SUBMISSION_STATUS_UNSPECIFIED: SessionSubmissionStatus
SESSION_SUBMISSION_STATUS_PENDING: SessionSubmissionStatus
SESSION_SUBMISSION_STATUS_CLAIMED: SessionSubmissionStatus
SESSION_SUBMISSION_STATUS_COMMITTED: SessionSubmissionStatus
SESSION_SUBMISSION_STATUS_FAILED: SessionSubmissionStatus
SESSION_SUBMISSION_STATUS_INTERRUPTED: SessionSubmissionStatus

class SessionSubmission(_message.Message):
    __slots__ = ("submission_id", "session_id", "user_message_id", "status", "attempt_id", "attempt_count", "claim_expires_at", "created_at", "updated_at", "completed_at", "committed_message_id", "current_phase", "current_journal_entry_id")
    SUBMISSION_ID_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    USER_MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    ATTEMPT_ID_FIELD_NUMBER: _ClassVar[int]
    ATTEMPT_COUNT_FIELD_NUMBER: _ClassVar[int]
    CLAIM_EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    UPDATED_AT_FIELD_NUMBER: _ClassVar[int]
    COMPLETED_AT_FIELD_NUMBER: _ClassVar[int]
    COMMITTED_MESSAGE_ID_FIELD_NUMBER: _ClassVar[int]
    CURRENT_PHASE_FIELD_NUMBER: _ClassVar[int]
    CURRENT_JOURNAL_ENTRY_ID_FIELD_NUMBER: _ClassVar[int]
    submission_id: str
    session_id: str
    user_message_id: str
    status: SessionSubmissionStatus
    attempt_id: str
    attempt_count: int
    claim_expires_at: int
    created_at: int
    updated_at: int
    completed_at: int
    committed_message_id: str
    current_phase: _session_journal_entry_pb2.SessionExecutionPhase
    current_journal_entry_id: str
    def __init__(self, submission_id: _Optional[str] = ..., session_id: _Optional[str] = ..., user_message_id: _Optional[str] = ..., status: _Optional[_Union[SessionSubmissionStatus, str]] = ..., attempt_id: _Optional[str] = ..., attempt_count: _Optional[int] = ..., claim_expires_at: _Optional[int] = ..., created_at: _Optional[int] = ..., updated_at: _Optional[int] = ..., completed_at: _Optional[int] = ..., committed_message_id: _Optional[str] = ..., current_phase: _Optional[_Union[_session_journal_entry_pb2.SessionExecutionPhase, str]] = ..., current_journal_entry_id: _Optional[str] = ...) -> None: ...
