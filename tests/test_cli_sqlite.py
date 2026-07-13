import hashlib
import time
import uuid
from pathlib import Path

import pytest

from e2e import scenarios as e2e
from e2e.stack import E2EStack


def file_resource_name_for_path(path: str) -> str:
    slug = "".join(
        ch.lower() if ch.isascii() and ch.isalnum() else "-"
        for ch in path.strip("/")
    )
    slug = slug.strip("-")[:48] or "file"
    digest = hashlib.sha256(path.encode()).hexdigest()
    return f"{slug}-{digest[:12]}"


@pytest.fixture
def stack(sqlite_local_stack: E2EStack) -> E2EStack:
    return sqlite_local_stack


def test_cli_chat_sqlite_local_socket(
    stack: E2EStack,
) -> None:
    # Verify the CLI can create an agent, open a session, send a message, and
    # observe a completed assistant reply on the SQLite/local-socket stack.
    cli = stack.cli()
    suffix = uuid.uuid4().hex[:8]
    namespace = f"talon-cli-chat-{suffix}"
    agent = "cli-chat-agent"
    e2e.apply(
        cli,
        e2e.MANIFEST_ROOT / "chat" / "agent.yaml",
        {
            "namespace": namespace,
            "agent": agent,
            "system_prompt": "You are a helpful CLI E2E test assistant.",
        },
    )
    session_id = e2e.session_create(cli, namespace, agent)
    e2e.session_send(
        cli,
        namespace,
        agent,
        session_id,
        "What is the square root of 144?",
    )
    completed = e2e.wait_for_session_text(cli, namespace, agent, session_id, "12")
    assert completed["state"] == "IDLE"


def test_cli_filtered_search_sqlite_local_socket(
    stack: E2EStack,
) -> None:
    # Verify the CLI session-search command can find indexed session content
    # after a message is sent on the SQLite/local-socket stack.
    cli = stack.cli()
    suffix = uuid.uuid4().hex[:8]
    namespace = f"talon-cli-search-{suffix}"
    agent = "cli-search-agent"
    token = f"clisearchtoken{suffix}"
    e2e.apply(
        cli,
        e2e.MANIFEST_ROOT / "chat" / "agent.yaml",
        {
            "namespace": namespace,
            "agent": agent,
            "system_prompt": "You are a helpful search CLI E2E test assistant.",
        },
    )
    session_id = e2e.session_create(cli, namespace, agent)
    e2e.session_send(
        cli,
        namespace,
        agent,
        session_id,
        f"Please index {token} for CLI search.",
    )

    last_output = ""
    for _ in range(30):
        result = cli.run(
            "search",
            "sessions",
            "--namespace",
            namespace,
            "--agent",
            agent,
            token,
        )
        last_output = result.stdout
        if token in last_output:
            break
        time.sleep(1)

    assert token in last_output
    assert namespace in last_output
    assert "SessionMessage" in last_output
    assert "part" in last_output


def test_cli_streaming_chat_sqlite_local_socket(
    stack: E2EStack,
) -> None:
    # Verify the CLI streaming path emits reasoning, text, and usage parts for a
    # normal assistant response on the SQLite/local-socket stack.
    cli = stack.cli()
    suffix = uuid.uuid4().hex[:8]
    namespace = f"talon-cli-stream-{suffix}"
    agent = "cli-stream-agent"
    e2e.apply(
        cli,
        e2e.MANIFEST_ROOT / "chat" / "agent.yaml",
        {
            "namespace": namespace,
            "agent": agent,
            "system_prompt": "You are a helpful streaming CLI E2E test assistant.",
        },
    )

    session_id = e2e.session_create(cli, namespace, agent)
    events = e2e.session_send_stream_json(
        cli,
        namespace,
        agent,
        session_id,
        "Stream test message",
    )
    parts = [
        event.get("part") or {}
        for event in events
        if event.get("part") is not None
    ]
    reasoning = [part for part in parts if part.get("type") == "reasoning"]
    text = [part for part in parts if part.get("type") == "text"]
    usage = [part for part in parts if part.get("type") == "usage"]

    assert reasoning
    assert text
    assert usage
    assert "Inspecting the request" in reasoning[0].get("content", "")
    assert "received" in "".join(part.get("content", "") for part in text)


def test_cli_apply_rejects_status_in_resource_manifest(
    stack: E2EStack,
    tmp_path: Path,
) -> None:
    # Verify `talon apply` rejects manifests that try to set resource status,
    # which should remain server-owned.
    manifest = tmp_path / "agent-with-status.yaml"
    manifest.write_text(
        """
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: status-owned
  namespace: status-owned-test
spec:
  systemPrompt: This manifest should fail before apply.
status:
  phase: Ready
"""
    )
    cli = stack.cli()
    result = cli.run("apply", "-f", str(manifest), check=False)

    assert result.returncode != 0
    assert "Resource manifests cannot set status" in (result.stderr + result.stdout)


