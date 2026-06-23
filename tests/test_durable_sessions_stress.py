from __future__ import annotations

import json
import os
import shutil
import socket
import sqlite3
import subprocess
import sys
import threading
import time
import tempfile
import uuid
from pathlib import Path
from typing import Any, Callable, Iterator, TypeVar

import grpc
import pytest
import requests

from talon_client import (
    TalonClient,
    CreateNamespaceRequest,
    CreateResourceRequest,
    CreateSessionRequest,
    GetSessionRequest,
    SendMessageRequest,
    StreamSessionPartsRequest,
)
from talon_client.data import (
    SESSION_SUBMISSION_STATUS_CLAIMED,
    SESSION_SUBMISSION_STATUS_COMMITTED,
    SessionMessage,
    SessionSubmission,
    SESSION_EXECUTION_PHASE_COMMITTED,
    SESSION_EXECUTION_PHASE_LLM_RESPONSE,
    SESSION_EXECUTION_PHASE_TOOL_RESULT,
    SessionJournalEntry,
)
from talon_client.events import SessionMessagePartEvent
from talon_client.resources import (
    AgentSpec,
    CommonResourceStatus,
    McpServer,
    McpServerSpec,
    Model,
    ResourceManifest,
    ResourceMeta,
    ResourceSpec,
)

import conftest


BLOCKING_MCP_SERVER = "durable-slow"
BLOCKING_MCP_TOOL = "mcp_durable_slow_blocking_lookup"
BLOCKING_TOOL_CALL_ID = "call_blocking_lookup_1"
PART_TYPE_TEXT = 1
PART_TYPE_TOOL_CALL = 3
PART_TYPE_TOOL_RESULT = 4
ROLE_USER = 1
ROLE_ASSISTANT = 2
KvRow = dict[str, Any]
SubmissionRecord = dict[str, Any]
JournalEntryRecord = dict[str, Any]
T = TypeVar("T")


def unused_tcp_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


class DurableSessionKvProbe:
    """Direct SQLite reader for durable session internals."""

    def __init__(self, data_dir: Path) -> None:
        self.data_dir = data_dir

    @property
    def sqlite_db_path(self) -> Path:
        return self.data_dir / "talon-control-plane.db"

    def read_kv_rows(self, kind: str) -> list[KvRow]:
        with sqlite3.connect(self.sqlite_db_path) as conn:
            conn.row_factory = sqlite3.Row
            return [
                dict(row)
                for row in conn.execute(
                    """
                    SELECT namespace, parent_path, kind, name, value
                    FROM talon_kv_store
                    WHERE kind = ?
                    ORDER BY parent_path, name
                    """,
                    (kind,),
                )
            ]

    def read_submission(
        self, namespace: str, agent: str, session_id: str, submission_id: str | None = None
    ) -> SubmissionRecord | None:
        rows = [
            row
            for row in self.read_kv_rows("SessionSubmission")
            if row["namespace"] == namespace
            and row["parent_path"] == session_parent(agent, session_id)
            and (submission_id is None or row["name"] == submission_id)
        ]
        if not rows:
            return None
        return submission_dict(rows[0])

    def read_journal_entries(
        self, namespace: str, agent: str, session_id: str, submission_id: str
    ) -> list[JournalEntryRecord]:
        rows = [
            row
            for row in self.read_kv_rows("SessionJournalEntry")
            if row["namespace"] == namespace
            and row["parent_path"] == submission_parent(agent, session_id, submission_id)
        ]
        return [journal_entry_dict(row) for row in rows]

    def read_session_messages(
        self, namespace: str, agent: str, session_id: str
    ) -> list[SessionMessage]:
        messages = []
        for row in self.read_kv_rows("SessionMessage"):
            if (
                row["namespace"] != namespace
                or row["parent_path"] != session_parent(agent, session_id)
            ):
                continue
            message = SessionMessage()
            message.ParseFromString(row["value"])
            messages.append(message)
        return messages

    def assistant_messages(
        self, namespace: str, agent: str, session_id: str
    ) -> list[SessionMessage]:
        return [
            message
            for message in self.read_session_messages(namespace, agent, session_id)
            if message.role == ROLE_ASSISTANT
        ]

    def seed_blocking_mcp_server(self) -> None:
        server = McpServer(
            metadata=ResourceMeta(name=BLOCKING_MCP_SERVER, namespace="Sys"),
            spec=McpServerSpec(
                transport="http",
                target=f"http://127.0.0.1:{conftest.MOCK_LLM_PORT}/mcp",
            ),
            status=CommonResourceStatus(phase="Ready"),
        )
        with sqlite3.connect(self.sqlite_db_path) as conn:
            conn.execute(
                """
                INSERT INTO talon_kv_store (namespace, parent_path, kind, name, value)
                VALUES (?, ?, ?, ?, ?)
                ON CONFLICT (namespace, parent_path, kind, name)
                DO UPDATE SET value = excluded.value
                """,
                (
                    "Sys",
                    "",
                    "MCPServer",
                    BLOCKING_MCP_SERVER,
                    server.SerializeToString(),
                ),
            )


