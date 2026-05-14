import time
import sys
import os
import subprocess
import signal

# Add generated protos to path
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))

from testcontainers.postgres import PostgresContainer
from testcontainers.core.container import DockerContainer
import requests
import shutil

SESSION_DISPATCH_TOPIC = "talon.session.dispatch"
RESOURCE_LIFECYCLE_TOPIC = "talon.resource.lifecycle"

def get_binary_path(name):
    workspace = os.environ.get("BUILD_WORKSPACE_DIRECTORY")
    if workspace:
        path = os.path.join(workspace, "bazel-bin", "talon", name)
        if os.path.exists(path):
            return path
    
    path = os.path.abspath(f"../../bazel-bin/talon/{name}")
    if os.path.exists(path):
        return path
    return name

def main():
    print("Starting mock LLM server...")
    import threading
    import uvicorn
    from talon.tests.mock_llm import app
    
    server_thread = threading.Thread(
        target=uvicorn.run,
        args=(app,),
        kwargs={"host": "0.0.0.0", "port": 8000, "log_level": "info"},
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
    
    # Use a specific port for E2E testing
    test_grpc_port = 18789
    env["GRPC_ADDR"] = f"0.0.0.0:{test_grpc_port}"
    
    try:
        for topic, subscription in [
            (SESSION_DISPATCH_TOPIC, "talon-session-dispatch-sub"),
            (RESOURCE_LIFECYCLE_TOPIC, "talon-resource-lifecycle-sub"),
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

    with open("talon.yaml", "w") as f:
        f.write("""
providers:
  mock:
    type: openai_compatible
    name: mock
    base_url: "http://127.0.0.1:8000"
    model: minimax/minimax-m2.7
    api_key:
      source: env
      key: NOVITA_API_KEY
server:
  host: "0.0.0.0"
  port: 18789
control_plane:
  database:
    driver: postgres
    url:
      source: env
      key: POSTGRES_URL
  message_broker:
    driver: gcp_pubsub
""")
    env["TALON_CONFIG_PATH"] = os.path.abspath("talon.yaml")

    server_bin = get_binary_path("talon_server")
    worker_bin = get_binary_path("talon_worker")
    
    print("Starting Talon server and worker...")
    server_proc = subprocess.Popen([server_bin], env=env, stdout=sys.stdout, stderr=sys.stderr)
    
    import socket
    gateway_ready = False
    for _ in range(20):
        try:
            with socket.create_connection(("127.0.0.1", test_grpc_port), timeout=1):
                gateway_ready = True
                break
        except Exception:
            time.sleep(1)
            
    if not gateway_ready:
        server_proc.terminate()
        postgres.stop()
        pubsub.stop()
        raise RuntimeError(f"Talon server failed to start on port {test_grpc_port}")

    time.sleep(3)
    env_worker = env.copy()
    env_worker["PULL_MODE"] = "1"
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
        postgres.stop()
        pubsub.stop()
        sys.exit(0)

    signal.signal(signal.SIGINT, signal_handler)
    signal.signal(signal.SIGTERM, signal_handler)

    server_proc.wait()

if __name__ == '__main__':
    main()
