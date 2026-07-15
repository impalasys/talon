import os
import shutil
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Iterator

import pytest

# Important: Add local test helpers and generated client packages to the import path.
REPO_ROOT = Path(__file__).resolve().parents[1]
PYTHON_SDK_SRC = REPO_ROOT / "sdk" / "python" / "talon-client" / "src"
sys.path.insert(0, str(PYTHON_SDK_SRC))
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))
sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))

from talon_client import TalonClient  # noqa: E402
from talon_client.auth import ApiKeyTokenSource  # noqa: E402

from e2e.stack import (  # noqa: E402
    E2E_JWT_ISSUER,
    E2E_JWT_PRIVATE_KEY_PEM,
    E2EStack,
    MOCK_LLM_PORT,
    REPO_ROOT as STACK_REPO_ROOT,
    authenticated_gateway_channel,
    binary_candidates,
    get_binary_path,
    get_runfile_binary_path,
    load_repo_dotenv_values,
    start_aws_local_stack,
    start_postgres_pubsub_stack,
    start_rocksdb_local_stack,
    start_sqlite_local_stack,
)

REPO_ROOT = STACK_REPO_ROOT


@dataclass(frozen=True)
class LlmServer:
    base_url: str


def api_key_env_name(grpc_port: int) -> str:
    return f"TALON_E2E_API_KEY_{grpc_port}"


def api_key_auth_file_env_name(grpc_port: int) -> str:
    return f"TALON_E2E_AUTH_FILE_{grpc_port}"


@pytest.fixture(scope="session", autouse=True)
def llm_server() -> Iterator[LlmServer]:
    print("\nStarting mock LLM server...")
    import socket
    server = LlmServer(base_url=f"http://127.0.0.1:{MOCK_LLM_PORT}")

    try:
        with socket.create_connection(("127.0.0.1", MOCK_LLM_PORT), timeout=1):
            print(f"\nReusing existing mock LLM server on 127.0.0.1:{MOCK_LLM_PORT}")
            yield server
            return
    except OSError:
        pass

    import threading
    import uvicorn
    from mock_llm import app

    server_thread = threading.Thread(
        target=uvicorn.run,
        args=(app,),
        kwargs={"host": "0.0.0.0", "port": MOCK_LLM_PORT, "log_level": "info"},
        daemon=True,
    )
    server_thread.start()

    for _ in range(10):
        try:
            with socket.create_connection(("127.0.0.1", MOCK_LLM_PORT), timeout=1):
                break
        except OSError:
            time.sleep(0.5)
    else:
        raise RuntimeError(
            f"Mock LLM server failed to start on 127.0.0.1:{MOCK_LLM_PORT}"
        )

    yield server
    print("\nShutting down mock LLM server...")


@pytest.fixture(scope="session")
def mock_llm_server(llm_server: LlmServer) -> LlmServer:
    return llm_server


@pytest.fixture(scope="session")
def postgres_pubsub_stack(llm_server: LlmServer) -> Iterator[E2EStack]:
    stack = start_postgres_pubsub_stack(grpc_port=50061, worker_port=8081)
    os.environ[api_key_env_name(stack.grpc_port)] = stack.api_key
    os.environ[api_key_auth_file_env_name(stack.grpc_port)] = stack.auth_file
    try:
        yield stack
    finally:
        os.environ.pop(api_key_env_name(stack.grpc_port), None)
        os.environ.pop(api_key_auth_file_env_name(stack.grpc_port), None)
        stack.stop()


@pytest.fixture(scope="session")
def sqlite_local_stack(llm_server: LlmServer) -> Iterator[E2EStack]:
    stack = start_sqlite_local_stack(grpc_port=50054, worker_port=18082)
    os.environ[api_key_env_name(stack.grpc_port)] = stack.api_key
    os.environ[api_key_auth_file_env_name(stack.grpc_port)] = stack.auth_file
    try:
        yield stack
    finally:
        os.environ.pop(api_key_env_name(stack.grpc_port), None)
        os.environ.pop(api_key_auth_file_env_name(stack.grpc_port), None)
        stack.stop()