class DurableSessionStack:
    """Subprocess harness for destructive durable-session stress tests."""

    def __init__(
        self,
        *,
        temp_dir: Path,
        data_dir: Path,
        env: dict[str, str],
        grpc_port: int,
        server_proc: subprocess.Popen,
        worker_proc: subprocess.Popen,
    ) -> None:
        self.temp_dir = temp_dir
        self.data_dir = data_dir
        self.env = env
        self.grpc_port = grpc_port
        self.server_proc = server_proc
        self.worker_proc = worker_proc
        self.worker_procs = [worker_proc]
        self.kv = DurableSessionKvProbe(data_dir)

    @classmethod
    def start(cls) -> DurableSessionStack:
        print("\nStarting isolated SQLite + local_socket durability stack...")
        test_grpc_port = unused_tcp_port()
        worker_port = unused_tcp_port()

        env = os.environ.copy()
        conftest.load_repo_dotenv_into_env(env, keys={"OPENAI_API_KEY", "CODEX_API_KEY"})
        env["RUST_LOG"] = "info"
        env["NOVITA_API_KEY"] = "test-dummy-key"
        env["GRPC_ADDR"] = f"127.0.0.1:{test_grpc_port}"
        env["PORT"] = str(worker_port)
        env["TALON_SESSION_PROCESSING_TIMEOUT_SECONDS"] = "1"

        temp_dir = Path(tempfile.mkdtemp(prefix="talon-durable-stress-"))
        data_dir = temp_dir / "data"
        data_dir.mkdir(parents=True, exist_ok=True)
        config_path = temp_dir / "talon.e2e.sqlite.yaml"
        config_path.write_text(
            f"""
providers:
  mock:
    type: openai_compatible
    name: mock
    base_url: "http://127.0.0.1:{conftest.MOCK_LLM_PORT}"
    model: minimax/minimax-m2.7
    api_key:
      source: env
      key: NOVITA_API_KEY
server:
  host: "127.0.0.1"
  port: {test_grpc_port}
control_plane:
  database:
    driver: sqlite
    data_dir: ./data
  message_broker:
    driver: local_socket
""".strip()
            + "\n"
        )
        env["TALON_CONFIG_PATH"] = str(config_path)

        server_proc, worker_proc = conftest.start_talon_server_and_worker(
            env,
            test_grpc_port,
            worker_pull_mode=True,
        )
        return cls(
            temp_dir=temp_dir,
            data_dir=data_dir,
            env=env,
            grpc_port=test_grpc_port,
            server_proc=server_proc,
            worker_proc=worker_proc,
        )

    @property
    def worker_port(self) -> int:
        return int(self.env["PORT"])

    def shutdown(self) -> None:
        print("\nShutting down isolated SQLite + local_socket durability stack...")
        for proc in self.worker_procs:
            if proc.poll() is None:
                proc.terminate()
        if self.server_proc.poll() is None:
            self.server_proc.terminate()
        for proc in [*self.worker_procs, self.server_proc]:
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=10)
        shutil.rmtree(self.temp_dir, ignore_errors=True)

    def kill_worker(self) -> None:
        if self.worker_proc.poll() is None:
            self.worker_proc.kill()
        self.worker_proc.wait(timeout=10)
        assert self.worker_proc.returncode is not None

    def start_worker(self) -> subprocess.Popen:
        env = self.env.copy()
        env["PULL_MODE"] = "1"
        worker = subprocess.Popen(
            [conftest.get_binary_path("talon_worker")],
            env=env,
            stdout=sys.stdout,
            stderr=sys.stderr,
        )
        self.worker_procs.append(worker)
        self.worker_proc = worker
        conftest.wait_for_gateway("127.0.0.1", self.worker_port)
        time.sleep(1.0)
        return worker

    def restart_worker(self) -> subprocess.Popen:
        self.kill_worker()
        return self.start_worker()


