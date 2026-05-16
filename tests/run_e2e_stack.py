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
REPO_ROOT = Path(__file__).resolve().parents[1]
ENVOY_PORT = 18789
GATEWAY_GRPC_PORT = 50051
GATEWAY_UI_PORT = 50052

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

def write_envoy_artifacts(temp_dir: Path):
    descriptor_path = temp_dir / "talon_gateway_proto-descriptor-set.e2e.bin"
    subprocess.run(
        [
            "protoc",
            "-I.",
            "-Iproto",
            "-Ithird_party/googleapis",
            "--include_imports",
            "--include_source_info",
            "--experimental_allow_proto3_optional",
            f"--descriptor_set_out={descriptor_path}",
            "proto/gateway.proto",
        ],
        check=True,
        cwd=REPO_ROOT,
    )

    envoy_config_path = temp_dir / "envoy.e2e.yaml"
    envoy_config_path.write_text(
        f"""static_resources:
  listeners:
  - name: listener_0
    address:
      socket_address: {{ address: 0.0.0.0, port_value: 8081 }}
    filter_chains:
    - filters:
      - name: envoy.filters.network.http_connection_manager
        typed_config:
          "@type": type.googleapis.com/envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager
          stat_prefix: ingress_http
          codec_type: AUTO
          route_config:
            name: local_route
            virtual_hosts:
            - name: local_service
              domains: ["*"]
              routes:
              - match: {{ prefix: "/v1/ui/" }}
                route:
                  cluster: talon_ui_http_service
                  timeout: 0s
              - match: {{ prefix: "/" }}
                route:
                  cluster: talon_grpc_service
                  timeout: 60s
              cors:
                allow_origin_string_match:
                - safe_regex:
                    google_re2: {{}}
                    regex: ".*"
                allow_methods: GET, PUT, DELETE, POST, OPTIONS
                allow_headers: keep-alive,user-agent,cache-control,content-type,content-transfer-encoding,x-accept-content-transfer-encoding,x-accept-response-streaming,x-user-agent,x-grpc-web,grpc-timeout,authorization
                max_age: "1728000"
                expose_headers: grpc-status,grpc-message
          http_filters:
          - name: envoy.filters.http.grpc_web
            typed_config:
              "@type": type.googleapis.com/envoy.extensions.filters.http.grpc_web.v3.GrpcWeb
          - name: envoy.filters.http.cors
            typed_config:
              "@type": type.googleapis.com/envoy.extensions.filters.http.cors.v3.Cors
          - name: envoy.filters.http.grpc_json_transcoder
            typed_config:
              "@type": type.googleapis.com/envoy.extensions.filters.http.grpc_json_transcoder.v3.GrpcJsonTranscoder
              proto_descriptor: "/etc/envoy/talon_gateway_proto-descriptor-set.proto.bin"
              services: ["talon.gateway.GatewayService"]
              max_response_body_size: 33554432
              print_options:
                add_whitespace: true
                always_print_primitive_fields: true
                always_print_enums_as_ints: false
                preserve_proto_field_names: false
          - name: envoy.filters.http.router
            typed_config:
              "@type": type.googleapis.com/envoy.extensions.filters.http.router.v3.Router
  clusters:
  - name: talon_grpc_service
    connect_timeout: 0.25s
    type: LOGICAL_DNS
    typed_extension_protocol_options:
      envoy.extensions.upstreams.http.v3.HttpProtocolOptions:
        "@type": type.googleapis.com/envoy.extensions.upstreams.http.v3.HttpProtocolOptions
        explicit_http_config:
          http2_protocol_options: {{}}
    lb_policy: ROUND_ROBIN
    load_assignment:
      cluster_name: talon_grpc_service
      endpoints:
      - lb_endpoints:
        - endpoint:
            address:
              socket_address:
                address: host.docker.internal
                port_value: {GATEWAY_GRPC_PORT}
  - name: talon_ui_http_service
    connect_timeout: 0.25s
    type: LOGICAL_DNS
    lb_policy: ROUND_ROBIN
    load_assignment:
      cluster_name: talon_ui_http_service
      endpoints:
      - lb_endpoints:
        - endpoint:
            address:
              socket_address:
                address: host.docker.internal
                port_value: {GATEWAY_UI_PORT}
"""
    )
    return descriptor_path, envoy_config_path

def stop_container(container_name):
    subprocess.run(["docker", "rm", "-f", container_name], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def stop_stale_envoy_containers():
    result = subprocess.run(
        ["docker", "ps", "-aq", "--filter", "name=talon-e2e-envoy-"],
        check=False,
        capture_output=True,
        text=True,
    )
    for container_id in result.stdout.split():
        stop_container(container_id)

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
    temp_dir = Path(tempfile.mkdtemp(prefix="talon-e2e-"))
    
    env["GRPC_ADDR"] = f"0.0.0.0:{GATEWAY_GRPC_PORT}"
    env["GATEWAY_UI_ADDR"] = f"0.0.0.0:{GATEWAY_UI_PORT}"
    
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

    config_path = temp_dir / "talon.e2e.yaml"
    with open(config_path, "w") as f:
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
    env["TALON_CONFIG_PATH"] = str(config_path)

    server_bin = get_binary_path("talon_server")
    worker_bin = get_binary_path("talon_worker")
    descriptor_path, envoy_config_path = write_envoy_artifacts(temp_dir)
    envoy_container_name = f"talon-e2e-envoy-{os.getpid()}"

    print("Starting Talon server and worker...")
    server_proc = subprocess.Popen([server_bin], env=env, stdout=sys.stdout, stderr=sys.stderr)

    try:
        wait_for_port("127.0.0.1", GATEWAY_GRPC_PORT)
        wait_for_port("127.0.0.1", GATEWAY_UI_PORT)
    except Exception:
        server_proc.terminate()
        postgres.stop()
        pubsub.stop()
        cleanup_temp_dir(temp_dir)
        raise RuntimeError(f"Talon server failed to start on ports {GATEWAY_GRPC_PORT}/{GATEWAY_UI_PORT}")

    time.sleep(3)
    env_worker = env.copy()
    env_worker["PULL_MODE"] = "1"
    worker_proc = subprocess.Popen([worker_bin], env=env_worker, stdout=sys.stdout, stderr=sys.stderr)

    stop_stale_envoy_containers()
    stop_container(envoy_container_name)
    subprocess.run(
        [
            "docker",
            "run",
            "--rm",
            "--detach",
            "--name",
            envoy_container_name,
            "--add-host",
            "host.docker.internal:host-gateway",
            "--publish",
            f"{ENVOY_PORT}:8081",
            "--volume",
            f"{envoy_config_path}:/etc/envoy/envoy.yaml:ro",
            "--volume",
            f"{descriptor_path}:/etc/envoy/talon_gateway_proto-descriptor-set.proto.bin:ro",
            "envoyproxy/envoy:v1.33-latest",
            "-c",
            "/etc/envoy/envoy.yaml",
        ],
        check=True,
        cwd=REPO_ROOT,
    )
    wait_for_port("127.0.0.1", ENVOY_PORT)

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
        stop_container(envoy_container_name)
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
        stop_container(envoy_container_name)
        postgres.stop()
        pubsub.stop()
        cleanup_temp_dir(temp_dir)

if __name__ == '__main__':
    main()
