import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterator

import boto3
import grpc
import requests
from testcontainers.core.container import DockerContainer
from testcontainers.postgres import PostgresContainer
from talon_client.auth import ApiKeyTokenSource
from talon_client.proto.talon.v1 import auth_pb2, auth_pb2_grpc

from e2e.cli_harness import TalonCli

try:
    from python.runfiles import runfiles
except ModuleNotFoundError:
    try:
        import runfiles
    except ModuleNotFoundError:
        runfiles = None


SESSION_DISPATCH_TOPIC = "talon.session.dispatch"
RESOURCE_LIFECYCLE_TOPIC = "talon.resource.lifecycle"
WORKFLOW_DISPATCH_TOPIC = "talon.workflow.dispatch"
INDEX_EVENTS_TOPIC = "talon.index.events"
REPO_ROOT = Path(__file__).resolve().parents[2]
MOCK_LLM_PORT = int(os.environ.get("MOCK_LLM_PORT", "8000"))
AWS_ACCOUNT_ID = os.environ.get("AWS_ACCOUNT_ID", "000000000000")
E2E_JWT_ISSUER = os.environ.get(
    "TALON_E2E_JWT_ISSUER",
    "https://talon-e2e.example.com",
)
E2E_JWT_PRIVATE_KEY_PEM = os.environ.get("TALON_E2E_JWT_PRIVATE_KEY_PEM") or (
    REPO_ROOT / "src/control/security/test_rsa_private_key.pem"
).read_text()


class _ClientCallDetails(
    tuple,
    grpc.ClientCallDetails,
):
    __slots__ = ()

    def __new__(
        cls,
        method: str,
        timeout: float | None,
        metadata: list[tuple[str, str]],
        credentials: grpc.CallCredentials | None,
        wait_for_ready: bool | None,
        compression: grpc.Compression | None,
    ) -> "_ClientCallDetails":
        return tuple.__new__(
            cls,
            (method, timeout, metadata, credentials, wait_for_ready, compression),
        )

    method = property(lambda self: self[0])
    timeout = property(lambda self: self[1])
    metadata = property(lambda self: self[2])
    credentials = property(lambda self: self[3])
    wait_for_ready = property(lambda self: self[4])
    compression = property(lambda self: self[5])


class ApiKeyAuthInterceptor(
    grpc.UnaryUnaryClientInterceptor,
    grpc.UnaryStreamClientInterceptor,
    grpc.StreamUnaryClientInterceptor,
    grpc.StreamStreamClientInterceptor,
):
    def __init__(self, token_source: ApiKeyTokenSource) -> None:
        self._token_source = token_source

    def _details(self, client_call_details: grpc.ClientCallDetails) -> _ClientCallDetails:
        metadata = list(client_call_details.metadata or [])
        metadata.append(("authorization", f"Bearer {self._token_source.token()}"))
        return _ClientCallDetails(
            client_call_details.method,
            client_call_details.timeout,
            metadata,
            client_call_details.credentials,
            client_call_details.wait_for_ready,
            client_call_details.compression,
        )

    def intercept_unary_unary(
        self,
        continuation: Any,
        client_call_details: grpc.ClientCallDetails,
        request: Any,
    ) -> Any:
        return continuation(self._details(client_call_details), request)

    def intercept_unary_stream(
        self,
        continuation: Any,
        client_call_details: grpc.ClientCallDetails,
        request: Any,
    ) -> Any:
        return continuation(self._details(client_call_details), request)

    def intercept_stream_unary(
        self,
        continuation: Any,
        client_call_details: grpc.ClientCallDetails,
        request_iterator: Iterator[Any],
    ) -> Any:
        return continuation(self._details(client_call_details), request_iterator)

    def intercept_stream_stream(
        self,
        continuation: Any,
        client_call_details: grpc.ClientCallDetails,
        request_iterator: Iterator[Any],
    ) -> Any:
        return continuation(self._details(client_call_details), request_iterator)


def binary_candidates(name: str) -> Iterator[str]:
    yield name
    if "_" in name:
        yield name.replace("_", "-")
    if "-" in name:
        yield name.replace("-", "_")