class SessionStreamBuffer:
    """Background gRPC session-stream subscriber used to assert live UI fan-out."""

    def __init__(
        self,
        *,
        grpc_port: int,
        namespace: str,
        agent: str,
        session_id: str,
    ) -> None:
        self.grpc_port = grpc_port
        self.namespace = namespace
        self.agent = agent
        self.session_id = session_id
        self._events: list[SessionMessagePartEvent] = []
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._ready = threading.Event()
        self._error: grpc.RpcError | BaseException | None = None
        self._channel: grpc.Channel | None = None
        self._thread: threading.Thread | None = None

    def __enter__(self) -> SessionStreamBuffer:
        self.start()
        return self

    def __exit__(self, *args: Any) -> None:
        self.stop()

    def start(self) -> None:
        self._channel = grpc.insecure_channel(f"127.0.0.1:{self.grpc_port}")
        self._thread = threading.Thread(
            target=self._run,
            name=f"session-stream-{self.session_id}",
            daemon=True,
        )
        self._thread.start()
        assert self._ready.wait(timeout=5), "session stream thread did not start"
        time.sleep(0.2)

    def stop(self) -> None:
        self._stop.set()
        if self._channel is not None:
            self._channel.close()
        if self._thread is not None:
            self._thread.join(timeout=5)

    def snapshot(self) -> list[SessionMessagePartEvent]:
        with self._lock:
            return list(self._events)

    def tool_part_events(
        self, *, part_type: int, tool_call_id: str
    ) -> list[SessionMessagePartEvent]:
        return [
            event
            for event in self.snapshot()
            if event.part.part_type == part_type
            and part_payload(event.part).get("tool_call_id") == tool_call_id
        ]

    def wait_for_tool_part_events(
        self,
        *,
        part_type: int,
        tool_call_id: str,
        count: int,
        label: str,
        attempts: int = 120,
        delay: float = 0.05,
    ) -> list[SessionMessagePartEvent]:
        for _ in range(attempts):
            events = self.tool_part_events(
                part_type=part_type, tool_call_id=tool_call_id
            )
            if len(events) >= count:
                return events
            time.sleep(delay)
        if self._error and not self._stop.is_set():
            raise AssertionError(f"{label} stream failed: {self._error!r}") from self._error
        observed_events = [
            {
                "kind": event.kind,
                "part_type": event.part.part_type,
                "content": event.part.content,
                "payload": event.part.payload_json,
                "message_id": event.message_id,
            }
            for event in self.snapshot()
        ]
        observed_count = len(
            self.tool_part_events(part_type=part_type, tool_call_id=tool_call_id)
        )
        raise AssertionError(
            f"timed out waiting for {label}; "
            f"observed={observed_count}; "
            f"stream_events={observed_events!r}"
        )

    def _run(self) -> None:
        assert self._channel is not None
        stub = TalonClient(self._channel)
        request = StreamSessionPartsRequest(
            session_id=self.session_id,
            agent=self.agent,
            ns=self.namespace,
        )
        try:
            stream = stub.sessions.StreamParts(request, timeout=120)
            stream.initial_metadata()
            self._ready.set()
            for event in stream:
                copied = SessionMessagePartEvent()
                copied.CopyFrom(event)
                with self._lock:
                    self._events.append(copied)
                if self._stop.is_set():
                    break
        except grpc.RpcError as exc:
            if not self._stop.is_set():
                self._error = exc
        except BaseException as exc:
            if not self._stop.is_set():
                self._error = exc
            raise
        finally:
            self._ready.set()


@pytest.fixture
def talon_infrastructure_sqlite() -> Iterator[DurableSessionStack]:
    """Isolate destructive worker-kill scenarios from one another."""
    stack = DurableSessionStack.start()
    try:
        yield stack
    finally:
        stack.shutdown()


@pytest.fixture
def sqlite_test_grpc_port(talon_infrastructure_sqlite: DurableSessionStack) -> int:
    return talon_infrastructure_sqlite.grpc_port


def mock_control(
    method: str, path: str, payload: dict[str, Any] | None = None, *, timeout: int = 5
) -> dict[str, Any]:
    url = f"http://127.0.0.1:{conftest.MOCK_LLM_PORT}{path}"
    response = requests.request(method, url, json=payload, timeout=timeout)
    response.raise_for_status()
    return response.json()


def wait_for_mock_blocked(*, attempts: int = 80, delay: float = 0.1) -> dict[str, Any]:
    state: dict[str, Any] = {}
    for _ in range(attempts):
        state = mock_control("GET", "/__control/state")
        if state.get("blocked"):
            return state
        time.sleep(delay)
    raise AssertionError(f"mock LLM did not block; final state={state}")


def wait_for_mock_mcp_tool_blocked(
    *, attempts: int = 120, delay: float = 0.05
) -> dict[str, Any]:
    state: dict[str, Any] = {}
    for _ in range(attempts):
        state = mock_control("GET", "/__control/state")
        if state.get("mcp_tool_blocked"):
            return state
        time.sleep(delay)
    raise AssertionError(f"mock MCP tool did not block; final state={state}")


