from __future__ import annotations

import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from pathlib import Path

import boto3
import grpc
import pytest
from testcontainers.core.container import DockerContainer

from talon_client import (
    CreateNamespaceRequest,
    CreateResourceRequest,
    CreateSessionRequest,
    GetSessionRequest,
    SendMessageRequest,
    StreamSessionPartsRequest,
    TalonClient,
)
from talon_client.resources import AgentSpec, Model, ResourceManifest, ResourceMeta, ResourceSpec

import conftest


PART_TYPE_TEXT = 1


def unused_tcp_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def message_text(message):
    return "".join(part.content for part in message.parts if part.part_type == PART_TYPE_TEXT)


def last_assistant_message(messages):
    assistants = [message for message in messages if message.role == 2]
    return assistants[-1] if assistants else None


def ensure_namespace(stub, name):
    try:
        stub.namespaces.Create(CreateNamespaceRequest(name=name, recursive=True))
    except grpc.RpcError as err:
        if err.code() != grpc.StatusCode.ALREADY_EXISTS:
            raise


def create_agent_resource(stub, ns, name, spec):
    return stub.resources.Create(
        CreateResourceRequest(
            ns=ns,
            manifest=ResourceManifest(
                api_version="talon.impalasys.com/v1",
                kind="Agent",
                metadata=ResourceMeta(name=name, namespace=ns),
                spec=ResourceSpec(agent=spec),
            ),
        )
    ).resource


class SessionStreamBuffer:
    def __init__(self, *, grpc_port: int, api_key: str, namespace: str, agent: str, session_id: str):
        self.grpc_port = grpc_port
        self.api_key = api_key
        self.namespace = namespace
        self.agent = agent
        self.session_id = session_id
        self.events = []
        self._lock = threading.Lock()
        self._ready = threading.Event()
        self._stop = threading.Event()
        self._raw_channel = None
        self._channel = None
        self._thread = None
        self.error = None

    def __enter__(self):
        self._raw_channel, self._channel = conftest.authenticated_gateway_channel(
            self.grpc_port, self.api_key
        )
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()
        assert self._ready.wait(timeout=5), "session stream did not start"
        return self

    def __exit__(self, *args):
        self._stop.set()
        if self._channel is not None:
            self._channel.close()
        if self._raw_channel is not None:
            self._raw_channel.close()
        if self._thread is not None:
            self._thread.join(timeout=5)

    def _run(self):
        stub = TalonClient(self._channel)
        request = StreamSessionPartsRequest(
            ns=self.namespace,
            agent=self.agent,
            session_id=self.session_id,
        )
        self._ready.set()
        try:
            for event in stub.sessions.StreamParts(request):
                with self._lock:
                    self.events.append(event)
                if self._stop.is_set():
                    break
        except grpc.RpcError as err:
            if not self._stop.is_set():
                self.error = err

    def saw_text(self) -> bool:
        with self._lock:
            return any(event.part.content for event in self.events)


