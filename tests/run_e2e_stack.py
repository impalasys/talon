import time
import sys
import os
import subprocess
import signal
from pathlib import Path
import socket
import tempfile
import json

# Add generated protos to path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "sdk" / "python" / "talon-client" / "src"))
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))
sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))

from testcontainers.postgres import PostgresContainer
from testcontainers.core.container import DockerContainer
import requests
import shutil
import grpc
from talon_client.proto.talon.v1 import auth_pb2, auth_pb2_grpc

SESSION_DISPATCH_TOPIC = "talon.session.dispatch"
RESOURCE_LIFECYCLE_TOPIC = "talon.resource.lifecycle"
WORKFLOW_DISPATCH_TOPIC = "talon.workflow.dispatch"
REPO_ROOT = Path(__file__).resolve().parents[1]
GATEWAY_GRPC_PORT = int(os.environ.get("GRPC_PORT", "50051"))
MOCK_LLM_PORT = int(os.environ.get("MOCK_LLM_PORT", "8000"))
READY_PORT = int(os.environ.get("READY_PORT", os.environ.get("E2E_READY_PORT", "8090")))
E2E_PLATFORM_JWT_ISSUER = os.environ.get(
    "TALON_E2E_PLATFORM_JWT_ISSUER",
    "https://talon-e2e.example.com",
)
E2E_JWT_PRIVATE_KEY_PEM = os.environ.get(
    "TALON_E2E_JWT_PRIVATE_KEY_PEM",
    (REPO_ROOT / "src/control/security/test_rsa_private_key.pem").read_text(),
)
E2E_AUTH_FILE = Path(
    os.environ.get(
        "TALON_E2E_AUTH_FILE",
        str(REPO_ROOT / "target" / "talon-e2e-auth.json"),
    )
)

def binary_candidates(name):
    yield name
    if "_" in name:
        yield name.replace("_", "-")
    if "-" in name:
        yield name.replace("-", "_")

def get_binary_path(name):
    workspace = os.environ.get("BUILD_WORKSPACE_DIRECTORY")
    for candidate in binary_candidates(name):
        if workspace:
            path = os.path.join(workspace, "bazel-bin", "talon", candidate)
            if os.path.exists(path):
                return path

        path = REPO_ROOT / "bazel-bin" / "talon" / candidate
        if path.exists():
            return str(path)

        path = REPO_ROOT / "target" / "debug" / candidate
        if path.exists():
            return str(path)

        path = REPO_ROOT / "target" / "release" / candidate
        if path.exists():
            return str(path)

        resolved = shutil.which(candidate)
        if resolved:
            return resolved

    return name

def wait_for_port(host, port, timeout_seconds=30):
    deadline = time.time() + timeout_seconds
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=1):
                return
        except OSError:
            time.sleep(1)
    raise RuntimeError(f"Timed out waiting for {host}:{port}")