def create_agent(
    stub: TalonClient,
    namespace: str,
    agent: str,
    *,
    mcp_server_refs: list[str] | None = None,
) -> None:
    stub.namespaces.Create(CreateNamespaceRequest(name=namespace, recursive=True))
    stub.resources.Create(
        CreateResourceRequest(
            ns=namespace,
            manifest=ResourceManifest(
                api_version="talon.impalasys.com/v1",
                kind="Agent",
                metadata=ResourceMeta(name=agent, namespace=namespace),
                spec=ResourceSpec(
                    agent=AgentSpec(
                        mcp_server_refs=mcp_server_refs or [],
                        model_policy={
                            "profiles": [
                                {
                                    "name": "default",
                                    "model": Model(
                                        provider="mock",
                                        name="minimax-m2.7",
                                        temperature=0.7,
                                    ),
                                }
                            ]
                        },
                        system_prompt="You are a durable session stress test assistant.",
                    )
                ),
            ),
        )
    )


def decode_proto_row(row: KvRow, message_type: type[T]) -> T:
    message: Any = message_type()
    message.ParseFromString(row["value"])
    return message


def submission_dict(row: KvRow) -> SubmissionRecord:
    submission = decode_proto_row(row, SessionSubmission)
    return {
        "_kv_name": row["name"],
        "submissionId": submission.submission_id,
        "sessionId": submission.session_id,
        "userMessageId": submission.user_message_id,
        "status": submission.status,
        "attemptId": submission.attempt_id,
        "attemptCount": submission.attempt_count,
        "claimExpiresAt": (
            submission.claim_expires_at if submission.HasField("claim_expires_at") else None
        ),
        "createdAt": submission.created_at,
        "updatedAt": submission.updated_at,
        "completedAt": (
            submission.completed_at if submission.HasField("completed_at") else None
        ),
        "committedMessageId": (
            submission.committed_message_id
            if submission.HasField("committed_message_id")
            else None
        ),
        "currentPhase": submission.current_phase,
        "currentJournalEntryId": (
            submission.current_journal_entry_id
            if submission.HasField("current_journal_entry_id")
            else None
        ),
    }


def journal_entry_dict(row: KvRow) -> JournalEntryRecord:
    entry = decode_proto_row(row, SessionJournalEntry)
    return {
        "submissionId": entry.submission_id,
        "journalEntryId": entry.journal_entry_id,
        "attemptId": entry.attempt_id,
        "phase": entry.phase,
        "payload": journal_payload_dict(entry),
        "createdAt": entry.created_at,
        "updatedAt": entry.updated_at,
        "committedAt": entry.committed_at if entry.HasField("committed_at") else None,
        "committedMessageId": (
            entry.committed_message_id if entry.HasField("committed_message_id") else None
        ),
    }


def journal_payload_dict(entry: SessionJournalEntry) -> dict[str, Any] | None:
    if not entry.HasField("payload"):
        return None
    payload_kind = entry.payload.WhichOneof("payload")
    if payload_kind == "llm_response":
        response = entry.payload.llm_response.response
        return {
            "llmResponse": {
                "content": response.content,
                "toolCalls": [
                    {
                        "id": tool.id,
                        "name": tool.name,
                        "arguments": tool.arguments,
                    }
                    for tool in response.tool_calls
                ],
            }
        }
    if payload_kind == "tool_result":
        result = entry.payload.tool_result
        return {
            "toolResult": {
                "toolCallId": result.tool_call_id,
                "name": result.name,
                "output": result.output,
            }
        }
    if payload_kind == "commit":
        return {
            "commit": {
                "committedMessageId": entry.payload.commit.committed_message_id,
            }
        }
    return None


def projection_state(message: SessionMessage) -> str | None:
    return message.labels.get("talon.session.projection_state") or None


def latest_projection(messages: list[SessionMessage]) -> SessionMessage | None:
    projections = [
        message
        for message in messages
        if message.labels.get("talon.session.submission_id")
    ]
    return projections[-1] if projections else None


def llm_response_tool_calls(entry: JournalEntryRecord) -> list[dict[str, Any]]:
    payload = entry["payload"] or {}
    response = payload.get("llmResponse") or {}
    return response.get("toolCalls") or []


def session_parent(agent: str, session_id: str) -> str:
    return f"Agent/{agent}/Session/{session_id}"


def submission_parent(agent: str, session_id: str, submission_id: str) -> str:
    return f"{session_parent(agent, session_id)}/SessionSubmission/{submission_id}"


def text_parts_for(message: SessionMessage) -> list[Any]:
    return [part for part in message.parts if part.part_type == PART_TYPE_TEXT]


def parts_for_type(message: SessionMessage, part_type: int) -> list[Any]:
    return [part for part in message.parts if part.part_type == part_type]


def part_payload(part: Any) -> dict[str, Any]:
    return json.loads(part.payload_json or "{}")