def load_repo_dotenv_values() -> dict[str, str]:
    dotenv_path = REPO_ROOT / ".env"
    values = {}
    if not dotenv_path.exists():
        return values
    for line in dotenv_path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip().strip('"').strip("'")
        if key:
            values[key] = value
    return values


def load_repo_dotenv_into_env(
    env: dict[str, str],
    keys: set[str] | None = None,
) -> dict[str, str]:
    values = load_repo_dotenv_values()
    for key, value in values.items():
        if keys is not None and key not in keys:
            continue
        if value and key not in env:
            env[key] = value
    return env


def get_runfile_binary_path(name: str) -> str | None:
    if runfiles is None:
        return None

    runfiles_manifest = runfiles.Create()
    if runfiles_manifest is None:
        return None

    repo = runfiles_manifest.CurrentRepository()
    for candidate in binary_candidates(name):
        for runfile in (
            f"{repo}/talon/{candidate}" if repo else None,
            f"_main/talon/{candidate}",
            f"talon/{candidate}",
        ):
            if runfile is None:
                continue
            path = runfiles_manifest.Rlocation(runfile)
            if path and Path(path).exists():
                return str(path)

    return None


def get_binary_path(name: str) -> str:
    workspace = os.environ.get("BUILD_WORKSPACE_DIRECTORY")
    for candidate in binary_candidates(name):
        if workspace:
            path = Path(workspace) / "bazel-bin" / "talon" / candidate
            if path.exists():
                return str(path)

        runfile_path = get_runfile_binary_path(candidate)
        if runfile_path:
            return runfile_path

        for base in (REPO_ROOT / "bazel-bin" / "talon", REPO_ROOT / "target" / "debug", REPO_ROOT / "target" / "release"):
            path = base / candidate
            if path.exists():
                return str(path)

        resolved = shutil.which(candidate)
        if resolved:
            return resolved

    raise FileNotFoundError(f"Could not find binary {name}")


def wait_for_gateway(
    host: str,
    port: int,
    attempts: int = 20,
    delay_seconds: float = 1,
) -> None:
    for _ in range(attempts):
        try:
            with socket.create_connection((host, port), timeout=1):
                return
        except OSError:
            time.sleep(delay_seconds)
    raise RuntimeError(f"Talon server failed to start on port {port}")


def unused_tcp_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def write_e2e_private_key_file(grpc_port: int) -> Path:
    with tempfile.NamedTemporaryFile(
        mode="w",
        prefix=f"talon-e2e-platform-jwt-key-{grpc_port}-",
        suffix=".pem",
        delete=False,
    ) as file:
        file.write(E2E_JWT_PRIVATE_KEY_PEM)
        path = Path(file.name)
    try:
        path.chmod(0o600)
    except OSError:
        pass
    return path