def stop_container(container_name):
    subprocess.run(["docker", "rm", "-f", container_name], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def cleanup_temp_dir(temp_dir: Path):
    if temp_dir.exists():
        shutil.rmtree(temp_dir, ignore_errors=True)

def write_e2e_private_key_file(grpc_port):
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

def create_bootstrap_token(grpc_port):
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
                "playwright-bootstrap",
            ],
            env={**os.environ, "TALON_PLATFORM_JWT_ISSUER": E2E_PLATFORM_JWT_ISSUER},
            text=True,
            capture_output=True,
            check=False,
            timeout=30,
        )
    finally:
        key_file.unlink(missing_ok=True)
    if result.returncode != 0:
        raise RuntimeError(
            "Failed to mint Playwright bootstrap token\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    token = result.stdout.strip()
    if not token:
        raise RuntimeError("Bootstrap token command printed no token")
    return token

def create_api_key(grpc_port):
    cli = get_binary_path("talon_cli")
    env = os.environ.copy()
    for key in (
        "TALON_API_KEY",
        "TALON_AUTH_FILE",
    ):
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
            create_bootstrap_token(grpc_port),
            "auth",
            "api-key",
            "create",
            "--name",
            "playwright-root",
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
            "Failed to create Playwright API key\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    for line in result.stdout.splitlines():
        if line.startswith("secret="):
            secret = line.split("=", 1)[1].strip()
            if secret:
                return secret
    raise RuntimeError(f"API key creation did not print a secret:\n{result.stdout}")

def write_auth_handoff(grpc_port, api_key):
    channel = grpc.insecure_channel(f"127.0.0.1:{grpc_port}")
    try:
        exchanged = auth_pb2_grpc.AuthServiceStub(channel).ExchangeApiKey(
            auth_pb2.ExchangeApiKeyRequest(api_key=api_key),
            timeout=10,
        )
    finally:
        channel.close()

    E2E_AUTH_FILE.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = E2E_AUTH_FILE.with_suffix(E2E_AUTH_FILE.suffix + ".tmp")
    tmp_path.write_text(
        json.dumps(
            {
                "gatewayUrl": f"http://127.0.0.1:{grpc_port}",
                "apiKey": api_key,
                "accessToken": exchanged.access_token,
                "expiresAt": int(exchanged.expires_at),
            },
            indent=2,
        )
        + "\n"
    )
    try:
        tmp_path.chmod(0o600)
    except OSError:
        pass
    tmp_path.replace(E2E_AUTH_FILE)

def main():
    print("Starting mock LLM server...")
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

    print("Starting Postgres container...")
    postgres = PostgresContainer("postgres:15-alpine", dbname="talon", username="talon", password="password")
    postgres.start()
    
    print("Starting PubSub emulator container...")
    pubsub = DockerContainer("gcr.io/google.com/cloudsdktool/cloud-sdk:emulators")
    pubsub.with_command("gcloud beta emulators pubsub start --host-port=0.0.0.0:8085")
    pubsub.with_exposed_ports(8085)
    pubsub.start()
    
    time.sleep(5)
    
    postgres_url = postgres.get_connection_url().replace("postgresql+psycopg2://", "postgres://")
    pubsub_host = f"{pubsub.get_container_host_ip()}:{pubsub.get_exposed_port(8085)}"
    
    env = os.environ.copy()
    env["POSTGRES_URL"] = postgres_url
    env["PUBSUB_EMULATOR_HOST"] = pubsub_host
    env["RUST_LOG"] = "info"
    env["GCP_PROJECT_ID"] = "talon-local"
    env["NOVITA_API_KEY"] = "test-dummy-key"
    env["TALON_JWT_PRIVATE_KEY_PEM"] = E2E_JWT_PRIVATE_KEY_PEM
    env["TALON_PLATFORM_JWT_ISSUER"] = E2E_PLATFORM_JWT_ISSUER
    temp_dir = Path(tempfile.mkdtemp(prefix="talon-e2e-"))
    
    env["GRPC_ADDR"] = f"0.0.0.0:{GATEWAY_GRPC_PORT}"
    
    try:
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

    config_path = temp_dir / "talon.e2e.yaml"
    with open(config_path, "w") as f:
        f.write(f"""
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
  host: "0.0.0.0"
  port: {GATEWAY_GRPC_PORT}
control_plane:
  database:
    driver: postgres
    url:
      source: env
      key: POSTGRES_URL
  message_broker:
    driver: gcp_pubsub
""")
    env["TALON_CONFIG_PATH"] = str(config_path)

    server_bin = get_binary_path("talon_server")
    worker_bin = get_binary_path("talon_worker")

    print("Starting Talon server and worker...")
    server_proc = subprocess.Popen([server_bin], env=env, stdout=sys.stdout, stderr=sys.stderr)

    try:
        wait_for_port("127.0.0.1", GATEWAY_GRPC_PORT)
    except Exception:
        server_proc.terminate()
        postgres.stop()
        pubsub.stop()
        cleanup_temp_dir(temp_dir)
        raise RuntimeError(f"Talon server failed to start on port {GATEWAY_GRPC_PORT}")

    try:
        time.sleep(3)
        api_key = create_api_key(GATEWAY_GRPC_PORT)
        write_auth_handoff(GATEWAY_GRPC_PORT, api_key)
    except Exception:
        server_proc.terminate()
        postgres.stop()
        pubsub.stop()
        E2E_AUTH_FILE.unlink(missing_ok=True)
        cleanup_temp_dir(temp_dir)
        raise

    env_worker = env.copy()
    env_worker["PULL_MODE"] = "1"
    env_worker["TALON_WORKER_ENDPOINT_URL"] = "http://127.0.0.1:8081"
    env_worker["TALON_WORKER_ENDPOINT_PROTOCOL"] = "grpc"
    worker_proc = subprocess.Popen([worker_bin], env=env_worker, stdout=sys.stdout, stderr=sys.stderr)

    print("--- E2E STACK READY ---")

    # Start a dummy HTTP server to signal readiness to Playwright
    import http.server
    import socketserver
    
    class ReadyHandler(http.server.SimpleHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"READY")

    ready_server = socketserver.TCPServer(("127.0.0.1", READY_PORT), ReadyHandler)
    ready_thread = threading.Thread(target=ready_server.serve_forever, daemon=True)
    ready_thread.start()

    def signal_handler(sig, frame):
        print("Shutting down...")
        server_proc.terminate()
        worker_proc.terminate()
        ready_server.shutdown()
        ready_server.server_close()
        postgres.stop()
        pubsub.stop()
        E2E_AUTH_FILE.unlink(missing_ok=True)
        cleanup_temp_dir(temp_dir)
        sys.exit(0)

    signal.signal(signal.SIGINT, signal_handler)
    signal.signal(signal.SIGTERM, signal_handler)

    try:
        server_proc.wait()
    finally:
        ready_server.shutdown()
        ready_server.server_close()
        postgres.stop()
        pubsub.stop()
        E2E_AUTH_FILE.unlink(missing_ok=True)
        cleanup_temp_dir(temp_dir)

if __name__ == '__main__':
    main()
