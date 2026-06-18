import pytest
import subprocess
import time
import grpc
import requests
import sys
import os
from pathlib import Path
import tempfile

try:
    from python.runfiles import runfiles
except ModuleNotFoundError:
    try:
        import runfiles
    except ModuleNotFoundError:
        runfiles = None

# Important: Add generated protos to path
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))
sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))

from testcontainers.postgres import PostgresContainer
from testcontainers.core.container import DockerContainer
import shutil

SESSION_DISPATCH_TOPIC = "talon.session.dispatch"
RESOURCE_LIFECYCLE_TOPIC = "talon.resource.lifecycle"
WORKFLOW_DISPATCH_TOPIC = "talon.workflow.dispatch"
MOCK_LLM_PORT = int(os.environ.get("MOCK_LLM_PORT", "8000"))
REPO_ROOT = Path(__file__).resolve().parents[1]

def load_repo_dotenv_values():
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

def load_repo_dotenv_into_env(env, keys=None):
    values = load_repo_dotenv_values()
    for key, value in values.items():
        if keys is not None and key not in keys:
            continue
        if value and key not in env:
            env[key] = value
    return env

def binary_candidates(name):
    yield name
    if "_" in name:
        yield name.replace("_", "-")
    if "-" in name:
        yield name.replace("-", "_")

def get_runfile_binary_path(name):
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

def get_binary_path(name):
    workspace = os.environ.get("BUILD_WORKSPACE_DIRECTORY")
    for candidate in binary_candidates(name):
        if workspace:
            path = Path(workspace) / "bazel-bin" / "talon" / candidate
            if path.exists():
                return str(path)

        runfile_path = get_runfile_binary_path(candidate)
        if runfile_path:
            return runfile_path

        for base in (REPO_ROOT / "target" / "debug", REPO_ROOT / "target" / "release"):
            path = base / candidate
            if path.exists():
                return str(path)

        resolved = shutil.which(candidate)
        if resolved:
            return resolved

    raise FileNotFoundError(f"Could not find binary {name}")


def wait_for_gateway(host, port, attempts=20, delay_seconds=1):
    import socket

    for _ in range(attempts):
        try:
            with socket.create_connection((host, port), timeout=1):
                return
        except Exception:
            time.sleep(delay_seconds)
    raise RuntimeError(f"Talon server failed to start on port {port}")


def start_talon_server_and_worker(env, grpc_port, worker_pull_mode=False):
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

    worker_proc = subprocess.Popen(
        [worker_bin],
        env=worker_env,
        stdout=sys.stdout,
        stderr=sys.stderr,
    )
    wait_for_gateway("127.0.0.1", int(worker_env.get("PORT", "8081")))
    time.sleep(1)

    return server_proc, worker_proc

