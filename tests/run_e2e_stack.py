import http.server
import os
import signal
import socketserver
import sys
import threading
from pathlib import Path

# Add generated protos to path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "sdk" / "python" / "talon-client" / "src"))
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))
sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))

from e2e.stack import (
    MOCK_LLM_PORT,
    start_aws_local_stack,
    start_postgres_pubsub_stack,
    write_auth_handoff,
)


READY_PORT = int(os.environ.get("READY_PORT", os.environ.get("E2E_READY_PORT", "8090")))
GATEWAY_GRPC_PORT = int(os.environ.get("GRPC_PORT", "50051"))
E2E_STACK = os.environ.get("TALON_E2E_STACK", "gcp").strip().lower()
E2E_AUTH_FILE = Path(
    os.environ.get(
        "TALON_E2E_AUTH_FILE",
        str(Path(__file__).resolve().parents[1] / "target" / "talon-e2e-auth.json"),
    )
)


def main():
    print("Starting mock LLM server...")
    import uvicorn
    from mock_llm import app

    server_thread = threading.Thread(
        target=uvicorn.run,
        args=(app,),
        kwargs={"host": "0.0.0.0", "port": MOCK_LLM_PORT, "log_level": "info"},
        daemon=True,
    )
    server_thread.start()

    if E2E_STACK == "aws":
        stack = start_aws_local_stack(
            grpc_port=GATEWAY_GRPC_PORT,
            api_key_name="playwright-root",
        )
    elif E2E_STACK in ("gcp", "default", ""):
        stack = start_postgres_pubsub_stack(
            grpc_port=GATEWAY_GRPC_PORT,
            worker_port=8081,
            api_key_name="playwright-root",
        )
    else:
        raise RuntimeError(
            f"Unsupported TALON_E2E_STACK={E2E_STACK!r}; expected 'gcp' or 'aws'"
        )

    ready_server: socketserver.TCPServer | None = None
    try:
        write_auth_handoff(E2E_AUTH_FILE, stack.grpc_port, stack.api_key)

        class ReadyHandler(http.server.SimpleHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()
                self.wfile.write(b"READY")

        ready_server = socketserver.TCPServer(("127.0.0.1", READY_PORT), ReadyHandler)
        ready_thread = threading.Thread(target=ready_server.serve_forever, daemon=True)
        ready_thread.start()
    except BaseException:
        E2E_AUTH_FILE.unlink(missing_ok=True)
        stack.stop()
        raise

    print("--- E2E STACK READY ---")
    stopped = False

    def shutdown(*_args):
        nonlocal stopped
        if stopped:
            raise SystemExit(0)
        stopped = True
        print("Shutting down...")
        if ready_server is not None:
            ready_server.shutdown()
            ready_server.server_close()
        E2E_AUTH_FILE.unlink(missing_ok=True)
        stack.stop()
        raise SystemExit(0)

    signal.signal(signal.SIGINT, shutdown)
    signal.signal(signal.SIGTERM, shutdown)

    try:
        stack.server_proc.wait()
    finally:
        if not stopped:
            stopped = True
            if ready_server is not None:
                ready_server.shutdown()
                ready_server.server_close()
            E2E_AUTH_FILE.unlink(missing_ok=True)
            stack.stop()


if __name__ == "__main__":
    main()