def assert_single_tool_call_and_result(
    message: SessionMessage, *, tool_call_id: str
) -> None:
    tool_call_parts = parts_for_type(message, PART_TYPE_TOOL_CALL)
    tool_result_parts = parts_for_type(message, PART_TYPE_TOOL_RESULT)
    assert len(tool_call_parts) == 1
    assert len(tool_result_parts) == 1
    assert part_payload(tool_call_parts[0])["tool_call_id"] == tool_call_id
    assert part_payload(tool_result_parts[0])["tool_call_id"] == tool_call_id


def assert_mock_llm_saw_user_message_once(mock_state: dict[str, Any], message: str) -> None:
    requests_seen = mock_state.get("chat_requests") or []
    assert requests_seen, "mock LLM should have received at least one chat request"
    for request in requests_seen:
        user_messages = [
            item
            for item in request.get("messages", [])
            if item.get("role") == "user" and item.get("content") == message
        ]
        assert len(user_messages) == 1, request.get("messages", [])


def text_from_parts(parts: list[Any]) -> str:
    return "".join(part.content for part in parts)


def entries_with_phase(
    entries: list[JournalEntryRecord], phase: int
) -> list[JournalEntryRecord]:
    return [entry for entry in entries if entry["phase"] == phase]


class SessionSnooper:
    """Scenario-level polling helpers for one durable session under test."""

    def __init__(
        self,
        *,
        stack: DurableSessionStack,
        stub: TalonClient,
        namespace: str,
        agent: str,
        session_id: str,
    ) -> None:
        self.stack = stack
        self.stub = stub
        self.namespace = namespace
        self.agent = agent
        self.session_id = session_id

    @property
    def kv(self) -> DurableSessionKvProbe:
        return self.stack.kv

    def assistant_messages(self) -> list[SessionMessage]:
        return self.kv.assistant_messages(self.namespace, self.agent, self.session_id)

    def _wait_for_condition(
        self,
        label: str,
        predicate: Callable[[], T | None],
        *,
        attempts: int = 80,
        delay: float = 0.1,
    ) -> T:
        last = None
        for _ in range(attempts):
            last = predicate()
            if last:
                return last
            time.sleep(delay)
        raise AssertionError(f"timed out waiting for {label}; last={last!r}")

    def wait_for_submission(self) -> SubmissionRecord:
        return self._wait_for_condition(
            "claimed submission",
            lambda: self.kv.read_submission(self.namespace, self.agent, self.session_id),
        )

    def wait_for_reclaimed_submission(
        self, submission_id: str, previous_attempt_count: int
    ) -> SubmissionRecord:
        def reclaimed_submission() -> SubmissionRecord | None:
            submission = self.kv.read_submission(
                self.namespace, self.agent, self.session_id, submission_id
            )
            if submission and submission["attemptCount"] > previous_attempt_count:
                return submission
            return None

        return self._wait_for_condition(
            "stream-triggered session submission reclaim",
            reclaimed_submission,
            attempts=120,
            delay=0.1,
        )

    def wait_for_in_progress_projection(self) -> SessionMessage:
        return self._wait_for_condition(
            "in-progress assistant projection",
            lambda: next(
                (
                    message
                    for message in reversed(self.assistant_messages())
                    if projection_state(message) == "in_progress"
                ),
                None,
            ),
            attempts=120,
            delay=0.1,
        )

    def wait_for_completed_session(self, label: str) -> Any:
        def completed_session() -> Any | None:
            try:
                session = self.stub.sessions.Get(
                    GetSessionRequest(
                        agent=self.agent, session_id=self.session_id, ns=self.namespace
                    )
                )
            except grpc.RpcError:
                return None
            assistants = [
                message for message in session.messages if message.role == ROLE_ASSISTANT
            ]
            if session.state in ("IDLE", "ERROR") and assistants:
                return session
            return None

        return self._wait_for_condition(label, completed_session, attempts=120, delay=0.25)

    def wait_for_llm_response_with_tool_calls_without_results(
        self,
    ) -> tuple[SubmissionRecord, list[JournalEntryRecord], JournalEntryRecord]:
        def observed_tool_call_before_result():
            submission = self.kv.read_submission(self.namespace, self.agent, self.session_id)
            if submission is None:
                return None

            submission_id = submission["_kv_name"]
            entries = self.kv.read_journal_entries(
                self.namespace, self.agent, self.session_id, submission_id
            )
            llm_tool_entries = [
                entry
                for entry in entries_with_phase(
                    entries, SESSION_EXECUTION_PHASE_LLM_RESPONSE
                )
                if llm_response_tool_calls(entry)
            ]
            tool_result_entries = entries_with_phase(
                entries, SESSION_EXECUTION_PHASE_TOOL_RESULT
            )
            if llm_tool_entries and not tool_result_entries:
                return submission, entries, llm_tool_entries[-1]
            return None

        return self._wait_for_condition(
            "LLM response with tool calls before any tool result",
            observed_tool_call_before_result,
            attempts=1000,
            delay=0.005,
        )

    def wait_for_tool_result_without_commit(
        self,
    ) -> tuple[SubmissionRecord, list[JournalEntryRecord]]:
        def observed_tool_result_before_commit():
            submission = self.kv.read_submission(self.namespace, self.agent, self.session_id)
            if submission is None:
                return None

            submission_id = submission["_kv_name"]
            entries = self.kv.read_journal_entries(
                self.namespace, self.agent, self.session_id, submission_id
            )
            if entries_with_phase(
                entries, SESSION_EXECUTION_PHASE_TOOL_RESULT
            ) and not entries_with_phase(entries, SESSION_EXECUTION_PHASE_COMMITTED):
                return submission, entries
            return None

        return self._wait_for_condition(
            "tool-result journal entry before commit",
            observed_tool_result_before_commit,
            attempts=1000,
            delay=0.005,
        )


