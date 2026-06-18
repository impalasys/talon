import json
import os
import shutil
import sys
import time
import urllib.error
import urllib.request
import uuid

from e2e.cli_harness import TalonCli
from e2e import scenarios as e2e


BASE_URL = os.environ.get("TALON_CLOUDFLARE_URL", "http://127.0.0.1:8787").rstrip("/")
SIGHTLINE_ORIGIN = os.environ.get(
    "TALON_CLOUDFLARE_CORS_ORIGIN", "https://sightline.impala.systems"
)
HEALTH_TIMEOUT_SECONDS = int(
    os.environ.get("TALON_CLOUDFLARE_HEALTH_TIMEOUT_SECONDS", "1200")
)


def request(method, path, *, headers=None):
    req = urllib.request.Request(
        f"{BASE_URL}{path}",
        method=method,
        headers=headers or {},
    )
    with urllib.request.urlopen(req, timeout=15) as response:
        headers = {key.lower(): value for key, value in response.headers.items()}
        return response.status, headers, response.read().decode()


def wait_for_health():
    deadline = time.time() + HEALTH_TIMEOUT_SECONDS
    last_error = None
    while time.time() < deadline:
        try:
            status, _, body = request("GET", "/healthz")
            if status == 200 and json.loads(body).get("ok"):
                return
            last_error = f"HTTP {status}: {body}"
        except (OSError, urllib.error.URLError, ValueError) as err:
            last_error = str(err)
        time.sleep(2)
    raise RuntimeError(f"Timed out waiting for Cloudflare Talon health: {last_error}")


def check_cors_preflight():
    status, headers, body = request(
        "OPTIONS",
        "/talon.gateway.GatewayService/ListNamespaces",
        headers={
            "Origin": SIGHTLINE_ORIGIN,
            "Access-Control-Request-Method": "POST",
            "Access-Control-Request-Headers": "authorization,content-type,connect-protocol-version,x-grpc-web",
        },
    )
    if status != 204:
        raise RuntimeError(f"CORS preflight failed: HTTP {status}: {body}")
    allow_origin = headers.get("access-control-allow-origin")
    if allow_origin != SIGHTLINE_ORIGIN:
        raise RuntimeError(f"CORS preflight returned unexpected origin {allow_origin!r}")
    allow_methods = headers.get("access-control-allow-methods", "").lower()
    if "post" not in allow_methods:
        raise RuntimeError(f"CORS preflight did not allow POST: {allow_methods!r}")
    allow_headers = headers.get("access-control-allow-headers", "").lower()
    for header in ("authorization", "content-type", "connect-protocol-version", "x-grpc-web"):
        if header not in allow_headers:
            raise RuntimeError(f"CORS preflight did not allow {header!r}: {allow_headers!r}")
    expose_headers = headers.get("access-control-expose-headers", "").lower()
    for header in ("grpc-status", "grpc-message"):
        if header not in expose_headers:
            raise RuntimeError(f"CORS response did not expose {header!r}: {expose_headers!r}")


def main():
    wait_for_health()
    check_cors_preflight()

    cli_binary = os.environ.get("TALON_CLI") or shutil.which("talon-cli")
    if not cli_binary:
        raise RuntimeError("talon-cli not found; set TALON_CLI or add it to PATH")

    cli = TalonCli(cli_binary, BASE_URL, rest=True, timeout=180)
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-cf-{run_id}"
    agent = "cf-agent"

    e2e.apply(
        cli,
        e2e.MANIFEST_ROOT / "chat" / "agent.yaml",
        {
            "namespace": namespace,
            "agent": agent,
            "system_prompt": "You are a Cloudflare E2E test assistant.",
        },
        timeout=180,
    )
    session_id = e2e.session_create(cli, namespace, agent)
    e2e.session_send(
        cli,
        namespace,
        agent,
        session_id,
        "What is the square root of 144?",
        timeout=180,
    )
    completed = e2e.wait_for_session_text(
        cli,
        namespace,
        agent,
        session_id,
        "12",
        attempts=60,
        delay=2,
    )
    print(json.dumps({"ok": True, "namespace": namespace, "sessionId": session_id}))
    assert completed["state"] == "IDLE"


if __name__ == "__main__":
    try:
        main()
    except Exception as err:
        print(f"Cloudflare Talon E2E failed: {err}", file=sys.stderr)
        raise
