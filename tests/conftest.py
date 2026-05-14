import pytest
import subprocess
import time
import grpc
import requests
import sys
import os

# Important: Add generated protos to path
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))

from testcontainers.postgres import PostgresContainer
from testcontainers.core.container import DockerContainer
import shutil

SESSION_DISPATCH_TOPIC = "talon.session.dispatch"
RESOURCE_LIFECYCLE_TOPIC = "talon.resource.lifecycle"

def get_binary_path(name):
    # Ensure .exe extension on Windows, though we are mostly on MAC/Linux
    path = os.path.abspath(f"talon/{name}")
    if os.path.exists(path):
        return path
        
    raise FileNotFoundError(f"Could not find binary {name} at {path}")

@pytest.fixture(scope="session", autouse=True)
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
    env["GRPC_ADDR"] = f"127.0.0.1:{test_grpc_port}"
    env["NOVITA_API_KEY"] = "test-dummy-key"
    
    # Copy talon.yaml so load_default() can find it in the test execution root
    config_src = os.path.abspath("talon/talon.yaml")
    if os.path.exists(config_src):
        shutil.copy(config_src, "talon.yaml")
        print("Copied talon.yaml to test execution root.")
    
    server_bin = get_binary_path("talon_server")
    worker_bin = get_binary_path("talon_worker")
    
    print("\nStarting Talon server and worker...")
    
    server_proc = subprocess.Popen(
        [server_bin],
        env=env,
        stdout=sys.stdout,
        stderr=sys.stderr
    )
    
    # Wait for gateway to be properly listening before starting worker 
    # to avoid Postgres CREATE TABLE concurrent race condition.
    # We use a raw socket to avoid initializing gRPC before python forks the worker_proc, 
    # which causes gRPC C-Core to panic.
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
        
    time.sleep(3) # Hard wait for Postgres table creations to fully finalize
    
    print("\nStarting Talon worker now that server initialized DB...")
    env_worker = env.copy()
    env_worker["PULL_MODE"] = "1"
    
    worker_proc = subprocess.Popen(
        [worker_bin],
        env=env_worker, # PULL_MODE enabled
        stdout=sys.stdout,
        stderr=sys.stderr
    )
            
    yield
    
    print("\nShutting down Talon servers and containers...")
    server_proc.terminate()
    worker_proc.terminate()
    server_proc.wait()
    worker_proc.wait()
    pubsub.stop()
    postgres.stop()

@pytest.fixture(scope="session", autouse=True)
def mock_llm_server():
    """Starts the FastAPI mock LLM server before the test suite runs and tears it down after."""
    print("\nStarting mock LLM server...")
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
    
    # Wait for the server to be healthy
    import socket
    for _ in range(10):
        try:
            with socket.create_connection(("127.0.0.1", 8000), timeout=1):
                break
        except Exception:
            time.sleep(0.5)
            
    yield
    print("\nShutting down mock LLM server...")

@pytest.fixture
def test_grpc_port():
    return 50052

@pytest.fixture
def gateway_channel(test_grpc_port):
    """Returns a connected gRPC channel to the Talon gateway."""
    channel = grpc.insecure_channel(f"127.0.0.1:{test_grpc_port}")
    yield channel
    channel.close()