@pytest.mark.stress
# Kill the worker while an LLM stream is in progress. This is meant to catch any
# regression where the UI projection is mistaken for durable recovery state or
# redelivery creates duplicate committed assistant messages.
def test_provider_started_worker_kill_restart_redelivery_is_non_polluting(
    talon_infrastructure_sqlite: DurableSessionStack,
    gateway_channel_sqlite: grpc.Channel,
    mock_llm_server: Any,
) -> None:
    infra: DurableSessionStack = talon_infrastructure_sqlite
    stub = TalonClient(gateway_channel_sqlite)
    namespace = f"durable-stress-{uuid.uuid4().hex[:8]}"
    agent = "stress-agent"
    create_agent(stub, namespace, agent)
    session_id = stub.sessions.Create(CreateSessionRequest(agent=agent, ns=namespace)).session_id
    snooper = SessionSnooper(
        stack=infra, stub=stub, namespace=namespace, agent=agent, session_id=session_id
    )

    mock_control("POST", "/__control/reset")
    mock_control("POST", "/__control/block_stream_after_chunks", {"chunks": 15})

    long_message = " ".join(f"token{i}" for i in range(40))
    stub.sessions.SendMessage(
        SendMessageRequest(
            agent=agent,
            session_id=session_id,
            ns=namespace,
            message=f"please stream a long durable response {long_message}",
        )
    )

    wait_for_mock_blocked()
    submission = snooper.wait_for_submission()
    submission_id = submission["_kv_name"]
    projection = snooper.wait_for_in_progress_projection()
    entries_before_crash = infra.kv.read_journal_entries(
        namespace, agent, session_id, submission_id
    )

    assert submission["status"] == SESSION_SUBMISSION_STATUS_CLAIMED
    assert entries_before_crash == []
    assert projection_state(projection) == "in_progress"
    partial_projection_text = text_from_parts(text_parts_for(projection))
    assert partial_projection_text

    infra.restart_worker()
    time.sleep(1.3)
    mock_control("POST", "/__control/unblock_stream")
    with SessionStreamBuffer(
        grpc_port=infra.grpc_port,
        namespace=namespace,
        agent=agent,
        session_id=session_id,
    ):
        snooper.wait_for_reclaimed_submission(
            submission_id, submission["attemptCount"]
        )
        session = snooper.wait_for_completed_session("session completion after worker restart")
    assistants = [message for message in session.messages if message.role == ROLE_ASSISTANT]
    users = [message for message in session.messages if message.role == ROLE_USER]
    assert len(users) == 1
    assert len(assistants) == 1
    assistant_parts = text_parts_for(assistants[0])
    assert assistant_parts
    assert "received your message" in text_from_parts(assistant_parts).lower()
    assert projection_state(assistants[0]) == "committed"

    final_submission = infra.kv.read_submission(namespace, agent, session_id, submission_id)
    assert final_submission is not None
    final_entries = infra.kv.read_journal_entries(namespace, agent, session_id, submission_id)
    final_entry = next(
        entry
        for entry in final_entries
        if entry["journalEntryId"] == final_submission["currentJournalEntryId"]
    )
    assert final_submission["status"] == SESSION_SUBMISSION_STATUS_COMMITTED
    assert final_submission["claimExpiresAt"] is None
    assert final_submission["attemptCount"] >= 2
    assert final_submission["committedMessageId"] == assistants[0].id
    assert final_submission["currentPhase"] == SESSION_EXECUTION_PHASE_COMMITTED
    assert final_entry["phase"] == SESSION_EXECUTION_PHASE_COMMITTED
    assert final_entry["committedMessageId"] == assistants[0].id
    assert len(entries_with_phase(final_entries, SESSION_EXECUTION_PHASE_LLM_RESPONSE)) == 1
    assert (
        len(
            [
                entry
                for entry in final_entries
                if entry["phase"] == SESSION_EXECUTION_PHASE_COMMITTED
            ]
        )
        == 1
    )

    canonical_texts = [text_from_parts(text_parts_for(message)) for message in assistants]
    assert all(projection_state(message) == "committed" for message in assistants)
    assert all(text != partial_projection_text for text in canonical_texts)