def create_e2e_bootstrap_token(grpc_port: int, *, subject: str) -> str:
    cli = get_binary_path("talon_cli")
    key_file = write_e2e_private_key_file(grpc_port)
    try:
        result = subprocess.run(
            [
                cli,
                "auth",
                "local-token",
                "--private-key-pem-file",
                str(key_file),
                "--subject",
                subject,
            ],
            env={**os.environ, "TALON_JWT_ISSUER": E2E_JWT_ISSUER},
            text=True,
            capture_output=True,
            check=False,
            timeout=30,
        )
    finally:
        key_file.unlink(missing_ok=True)
    if result.returncode != 0:
        raise RuntimeError(
            "Failed to mint E2E bootstrap token\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    token = result.stdout.strip()
    if not token:
        raise RuntimeError("E2E bootstrap token command printed no token")
    return token


def create_e2e_api_key(
    grpc_port: int,
    name: str,
    *,
    subject: str = "pytest-bootstrap",
) -> str:
    cli = get_binary_path("talon_cli")
    env = os.environ.copy()
    for key in ("TALON_API_KEY", "TALON_AUTH_FILE"):
        env.pop(key, None)
    env["TALON_AUTH_FILE"] = str(
        Path(tempfile.gettempdir()) / f"talon-e2e-bootstrap-auth-{grpc_port}.json"
    )
    result = subprocess.run(
        [
            cli,
            "--gateway",
            f"http://127.0.0.1:{grpc_port}",
            "--token",
            create_e2e_bootstrap_token(grpc_port, subject=subject),
            "auth",
            "api-key",
            "create",
            "--name",
            name,
            "--grant",
            "readwrite",
        ],
        text=True,
        capture_output=True,
        check=False,
        timeout=30,
        env=env,
    )
    if result.returncode != 0:
        raise RuntimeError(
            "Failed to create E2E API key\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    for line in result.stdout.splitlines():
        if line.startswith("secret="):
            api_key = line.split("=", 1)[1].strip()
            if api_key:
                return api_key
    raise RuntimeError(f"API key creation did not print a secret:\n{result.stdout}")


def authenticated_gateway_channel(
    grpc_port: int,
    api_key: str,
) -> tuple[grpc.Channel, grpc.Channel]:
    raw_channel = grpc.insecure_channel(f"127.0.0.1:{grpc_port}")
    token_source = ApiKeyTokenSource(raw_channel, api_key)
    return raw_channel, grpc.intercept_channel(raw_channel, ApiKeyAuthInterceptor(token_source))


def write_auth_handoff(path: Path, grpc_port: int, api_key: str) -> None:
    channel = grpc.insecure_channel(f"127.0.0.1:{grpc_port}")
    try:
        exchanged = auth_pb2_grpc.AuthServiceStub(channel).ExchangeApiKey(
            auth_pb2.ExchangeApiKeyRequest(api_key=api_key),
            timeout=10,
        )
    finally:
        channel.close()

    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.with_suffix(path.suffix + ".tmp")
    tmp_path.write_text(
        "{\n"
        f'  "gatewayUrl": "http://127.0.0.1:{grpc_port}",\n'
        f'  "apiKey": "{api_key}",\n'
        f'  "accessToken": "{exchanged.access_token}",\n'
        f'  "expiresAt": {int(exchanged.expires_at)}\n'
        "}\n"
    )
    try:
        tmp_path.chmod(0o600)
    except OSError:
        pass
    tmp_path.replace(path)


def prepare_pubsub_topics(pubsub_host: str) -> None:
    for topic, subscription in [
        (SESSION_DISPATCH_TOPIC, "talon-session-dispatch-sub"),
        (RESOURCE_LIFECYCLE_TOPIC, "talon-resource-lifecycle-sub"),
        (WORKFLOW_DISPATCH_TOPIC, "talon-workflow-dispatch-sub"),
        (INDEX_EVENTS_TOPIC, "talon-index-events-sub"),
    ]:
        requests.put(
            f"http://{pubsub_host}/v1/projects/talon-local/topics/{topic}",
            timeout=5,
        )
        requests.put(
            f"http://{pubsub_host}/v1/projects/talon-local/subscriptions/{subscription}",
            json={"topic": f"projects/talon-local/topics/{topic}"},
            timeout=5,
        )


def start_talon_server_and_worker(
    env: dict[str, str],
    grpc_port: int,
    worker_pull_mode: bool = False,
) -> tuple[subprocess.Popen[Any], subprocess.Popen[Any]]:
    server_bin = get_binary_path("talon_server")
    worker_bin = get_binary_path("talon_worker")

    server_proc = subprocess.Popen(
        [server_bin],
        env=env,
        stdout=sys.stdout,
        stderr=sys.stderr,
    )
    wait_for_gateway("127.0.0.1", grpc_port)
    time.sleep(3)

    worker_env = env.copy()
    if worker_pull_mode:
        worker_env["PULL_MODE"] = "1"
    worker_env.setdefault(
        "TALON_WORKER_ENDPOINT_URL",
        f"http://127.0.0.1:{worker_env.get('PORT', '8081')}",
    )
    worker_env.setdefault("TALON_WORKER_ENDPOINT_PROTOCOL", "grpc")

    worker_proc = subprocess.Popen(
        [worker_bin],
        env=worker_env,
        stdout=sys.stdout,
        stderr=sys.stderr,
    )
    wait_for_gateway("127.0.0.1", int(worker_env.get("PORT", "8081")))
    time.sleep(1)
    return server_proc, worker_proc


def _stop_process(proc: subprocess.Popen[Any] | None) -> None:
    if proc is None:
        return
    try:
        proc.terminate()
    except ProcessLookupError:
        return
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=10)


@dataclass
class E2EStack:
    name: str
    grpc_port: int
    worker_port: int | None
    temp_dir: Path
    env: dict[str, str]
    api_key: str
    auth_file: str
    server_proc: subprocess.Popen[Any]
    worker_proc: subprocess.Popen[Any]
    metadata: dict[str, Any] = field(default_factory=dict)
    _resources: list[Any] = field(default_factory=list)

    @property
    def gateway_url(self) -> str:
        return f"http://127.0.0.1:{self.grpc_port}"

    @property
    def worker_endpoint_url(self) -> str | None:
        if "worker_endpoint_url" in self.metadata:
            return str(self.metadata["worker_endpoint_url"])
        if self.worker_port is None:
            return None
        return f"http://127.0.0.1:{self.worker_port}"

    def __getitem__(self, key: str) -> Any:
        return getattr(self, key)

    def channel(self) -> tuple[grpc.Channel, grpc.Channel]:
        return authenticated_gateway_channel(self.grpc_port, self.api_key)

    def cli(self) -> TalonCli:
        return TalonCli(
            get_binary_path("talon_cli"),
            self.gateway_url,
            api_key=self.api_key,
            env={"TALON_AUTH_FILE": self.auth_file},
        )

    def stop(self) -> None:
        _stop_process(self.worker_proc)
        _stop_process(self.server_proc)
        for resource in reversed(self._resources):
            stop = getattr(resource, "stop", None)
            if callable(stop):
                stop()
        shutil.rmtree(self.temp_dir, ignore_errors=True)


def _base_env(grpc_port: int, *, worker_port: int | None = None) -> dict[str, str]:
    env = os.environ.copy()
    load_repo_dotenv_into_env(env, keys={"OPENAI_API_KEY", "CODEX_API_KEY"})
    env["RUST_LOG"] = "info"
    env["NOVITA_API_KEY"] = "test-dummy-key"
    env["GRPC_ADDR"] = f"127.0.0.1:{grpc_port}"
    env["TALON_JWT_PRIVATE_KEY_PEM"] = E2E_JWT_PRIVATE_KEY_PEM
    env["TALON_JWT_ISSUER"] = E2E_JWT_ISSUER
    if worker_port is not None:
        env["PORT"] = str(worker_port)
        env["TALON_SESSION_PROCESSING_TIMEOUT_SECONDS"] = "1"
    return env


def start_postgres_pubsub_stack(
    *,
    grpc_port: int | None = None,
    worker_port: int = 8081,
    api_key_name: str = "pytest-postgres-root",
) -> E2EStack:
    grpc_port = grpc_port or unused_tcp_port()
    temp_dir = Path(tempfile.mkdtemp(prefix="talon-postgres-e2e-"))
    env = _base_env(grpc_port, worker_port=worker_port)

    postgres: PostgresContainer | None = None
    pubsub: DockerContainer | None = None
    server_proc: subprocess.Popen[Any] | None = None
    worker_proc: subprocess.Popen[Any] | None = None
    try:
        postgres = PostgresContainer(
            "postgres:15-alpine",
            dbname="talon",
            username="talon",
            password="password",
        )
        postgres.start()

        pubsub = DockerContainer("gcr.io/google.com/cloudsdktool/cloud-sdk:emulators")
        pubsub.with_command("gcloud beta emulators pubsub start --host-port=0.0.0.0:8085")
        pubsub.with_exposed_ports(8085)
        pubsub.start()
        time.sleep(5)

        postgres_url = postgres.get_connection_url().replace("postgresql+psycopg2://", "postgres://")
        pubsub_host = f"{pubsub.get_container_host_ip()}:{pubsub.get_exposed_port(8085)}"
        env["POSTGRES_URL"] = postgres_url
        env["PUBSUB_EMULATOR_HOST"] = pubsub_host
        env["GCP_PROJECT_ID"] = "talon-local"

        try:
            prepare_pubsub_topics(pubsub_host)
        except Exception as err:
            print(f"Warning: Failed to pre-provision pubsub: {err}")

        config_path = temp_dir / "talon.e2e.postgres.yaml"
        config_path.write_text(
            f"""
providers:
  mock:
    type: openai_compatible
    name: mock
    base_url: "http://127.0.0.1:{MOCK_LLM_PORT}"
    model: minimax/minimax-m2.7
    api_key:
      source: env
      key: NOVITA_API_KEY
server:
  host: "127.0.0.1"
  port: {grpc_port}
control_plane:
  database:
    driver: postgres
    url:
      source: env
      key: POSTGRES_URL
  message_broker:
    driver: gcp_pubsub
    project_id:
      source: env
      key: GCP_PROJECT_ID
""".strip()
            + "\n"
        )
        env["TALON_CONFIG_PATH"] = str(config_path)

        server_proc, worker_proc = start_talon_server_and_worker(
            env,
            grpc_port,
            worker_pull_mode=True,
        )
        api_key = create_e2e_api_key(grpc_port, api_key_name)
        auth_file = temp_dir / "talon-auth.json"
        return E2EStack(
            name="postgres_pubsub",
            grpc_port=grpc_port,
            worker_port=worker_port,
            temp_dir=temp_dir,
            env=env,
            api_key=api_key,
            auth_file=str(auth_file),
            server_proc=server_proc,
            worker_proc=worker_proc,
            metadata={"config_path": str(config_path)},
            _resources=[pubsub, postgres],
        )
    except Exception:
        _stop_process(worker_proc)
        _stop_process(server_proc)
        if pubsub is not None:
            pubsub.stop()
        if postgres is not None:
            postgres.stop()
        shutil.rmtree(temp_dir, ignore_errors=True)
        raise


def start_sqlite_local_stack(
    *,
    grpc_port: int | None = None,
    worker_port: int | None = None,
    api_key_name: str = "pytest-sqlite-root",
) -> E2EStack:
    grpc_port = grpc_port or unused_tcp_port()
    worker_port = worker_port or unused_tcp_port()
    temp_dir = Path(tempfile.mkdtemp(prefix="talon-sqlite-e2e-"))
    data_dir = temp_dir / "data"
    data_dir.mkdir(parents=True, exist_ok=True)
    env = _base_env(grpc_port, worker_port=worker_port)

    server_proc: subprocess.Popen[Any] | None = None
    worker_proc: subprocess.Popen[Any] | None = None
    try:
        config_path = temp_dir / "talon.e2e.sqlite.yaml"
        config_path.write_text(
            f"""
providers:
  mock:
    type: openai_compatible
    name: mock
    base_url: "http://127.0.0.1:{MOCK_LLM_PORT}"
    model: minimax/minimax-m2.7
    api_key:
      source: env
      key: NOVITA_API_KEY
server:
  host: "127.0.0.1"
  port: {grpc_port}
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

        server_proc, worker_proc = start_talon_server_and_worker(
            env,
            grpc_port,
            worker_pull_mode=True,
        )
        api_key = create_e2e_api_key(grpc_port, api_key_name)
        auth_file = temp_dir / "talon-auth.json"
        return E2EStack(
            name="sqlite_local",
            grpc_port=grpc_port,
            worker_port=worker_port,
            temp_dir=temp_dir,
            env=env,
            api_key=api_key,
            auth_file=str(auth_file),
            server_proc=server_proc,
            worker_proc=worker_proc,
            metadata={
                "config_path": str(config_path),
                "data_dir": str(data_dir),
                "restarted_workers": [],
            },
        )
    except Exception:
        _stop_process(worker_proc)
        _stop_process(server_proc)
        shutil.rmtree(temp_dir, ignore_errors=True)
        raise


def _provision_localstack(endpoint: str, table_name: str, queue_name: str) -> str:
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
    return sqs.create_queue(QueueName=queue_name)["QueueUrl"]


def _wait_for_socket(path: Path) -> None:
    deadline = time.time() + 30
    while time.time() < deadline:
        if path.exists():
            return
        time.sleep(0.25)
    raise RuntimeError(f"Timed out waiting for worker Unix socket {path}")


def start_aws_local_stack(
    *,
    grpc_port: int | None = None,
    api_key_name: str = "pytest-aws-root",
) -> E2EStack:
    grpc_port = grpc_port or unused_tcp_port()
    suffix = uuid.uuid4().hex[:10]
    temp_dir = Path(tempfile.mkdtemp(prefix="talon-aws-e2e-"))
    socket_path = temp_dir / "worker.sock"
    table_name = f"talon_state_e2e_{suffix}"
    queue_name = f"talon-e2e-{suffix}"
    env = _base_env(grpc_port)

    localstack: DockerContainer | None = None
    server_proc: subprocess.Popen[Any] | None = None
    worker_proc: subprocess.Popen[Any] | None = None
    try:
        localstack = DockerContainer("localstack/localstack:3")
        localstack.with_env("SERVICES", "dynamodb,sqs,scheduler")
        localstack.with_exposed_ports(4566)
        localstack.start()
        endpoint = (
            f"http://{localstack.get_container_host_ip()}:"
            f"{localstack.get_exposed_port(4566)}"
        )
        queue_url = _provision_localstack(endpoint, table_name, queue_name)
        scheduler_role_arn = f"arn:aws:iam::{AWS_ACCOUNT_ID}:role/talon-e2e-scheduler"
        env.update(
            {
                "AWS_ACCESS_KEY_ID": "test",
                "AWS_SECRET_ACCESS_KEY": "test",
                "AWS_DEFAULT_REGION": "us-east-1",
                "AWS_REGION": "us-east-1",
                "TALON_DYNAMODB_ENDPOINT_URL": endpoint,
                "TALON_DYNAMODB_TABLE": table_name,
                "TALON_SQS_ENDPOINT_URL": endpoint,
                "TALON_SQS_QUEUE_NAME": queue_name,
                "TALON_AWS_SCHEDULER_ENDPOINT_URL": endpoint,
                "TALON_AWS_SCHEDULER_QUEUE_URL": queue_url,
                "TALON_AWS_SCHEDULER_EXECUTION_ROLE_ARN": scheduler_role_arn,
                "TALON_AWS_SCHEDULER_NAME_PREFIX": "talon-e2e",
                "TALON_WORKER_ENDPOINT_PROTOCOL": "grpc",
                "TALON_WORKER_ENDPOINT_URL": f"unix://{socket_path}",
            }
        )

        config_path = temp_dir / "talon.e2e.aws.yaml"
        config_path.write_text(
            f"""
providers:
  mock:
    type: openai_compatible
    name: mock
    base_url: "http://127.0.0.1:{MOCK_LLM_PORT}"
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
  scheduler:
    driver: aws_event_bridge_scheduler
    group_name: default
    queue_url: "{queue_url}"
    execution_role_arn: "{scheduler_role_arn}"
    schedule_name_prefix: talon-e2e
    endpoint_url: "{endpoint}"
  documents:
    driver: sqlite
    data_dir: ./documents
""".strip()
            + "\n"
        )
        env["TALON_CONFIG_PATH"] = str(config_path)

        server_proc = subprocess.Popen(
            [get_binary_path("talon_server")],
            env=env,
            stdout=sys.stdout,
            stderr=sys.stderr,
        )
        wait_for_gateway("127.0.0.1", grpc_port, attempts=40)
        time.sleep(2)

        api_key = create_e2e_api_key(grpc_port, api_key_name)
        worker_env = env.copy()
        worker_env["PULL_MODE"] = "1"
        worker_proc = subprocess.Popen(
            [get_binary_path("talon_worker")],
            env=worker_env,
            stdout=sys.stdout,
            stderr=sys.stderr,
        )
        _wait_for_socket(socket_path)
        time.sleep(1)

        auth_file = temp_dir / "talon-auth.json"
        return E2EStack(
            name="aws_local",
            grpc_port=grpc_port,
            worker_port=None,
            temp_dir=temp_dir,
            env=env,
            api_key=api_key,
            auth_file=str(auth_file),
            server_proc=server_proc,
            worker_proc=worker_proc,
            metadata={
                "config_path": str(config_path),
                "socket_path": str(socket_path),
                "worker_endpoint_url": f"unix://{socket_path}",
                "localstack_endpoint": endpoint,
                "sqs_queue_url": queue_url,
            },
            _resources=[localstack],
        )
    except Exception:
        _stop_process(worker_proc)
        _stop_process(server_proc)
        if localstack is not None:
            localstack.stop()
        shutil.rmtree(temp_dir, ignore_errors=True)
        raise