class AwsE2EStack:
    def __init__(
        self,
        *,
        temp_dir: Path,
        localstack: DockerContainer,
        env: dict[str, str],
        grpc_port: int,
        socket_path: Path,
        api_key: str,
        server_proc: subprocess.Popen,
        worker_proc: subprocess.Popen,
    ) -> None:
        self.temp_dir = temp_dir
        self.localstack = localstack
        self.env = env
        self.grpc_port = grpc_port
        self.socket_path = socket_path
        self.api_key = api_key
        self.server_proc = server_proc
        self.worker_proc = worker_proc

    @classmethod
    def start(cls) -> "AwsE2EStack":
        if shutil.which("docker") is None:
            pytest.skip("Docker is required for the AWS E2E stack")

        grpc_port = unused_tcp_port()
        suffix = uuid.uuid4().hex[:10]
        temp_dir = Path(tempfile.mkdtemp(prefix="talon-aws-e2e-"))
        socket_path = temp_dir / "worker.sock"
        table_name = f"talon_state_e2e_{suffix}"
        queue_name = f"talon-e2e-{suffix}"

        localstack = DockerContainer("localstack/localstack:3")
        localstack.with_exposed_ports(4566)
        localstack.start()
        endpoint = f"http://{localstack.get_container_host_ip()}:{localstack.get_exposed_port(4566)}"
        server_proc = None
        worker_proc = None

        try:
            cls._provision_localstack(endpoint, table_name, queue_name)
            env = cls._env(endpoint, table_name, queue_name, grpc_port, socket_path)
            config_path = temp_dir / "talon.e2e.aws.yaml"
            config_path.write_text(cls._config(grpc_port))
            env["TALON_CONFIG_PATH"] = str(config_path)

            server_proc = subprocess.Popen(
                [conftest.get_binary_path("talon_server")],
                env=env,
                stdout=sys.stdout,
                stderr=sys.stderr,
            )
            conftest.wait_for_gateway("127.0.0.1", grpc_port, attempts=40)
            time.sleep(2)

            api_key = conftest.create_e2e_api_key(grpc_port, "pytest-aws-root")
            os.environ[conftest.api_key_env_name(grpc_port)] = api_key

            worker_env = env.copy()
            worker_env["PULL_MODE"] = "1"
            worker_env["TALON_WORKER_ENDPOINT_URL"] = f"unix://{socket_path}"
            worker_proc = subprocess.Popen(
                [conftest.get_binary_path("talon_worker")],
                env=worker_env,
                stdout=sys.stdout,
                stderr=sys.stderr,
            )
            cls._wait_for_socket(socket_path)
            time.sleep(1)

            return cls(
                temp_dir=temp_dir,
                localstack=localstack,
                env=env,
                grpc_port=grpc_port,
                socket_path=socket_path,
                api_key=api_key,
                server_proc=server_proc,
                worker_proc=worker_proc,
            )
        except Exception:
            for proc in (worker_proc, server_proc):
                if proc is not None and proc.poll() is None:
                    proc.terminate()
                    try:
                        proc.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        proc.kill()
                        proc.wait(timeout=10)
            os.environ.pop(conftest.api_key_env_name(grpc_port), None)
            localstack.stop()
            shutil.rmtree(temp_dir, ignore_errors=True)
            raise

    @staticmethod
    def _env(endpoint, table_name, queue_name, grpc_port, socket_path) -> dict[str, str]:
        env = os.environ.copy()
        conftest.load_repo_dotenv_into_env(env, keys={"OPENAI_API_KEY", "CODEX_API_KEY"})
        env.update(
            {
                "AWS_ACCESS_KEY_ID": "test",
                "AWS_SECRET_ACCESS_KEY": "test",
                "AWS_DEFAULT_REGION": "us-east-1",
                "AWS_REGION": "us-east-1",
                "GRPC_ADDR": f"127.0.0.1:{grpc_port}",
                "NOVITA_API_KEY": "test-dummy-key",
                "RUST_LOG": "info",
                "TALON_DYNAMODB_ENDPOINT_URL": endpoint,
                "TALON_DYNAMODB_TABLE": table_name,
                "TALON_JWT_PRIVATE_KEY_PEM": conftest.E2E_JWT_PRIVATE_KEY_PEM,
                "TALON_JWT_ISSUER": conftest.E2E_JWT_ISSUER,
                "TALON_SQS_ENDPOINT_URL": endpoint,
                "TALON_SQS_QUEUE_NAME": queue_name,
                "TALON_WORKER_ENDPOINT_PROTOCOL": "grpc",
                "TALON_WORKER_ENDPOINT_URL": f"unix://{socket_path}",
            }
        )
        return env

    @staticmethod
    def _config(grpc_port: int) -> str:
        return f"""
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
  port: {grpc_port}
control_plane:
  database:
    driver: dynamodb
    url:
      source: env
      key: TALON_DYNAMODB_TABLE
  message_broker:
    driver: sqs
  documents:
    driver: sqlite
    data_dir: ./documents
""".strip() + "\n"

    @staticmethod
    def _provision_localstack(endpoint: str, table_name: str, queue_name: str) -> None:
        dynamodb = boto3.client(
            "dynamodb",
            endpoint_url=endpoint,
            region_name="us-east-1",
            aws_access_key_id="test",
            aws_secret_access_key="test",
        )
        sqs = boto3.client(
            "sqs",
            endpoint_url=endpoint,
            region_name="us-east-1",
            aws_access_key_id="test",
            aws_secret_access_key="test",
        )
        deadline = time.time() + 60
        while True:
            try:
                dynamodb.list_tables()
                sqs.list_queues()
                break
            except Exception:
                if time.time() > deadline:
                    raise
                time.sleep(1)

        dynamodb.create_table(
            TableName=table_name,
            BillingMode="PAY_PER_REQUEST",
            AttributeDefinitions=[
                {"AttributeName": "PK", "AttributeType": "S"},
                {"AttributeName": "SK", "AttributeType": "S"},
            ],
            KeySchema=[
                {"AttributeName": "PK", "KeyType": "HASH"},
                {"AttributeName": "SK", "KeyType": "RANGE"},
            ],
        )
        dynamodb.get_waiter("table_exists").wait(TableName=table_name)
        sqs.create_queue(QueueName=queue_name)

    @staticmethod
    def _wait_for_socket(path: Path) -> None:
        deadline = time.time() + 30
        while time.time() < deadline:
            if path.exists():
                return
            time.sleep(0.25)
        raise RuntimeError(f"Timed out waiting for worker Unix socket {path}")

    def shutdown(self) -> None:
        for proc in (self.worker_proc, self.server_proc):
            if proc.poll() is None:
                proc.terminate()
        for proc in (self.worker_proc, self.server_proc):
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=10)
        os.environ.pop(conftest.api_key_env_name(self.grpc_port), None)
        self.localstack.stop()
        shutil.rmtree(self.temp_dir, ignore_errors=True)