@pytest.mark.stress
# Kill the worker at the harder recovery boundary: after the completed LLM
# response with tool calls is durably journaled, but before any tool result is
# recorded. Restart should execute pending tool work from the journal instead of
# replaying the original LLM call and appending a duplicate tool-discovery entry.
def test_tool_call_recorded_worker_kill_restart_recovers_from_journal(
    talon_infrastructure_sqlite: DurableSessionStack,
    gateway_channel_sqlite: grpc.Channel,
    mock_llm_server: Any,
) -> None:
    infra: DurableSessionStack = talon_infrastructure_sqlite
    stub = TalonClient(gateway_channel_sqlite)
    namespace = f"durable-tool-recovery-{uuid.uuid4().hex[:8]}"
    agent = "stress-agent"
    infra.kv.seed_blocking_mcp_server()
    create_agent(stub, namespace, agent, mcp_server_refs=[BLOCKING_MCP_SERVER])
    session_id = stub.sessions.Create(CreateSessionRequest(agent=agent, ns=namespace)).session_id
    snooper = SessionSnooper(
        stack=infra, stub=stub, namespace=namespace, agent=agent, session_id=session_id
    )

    mock_control("POST", "/__control/reset")
    mock_control("POST", "/__control/block_mcp_tool")

    message = "Please run a blocking lookup docs.example.com and summarize what you found."
    stub.sessions.SendMessage(
        SendMessageRequest(
            agent=agent,
            session_id=session_id,
            ns=namespace,
            message=message,
        )
    )

    mock_state_at_crash = wait_for_mock_mcp_tool_blocked()
    submission, entries_before_crash, llm_tool_entry = (
        snooper.wait_for_llm_response_with_tool_calls_without_results()
    )
    submission_id = submission["_kv_name"]

    assert llm_tool_entry["phase"] == SESSION_EXECUTION_PHASE_LLM_RESPONSE
    assert any(
        tool["name"] == BLOCKING_MCP_TOOL
        for tool in llm_response_tool_calls(llm_tool_entry)
    )
    assert entries_with_phase(
        entries_before_crash, SESSION_EXECUTION_PHASE_TOOL_RESULT
    ) == []
    committed_assistants = [
        message
        for message in snooper.assistant_messages()
        if projection_state(message) == "committed"
    ]
    assert committed_assistants == []

    infra.kill_worker()
    time.sleep(1.3)
    mock_control("POST", "/__control/unblock_mcp_tool")
    infra.start_worker()
    with SessionStreamBuffer(
        grpc_port=infra.grpc_port,
        namespace=namespace,
        agent=agent,
        session_id=session_id,
    ):
        snooper.wait_for_reclaimed_submission(
            submission_id, submission["attemptCount"]
        )
        session = snooper.wait_for_completed_session(
            "tool-call journal recovery after worker restart"
        )
    assistants = [message for message in session.messages if message.role == ROLE_ASSISTANT]
    assert len(assistants) == 1
    assistant_parts = text_parts_for(assistants[0])
    assert assistant_parts
    assert "blocking_lookup" in text_from_parts(assistant_parts)
    assert_single_tool_call_and_result(
        assistants[0], tool_call_id=BLOCKING_TOOL_CALL_ID
    )

    final_submission = infra.kv.read_submission(namespace, agent, session_id, submission_id)
    assert final_submission is not None
    final_entries = infra.kv.read_journal_entries(namespace, agent, session_id, submission_id)
    final_mock_state = mock_control("GET", "/__control/state")

    assert final_submission["status"] == SESSION_SUBMISSION_STATUS_COMMITTED
    assert final_submission["currentPhase"] == SESSION_EXECUTION_PHASE_COMMITTED
    assert final_submission["committedMessageId"] == assistants[0].id
    assert (
        len(
            [
                entry
                for entry in entries_with_phase(
                    final_entries, SESSION_EXECUTION_PHASE_LLM_RESPONSE
                )
                if llm_response_tool_calls(entry)
            ]
        )
        == 1
    )
    assert (
        len(entries_with_phase(final_entries, SESSION_EXECUTION_PHASE_TOOL_RESULT))
        == 1
    )
    assert len(entries_with_phase(final_entries, SESSION_EXECUTION_PHASE_COMMITTED)) == 1

    # Correct recovery should execute the journaled pending tool call, then make
    # only the follow-up LLM call needed to produce the final assistant answer.
    assert final_mock_state["request_count"] == mock_state_at_crash["request_count"] + 1
    assert_mock_llm_saw_user_message_once(final_mock_state, message)