def test_cli_apply_file_manifest_with_symbolic_enums_sqlite_local_socket(
    stack: E2EStack,
    tmp_path: Path,
) -> None:
    cli = stack.cli()
    suffix = uuid.uuid4().hex[:8]
    namespace = f"talon-cli-file-yaml-{suffix}"
    file_name = file_resource_name_for_path("/memory/brand-guidelines.md")
    manifest = tmp_path / "file.yaml"
    manifest.write_text(
        f"""
apiVersion: talon.impalasys.com/v1
kind: Namespace
metadata:
  name: {namespace}
---
apiVersion: talon.impalasys.com/v1
kind: File
metadata:
  name: {file_name}
  namespace: {namespace}
spec:
  path: /memory/brand-guidelines.md
  mediaType: text/markdown
  purpose: MEMORY
  indexPolicy: RETRIEVAL
  retention: RETAINED
"""
    )

    cli.run("apply", "-f", str(manifest))
    resource = e2e.get_resource(
        cli,
        "file",
        file_name,
        namespace,
    )

    assert resource["kind"] == "File"
    assert resource["spec"]["purpose"] == "MEMORY"
    assert resource["spec"]["indexPolicy"] == "RETRIEVAL"
    assert resource["spec"]["retention"] == "RETAINED"

    rendered = cli.run(
        "get",
        "file",
        file_name,
        "--namespace",
        namespace,
        "--output",
        "yaml",
    ).stdout
    assert "purpose: MEMORY" in rendered
    assert "indexPolicy: RETRIEVAL" in rendered
    assert "retention: RETAINED" in rendered


def test_cli_file_commands_round_trip_sqlite_local_socket(
    stack: E2EStack,
    tmp_path: Path,
) -> None:
    cli = stack.cli()
    suffix = uuid.uuid4().hex[:8]
    namespace = f"talon-cli-file-roundtrip-{suffix}"
    manifest = tmp_path / "namespace.yaml"
    manifest.write_text(
        f"""
apiVersion: talon.impalasys.com/v1
kind: Namespace
metadata:
  name: {namespace}
"""
    )
    cli.run("apply", "-f", str(manifest))

    source = tmp_path / "source.md"
    source.write_text("# Draft\n\nInitial guidance.\n")
    path = "/memory/brand-guidelines.md"

    put = cli.run(
        "file",
        "put",
        "--namespace",
        namespace,
        "--path",
        path,
        "--file",
        source,
        "--media-type",
        "text/markdown",
        "--purpose",
        "memory",
        "--index-policy",
        "retrieval",
    )
    assert "written" in put.stdout

    downloaded = tmp_path / "downloaded.md"
    cli.run(
        "file",
        "get",
        "--namespace",
        namespace,
        "--path",
        path,
        "--output",
        downloaded,
    )
    assert downloaded.read_text() == source.read_text()

    listed = cli.run(
        "file",
        "list",
        "--namespace",
        namespace,
        "--prefix",
        "/memory",
    ).stdout
    assert path in listed
    assert "text/markdown" in listed

    updated = tmp_path / "updated.md"
    updated.write_text("# Draft\n\nUpdated guidance.\n")
    update = cli.run(
        "file",
        "update",
        "--namespace",
        namespace,
        "--path",
        path,
        "--file",
        updated,
        "--media-type",
        "text/markdown",
    )
    assert "updated" in update.stdout

    after_update = tmp_path / "after-update.md"
    cli.run(
        "file",
        "get",
        "--namespace",
        namespace,
        "--path",
        path,
        "--output",
        after_update,
    )
    assert after_update.read_text() == updated.read_text()

    delete = cli.run(
        "file",
        "delete",
        "--namespace",
        namespace,
        "--path",
        path,
    )
    assert "Deleted: true" in delete.stdout

    missing = cli.run(
        "file",
        "get",
        "--namespace",
        namespace,
        "--path",
        path,
        check=False,
    )
    assert missing.returncode != 0
    assert "not found" in (missing.stderr + missing.stdout).lower()


def test_cli_apply_task_manifest_with_string_type_sqlite_local_socket(
    stack: E2EStack,
    tmp_path: Path,
) -> None:
    cli = stack.cli()
    suffix = uuid.uuid4().hex[:8]
    namespace = f"talon-cli-task-yaml-{suffix}"
    manifest = tmp_path / "task.yaml"
    manifest.write_text(
        f"""
apiVersion: talon.impalasys.com/v1
kind: Namespace
metadata:
  name: {namespace}
---
apiVersion: talon.impalasys.com/v1
kind: Task
metadata:
  name: launch-copy-{suffix}
  namespace: {namespace}
spec:
  title: Launch copy
  description: Draft launch copy.
  type: agent_delegation
  requester:
    namespace: {namespace}
    name: cmo
  assignee:
    namespace: {namespace}
    name: writer
"""
    )

    cli.run("apply", "-f", str(manifest))
    resource = e2e.get_resource(cli, "task", f"launch-copy-{suffix}", namespace)

    assert resource["kind"] == "Task"
    assert resource["spec"]["type"] == "agent_delegation"

    rendered = cli.run(
        "get",
        "task",
        f"launch-copy-{suffix}",
        "--namespace",
        namespace,
        "--output",
        "yaml",
    ).stdout
    assert "type: agent_delegation" in rendered