@pytest.fixture(scope="session")
def aws_e2e_stack(mock_llm_server):
    stack = AwsE2EStack.start()
    try:
        yield stack
    finally:
        stack.shutdown()


def test_aws_e2e_dynamodb_sqs_and_unix_worker_streaming(aws_e2e_stack):
    raw_channel, channel = conftest.authenticated_gateway_channel(
        aws_e2e_stack.grpc_port, aws_e2e_stack.api_key
    )
    try:
        stub = TalonClient(channel)
        namespace = "talon-aws-e2e"
        agent = "aws-agent"
        ensure_namespace(stub, namespace)
        create_agent_resource(
            stub,
            namespace,
            agent,
            AgentSpec(
                model_policy={
                    "profiles": [
                        {
                            "name": "default",
                            "model": Model(
                                provider="mock",
                                name="minimax/minimax-m2.7",
                                temperature=0.7,
                            ),
                        }
                    ]
                },
                system_prompt="You are a concise arithmetic assistant.",
            ),
        )
        session_id = stub.sessions.Create(
            CreateSessionRequest(agent=agent, ns=namespace)
        ).session_id

        with SessionStreamBuffer(
            grpc_port=aws_e2e_stack.grpc_port,
            api_key=aws_e2e_stack.api_key,
            namespace=namespace,
            agent=agent,
            session_id=session_id,
        ) as stream:
            stub.sessions.SendMessage(
                SendMessageRequest(
                    agent=agent,
                    session_id=session_id,
                    ns=namespace,
                    message="What is the square root of 144?",
                )
            )
            deadline = time.time() + 45
            final_messages = []
            while time.time() < deadline:
                response = stub.sessions.Get(
                    GetSessionRequest(agent=agent, session_id=session_id, ns=namespace)
                )
                final_messages = response.messages
                assistant = last_assistant_message(final_messages)
                if response.state == "IDLE" and assistant is not None:
                    break
                time.sleep(1)
            else:
                raise AssertionError("AWS E2E session did not finish")

            assistant = last_assistant_message(final_messages)
            assert assistant is not None
            assert "12" in message_text(assistant)
            assert stream.saw_text(), "Unix worker stream did not deliver any text events"
            assert stream.error is None
    finally:
        channel.close()
        raw_channel.close()