@pytest.mark.stress
# Kill the worker after the tool result is durably journaled but before the
# final assistant commit. This catches duplicate sink/projection replay: the
# recovered final SessionMessage must contain exactly one tool call part and one
# matching tool result part, and the MCP tool must not run a second time.
def test_tool_result_recorded_worker_kill_restart_does_not_duplicate_message_parts(
    talon_infrastructure_sqlite: DurableSessionStack,
    gateway_channel_sqlite: grpc.Channel,
    mock_llm_server: Any,
) -> None:
    infra: DurableSessionStack = talon_infrastructure_sqlite
    stub = TalonClient(gateway_channel_sqlite)
    namespace = f"durable-tool-result-recovery-{uuid.uuid4().hex[:8]}"
    agent = "stress-agent"
    infra.kv.seed_blocking_mcp_server()
    create_agent(stub, namespace, agent, mcp_server_refs=[BLOCKING_MCP_SERVER])
    session_id = stub.sessions.Create(CreateSessionRequest(agent=agent, ns=namespace)).session_id
    snooper = SessionSnooper(
        stack=infra, stub=stub, namespace=namespace, agent=agent, session_id=session_id
    )

    mock_control("POST", "/__control/reset")
    mock_control("POST", "/__control/block_mcp_tool")
    mock_control("POST", "/__control/block_stream_after_chunks", {"chunks": 1})

    stream = SessionStreamBuffer(
        grpc_port=infra.grpc_port,
        namespace=namespace,
        agent=agent,
        session_id=session_id,
    )
    with stream:
        message = "Please run a blocking lookup docs.example.com and summarize what you found."
        stub.sessions.SendMessage(
            SendMessageRequest(
                agent=agent,
                session_id=session_id,
                ns=namespace,
                message=message,
            )
        )

        wait_for_mock_mcp_tool_blocked()
        submission = snooper.wait_for_submission()
        submission_id = submission["_kv_name"]
        mock_control("POST", "/__control/unblock_mcp_tool")
        wait_for_mock_blocked()
        stream.wait_for_tool_part_events(
            part_type=PART_TYPE_TOOL_CALL,
            tool_call_id=BLOCKING_TOOL_CALL_ID,
            count=1,
            label="initial streamed tool call",
        )
        stream.wait_for_tool_part_events(
            part_type=PART_TYPE_TOOL_RESULT,
            tool_call_id=BLOCKING_TOOL_CALL_ID,
            count=1,
            label="initial streamed tool result",
        )
        submission, entries_before_crash = snooper.wait_for_tool_result_without_commit()
        mock_state_at_crash = mock_control("GET", "/__control/state")

        assert (
            len(entries_with_phase(entries_before_crash, SESSION_EXECUTION_PHASE_TOOL_RESULT))
            == 1
        )
        assert entries_with_phase(entries_before_crash, SESSION_EXECUTION_PHASE_COMMITTED) == []
        assert mock_state_at_crash["mcp_tool_call_count"] == 1

        infra.kill_worker()
    time.sleep(1.3)
    mock_control("POST", "/__control/unblock_stream")
    infra.start_worker()
    with SessionStreamBuffer(
        grpc_port=infra.grpc_port,
        namespace=namespace,
        agent=agent,
        session_id=session_id,
    ):
        snooper.wait_for_reclaimed_submission(
            submission_id, submission["attemptCount"]
        )

        session = snooper.wait_for_completed_session(
            "tool-result journal recovery after worker restart"
        )
        assistants = [message for message in session.messages if message.role == ROLE_ASSISTANT]
        assert len(assistants) == 1
        assert "blocking_lookup" in text_from_parts(text_parts_for(assistants[0]))
        assert_single_tool_call_and_result(
            assistants[0], tool_call_id=BLOCKING_TOOL_CALL_ID
        )

        final_entries = infra.kv.read_journal_entries(namespace, agent, session_id, submission_id)
        final_mock_state = mock_control("GET", "/__control/state")
        streamed_tool_calls = stream.tool_part_events(
            part_type=PART_TYPE_TOOL_CALL,
            tool_call_id=BLOCKING_TOOL_CALL_ID,
        )
        streamed_tool_results = stream.tool_part_events(
            part_type=PART_TYPE_TOOL_RESULT,
            tool_call_id=BLOCKING_TOOL_CALL_ID,
        )

    assert len(entries_with_phase(final_entries, SESSION_EXECUTION_PHASE_TOOL_RESULT)) == 1
    assert len(entries_with_phase(final_entries, SESSION_EXECUTION_PHASE_COMMITTED)) == 1
    assert final_mock_state["mcp_tool_call_count"] == 1
    assert len(streamed_tool_calls) == 1
    assert len(streamed_tool_results) == 1
