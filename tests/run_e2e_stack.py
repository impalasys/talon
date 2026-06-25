import time
import sys
import os
import subprocess
import signal
from pathlib import Path
import socket
import tempfile

# Add generated protos to path
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))
sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))

from testcontainers.postgres import PostgresContainer
from testcontainers.core.container import DockerContainer
import requests
import shutil

SESSION_DISPATCH_TOPIC = "talon.session.dispatch"
RESOURCE_LIFECYCLE_TOPIC = "talon.resource.lifecycle"
WORKFLOW_DISPATCH_TOPIC = "talon.workflow.dispatch"
REPO_ROOT = Path(__file__).resolve().parents[1]
GATEWAY_GRPC_PORT = int(os.environ.get("GRPC_PORT", "50051"))
MOCK_LLM_PORT = int(os.environ.get("MOCK_LLM_PORT", "8000"))

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

    time.sleep(3)
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

    ready_server = socketserver.TCPServer(("127.0.0.1", 8090), ReadyHandler)
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
        cleanup_temp_dir(temp_dir)

if __name__ == '__main__':
    main()
