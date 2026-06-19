import json
import time
from pathlib import Path


MANIFEST_ROOT = Path(__file__).resolve().parent / "manifests"


def cli_vars(values):
    args = []
    for key, value in values.items():
        args.extend(["--var", f"{key}={value}"])
    return args


def render(cli, path, variables=None, *, fmt="yaml"):
    return cli.run(
        "render",
        "-f",
        path,
        "--format",
        fmt,
        *(cli_vars(variables or {})),
    ).stdout


def apply(cli, path, variables=None, *, timeout=None):
    return cli.run("apply", "-f", path, *(cli_vars(variables or {})), timeout=timeout).stdout


def get_resource(cli, kind, name, namespace):
    return cli.json("get", kind, name, "--namespace", namespace, "--output", "json")


def list_resources(cli, kind, namespace):
    return cli.json("get", kind, "--namespace", namespace, "--output", "json").get(
        "resources", []
    )


def wait_for_resource(cli, kind, name, namespace, *, attempts=30, delay=1):
    last_error = None
    for _ in range(attempts):
        try:
            return get_resource(cli, kind, name, namespace)
        except AssertionError as err:
            last_error = err
            time.sleep(delay)
    raise AssertionError(
        f"Timed out waiting for {kind} {namespace}/{name}; last error: {last_error}"
    )


def wait_for_resources(cli, kind, names, namespace, *, attempts=30, delay=1):
    expected = set(names)
    last_names = set()
    for _ in range(attempts):
        resources = list_resources(cli, kind, namespace)
        last_names = {resource.get("metadata", {}).get("name") for resource in resources}
        if expected.issubset(last_names):
            return resources
        time.sleep(delay)
    raise AssertionError(
        f"Timed out waiting for {kind} resources {sorted(expected)} in {namespace}; "
        f"saw {sorted(name for name in last_names if name)}"
    )


def session_create(cli, namespace, agent):
    return cli.json("session", "create", "--namespace", namespace, "--agent", agent)[
        "sessionId"
    ]


def session_send(cli, namespace, agent, session_id, message, *, timeout=None):
    return cli.json(
        "session",
        "send",
        "--namespace",
        namespace,
        "--agent",
        agent,
        session_id,
        message,
        timeout=timeout,
    )


def session_permission_answer(
    cli,
    namespace,
    agent,
    session_id,
    request_id,
    *,
    option_id="approved",
    decided_by="e2e",
):
    return cli.json(
        "session",
        "permission",
        "answer",
        "--namespace",
        namespace,
        "--agent",
        agent,
        session_id,
        request_id,
        "--option-id",
        option_id,
        "--decided-by",
        decided_by,
    )


def session_send_stream_json(cli, namespace, agent, session_id, message, *, timeout=None):
    result = cli.run(
        "session",
        "send",
        "--namespace",
        namespace,
        "--agent",
        agent,
        "--stream",
        "--json",
        session_id,
        message,
        timeout=timeout,
    )
    return [
        json.loads(line)
        for line in result.stdout.splitlines()
        if line.strip()
    ]


def session_get(cli, namespace, agent, session_id, *, message_limit=0):
    return cli.json(
        "session",
        "get",
        "--namespace",
        namespace,
        "--agent",
        agent,
        session_id,
        "--message-limit",
        message_limit,
    )


def assistant_messages(session):
    return [
        message
        for message in session.get("messages", [])
        if message.get("role") in ("assistant", "ROLE_ASSISTANT", 2)
    ]


def session_part_type(part):
    return part.get("type") or part.get("partType")


def message_text(message):
    return "".join(
        part.get("content", "")
        for part in message.get("parts", [])
        if session_part_type(part) in ("text", "SESSION_MESSAGE_PART_TYPE_TEXT", 1)
    )


def wait_for_session_text(
    cli,
    namespace,
    agent,
    session_id,
    expected_text,
    *,
    attempts=30,
    delay=1,
):
    last_state = None
    last_text = ""
    for _ in range(attempts):
        time.sleep(delay)
        session = session_get(cli, namespace, agent, session_id)
        last_state = session.get("state")
        messages = assistant_messages(session)
        if last_state == "IDLE" and messages:
            last_text = message_text(messages[-1])
            if expected_text in last_text:
                return session
    raise AssertionError(
        f"Timed out waiting for session text {expected_text!r}; "
        f"state={last_state!r}, last_assistant_text={last_text!r}"
    )


def session_parts(session, part_type=None):
    parts = []
    for message in session.get("messages", []):
        for part in message.get("parts", []):
            if part_type is None or session_part_type(part) == part_type:
                parts.append(part)
    return parts


def wait_for_session_part(
    cli,
    namespace,
    agent,
    session_id,
    part_type,
    *,
    attempts=30,
    delay=1,
):
    last_part_types = []
    for _ in range(attempts):
        session = session_get(cli, namespace, agent, session_id)
        parts = session_parts(session)
        last_part_types = [session_part_type(part) for part in parts]
        for part in parts:
            if session_part_type(part) == part_type:
                return part, session
        time.sleep(delay)
    raise AssertionError(
        f"Timed out waiting for session part {part_type!r}; saw {last_part_types}"
    )


def pretty(value):
    return json.dumps(value, indent=2, sort_keys=True)