@pytest.fixture(scope="session")
def rocksdb_local_stack(llm_server: LlmServer) -> Iterator[E2EStack]:
    stack = start_rocksdb_local_stack(grpc_port=50055)
    os.environ[api_key_env_name(stack.grpc_port)] = stack.api_key
    os.environ[api_key_auth_file_env_name(stack.grpc_port)] = stack.auth_file
    try:
        yield stack
    finally:
        os.environ.pop(api_key_env_name(stack.grpc_port), None)
        os.environ.pop(api_key_auth_file_env_name(stack.grpc_port), None)
        stack.stop()


@pytest.fixture(scope="session")
def aws_local_stack(llm_server: LlmServer) -> Iterator[E2EStack]:
    stack = start_aws_local_stack()
    os.environ[api_key_env_name(stack.grpc_port)] = stack.api_key
    os.environ[api_key_auth_file_env_name(stack.grpc_port)] = stack.auth_file
    try:
        yield stack
    finally:
        os.environ.pop(api_key_env_name(stack.grpc_port), None)
        os.environ.pop(api_key_auth_file_env_name(stack.grpc_port), None)
        stack.stop()


@pytest.fixture(scope="session")
def talon_infrastructure(postgres_pubsub_stack):
    return postgres_pubsub_stack


@pytest.fixture(scope="session")
def talon_infrastructure_sqlite(sqlite_local_stack):
    return sqlite_local_stack


@pytest.fixture
def test_grpc_port(talon_infrastructure):
    return talon_infrastructure.grpc_port


@pytest.fixture
def sqlite_test_grpc_port(talon_infrastructure_sqlite):
    return talon_infrastructure_sqlite.grpc_port


@pytest.fixture
def gateway_channel(talon_infrastructure, test_grpc_port):
    if hasattr(talon_infrastructure, "channel"):
        raw_channel, channel = talon_infrastructure.channel()
    else:
        raw_channel, channel = authenticated_gateway_channel(
            test_grpc_port,
            talon_infrastructure["api_key"],
        )
    yield channel
    raw_channel.close()


@pytest.fixture
def gateway_channel_sqlite(talon_infrastructure_sqlite, sqlite_test_grpc_port):
    if hasattr(talon_infrastructure_sqlite, "channel"):
        raw_channel, channel = talon_infrastructure_sqlite.channel()
        yield channel
        raw_channel.close()
        return

    if isinstance(talon_infrastructure_sqlite, dict):
        api_key = talon_infrastructure_sqlite.get("api_key")
    else:
        api_key = getattr(talon_infrastructure_sqlite, "api_key", None)
    if api_key:
        raw_channel, channel = authenticated_gateway_channel(
            sqlite_test_grpc_port,
            api_key,
        )
        yield channel
        raw_channel.close()
        return

    import grpc

    channel = grpc.insecure_channel(f"127.0.0.1:{sqlite_test_grpc_port}")
    yield channel
    channel.close()


def configured_stack_fixture_names() -> list[str]:
    configured = os.environ.get(
        "TALON_E2E_STACKS",
        "rocksdb_local,postgres_pubsub,sqlite_local,aws_local",
    )
    aliases = {
        "rocksdb": "rocksdb_local_stack",
        "rocksdb_local": "rocksdb_local_stack",
        "rocksdb-local": "rocksdb_local_stack",
        "postgres": "postgres_pubsub_stack",
        "postgres_pubsub": "postgres_pubsub_stack",
        "postgres-pubsub": "postgres_pubsub_stack",
        "sqlite": "sqlite_local_stack",
        "sqlite_local": "sqlite_local_stack",
        "sqlite-local": "sqlite_local_stack",
        "aws": "aws_local_stack",
        "aws_local": "aws_local_stack",
        "aws-local": "aws_local_stack",
    }
    stacks = []
    for raw_name in configured.split(","):
        name = raw_name.strip()
        if not name:
            continue
        if name not in aliases:
            raise ValueError(f"Unsupported TALON_E2E_STACKS entry: {name}")
        stacks.append(aliases[name])
    if not stacks:
        raise ValueError("TALON_E2E_STACKS must select at least one stack")
    return stacks


@pytest.fixture(
    params=configured_stack_fixture_names(),
    ids=lambda name: name.removesuffix("_stack").replace("_", "-"),
)
def stack(request):
    return request.getfixturevalue(request.param)


@pytest.fixture
def shared_e2e_stack(stack):
    return stack


@pytest.fixture
def client(stack: E2EStack) -> Iterator[TalonClient]:
    raw_channel, channel = stack.channel()
    try:
        yield TalonClient(channel)
    finally:
        raw_channel.close()