@pytest.fixture(scope="session")
def talon_infrastructure():
    print("\nStarting Postgres container...")
    postgres = PostgresContainer("postgres:15-alpine", dbname="talon", username="talon", password="password")
    postgres.start()
    
    print("\nStarting PubSub emulator container...")
    pubsub = DockerContainer("gcr.io/google.com/cloudsdktool/cloud-sdk:emulators")
    pubsub.with_command("gcloud beta emulators pubsub start --host-port=0.0.0.0:8085")
    pubsub.with_exposed_ports(8085)
    pubsub.start()
    
    # Wait for pubsub to be ready
    time.sleep(5)
    
    # Testcontainers returns postgresql+psycopg2:// format, which sqlx doesn't like. 
    # Use postgres:// instead.
    postgres_url = postgres.get_connection_url().replace("postgresql+psycopg2://", "postgres://")
    pubsub_host = f"{pubsub.get_container_host_ip()}:{pubsub.get_exposed_port(8085)}"
    
    print(f"\nPostgres URL: {postgres_url}")
    print(f"PubSub Host: {pubsub_host}")
    
    env = os.environ.copy()
    load_repo_dotenv_into_env(env, keys={"OPENAI_API_KEY", "CODEX_API_KEY"})
    env["POSTGRES_URL"] = postgres_url
    env["PUBSUB_EMULATOR_HOST"] = pubsub_host
    env["RUST_LOG"] = "info"
    env["GCP_PROJECT_ID"] = "talon-local"
    
    # Pre-provision PubSub topics and subscriptions to avoid races
    try:
        print("\nPre-provisioning PubSub topics...")
        for topic, subscription in [
            (SESSION_DISPATCH_TOPIC, "talon-session-dispatch-sub"),
            (RESOURCE_LIFECYCLE_TOPIC, "talon-resource-lifecycle-sub"),
            (WORKFLOW_DISPATCH_TOPIC, "talon-workflow-dispatch-sub"),
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
    except Exception as e:
        print(f"Warning: Failed to pre-provision pubsub: {e}")
    
    # Use an isolated port to guarantee we don't accidentally talk to a host docker-compose talon_server 
    test_grpc_port = 50052
    test_ui_port = 50053
    env["GRPC_ADDR"] = f"127.0.0.1:{test_grpc_port}"
    env["GATEWAY_UI_ADDR"] = f"127.0.0.1:{test_ui_port}"
    env["NOVITA_API_KEY"] = "test-dummy-key"
    
    temp_dir = Path(tempfile.mkdtemp(prefix="talon-postgres-e2e-"))
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
  port: {test_ui_port}
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

    print("\nStarting Talon server and worker...")
    server_proc, worker_proc = start_talon_server_and_worker(
        env,
        test_grpc_port,
        worker_pull_mode=True,
    )
            
    yield
    
    print("\nShutting down Talon servers and containers...")
    server_proc.terminate()
    worker_proc.terminate()
    server_proc.wait()
    worker_proc.wait()
    pubsub.stop()
    postgres.stop()
    shutil.rmtree(temp_dir, ignore_errors=True)

@pytest.fixture(scope="session", autouse=True)
def mock_llm_server():
    """Starts the FastAPI mock LLM server before the test suite runs and tears it down after."""
    print("\nStarting mock LLM server...")
    import socket

    try:
        with socket.create_connection(("127.0.0.1", 8000), timeout=1):
            print("\nReusing existing mock LLM server on 127.0.0.1:8000")
            yield
            return
    except Exception:
        pass

    import threading
    import uvicorn
    from mock_llm import app
    
    server_thread = threading.Thread(
        target=uvicorn.run,
        args=(app,),
        kwargs={"host": "0.0.0.0", "port": MOCK_LLM_PORT, "log_level": "info"},
        daemon=True
    )
    server_thread.start()
    
    # Wait for the server to be healthy
    for _ in range(10):
        try:
            with socket.create_connection(("127.0.0.1", MOCK_LLM_PORT), timeout=1):
                break
        except Exception:
            time.sleep(0.5)
            
    yield
    print("\nShutting down mock LLM server...")

@pytest.fixture
def test_grpc_port():
    return 50052

@pytest.fixture
def gateway_channel(talon_infrastructure, test_grpc_port):
    """Returns a connected gRPC channel to the Talon gateway."""
    channel = grpc.insecure_channel(f"127.0.0.1:{test_grpc_port}")
    yield channel
    channel.close()


@pytest.fixture(scope="session")
def talon_infrastructure_sqlite():
    print("\nStarting SQLite + local_socket Talon stack...")
    test_grpc_port = 50054
    test_ui_port = 50055
    worker_port = 18082

    env = os.environ.copy()
    load_repo_dotenv_into_env(env, keys={"OPENAI_API_KEY", "CODEX_API_KEY"})
    env["RUST_LOG"] = "info"
    env["NOVITA_API_KEY"] = "test-dummy-key"
    env["GRPC_ADDR"] = f"127.0.0.1:{test_grpc_port}"
    env["GATEWAY_UI_ADDR"] = f"127.0.0.1:{test_ui_port}"
    env["PORT"] = str(worker_port)

    temp_dir = Path(tempfile.mkdtemp(prefix="talon-sqlite-e2e-"))
    data_dir = temp_dir / "data"
    data_dir.mkdir(parents=True, exist_ok=True)
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
  port: {test_ui_port}
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
        test_grpc_port,
        worker_pull_mode=True,
    )

    yield {
        "grpc_port": test_grpc_port,
        "ui_port": test_ui_port,
        "worker_port": worker_port,
        "config_path": str(config_path),
        "data_dir": str(data_dir),
    }

    print("\nShutting down SQLite + local_socket Talon stack...")
    server_proc.terminate()
    worker_proc.terminate()
    server_proc.wait()
    worker_proc.wait()
    shutil.rmtree(temp_dir, ignore_errors=True)


@pytest.fixture
def sqlite_test_grpc_port(talon_infrastructure_sqlite):
    return talon_infrastructure_sqlite["grpc_port"]


@pytest.fixture
def gateway_channel_sqlite(talon_infrastructure_sqlite, sqlite_test_grpc_port):
    channel = grpc.insecure_channel(f"127.0.0.1:{sqlite_test_grpc_port}")
    yield channel
    channel.close()
