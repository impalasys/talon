import os
import time
import uuid

import requests


BASE_URL = os.environ.get("TALON_CLOUDFLARE_URL", "http://127.0.0.1:8787").rstrip("/")
SIGHTLINE_ORIGIN = os.environ.get("TALON_CLOUDFLARE_CORS_ORIGIN", "https://sightline.impala.systems")
API_REQUEST_TIMEOUT_SECONDS = int(os.environ.get("TALON_CLOUDFLARE_API_TIMEOUT_SECONDS", "180"))
HEALTH_TIMEOUT_SECONDS = int(os.environ.get("TALON_CLOUDFLARE_HEALTH_TIMEOUT_SECONDS", "1200"))


def wait_for_health():
    deadline = time.time() + HEALTH_TIMEOUT_SECONDS
    last_error = None
    while time.time() < deadline:
        try:
            response = requests.get(f"{BASE_URL}/healthz", timeout=5)
            if response.status_code == 200 and response.json().get("ok"):
                return
            last_error = f"HTTP {response.status_code}: {response.text}"
        except (requests.RequestException, ValueError) as err:
            last_error = str(err)
        time.sleep(2)
    raise RuntimeError(f"Timed out waiting for Cloudflare Talon health: {last_error}")


def request_json(method, path, **kwargs):
    response = requests.request(method, f"{BASE_URL}{path}", timeout=API_REQUEST_TIMEOUT_SECONDS, **kwargs)
    if response.status_code >= 400:
        raise RuntimeError(f"{method} {path} failed: HTTP {response.status_code}: {response.text}")
    if response.text.strip():
        return response.json()
    return {}


def check_cors_preflight():
    response = requests.options(
        f"{BASE_URL}/talon.gateway.GatewayService/ListNamespaces",
        headers={
            "Origin": SIGHTLINE_ORIGIN,
            "Access-Control-Request-Method": "POST",
            "Access-Control-Request-Headers": "authorization,content-type,connect-protocol-version,x-grpc-web",
        },
        timeout=15,
    )
    if response.status_code != 204:
        raise RuntimeError(f"CORS preflight failed: HTTP {response.status_code}: {response.text}")
    allow_origin = response.headers.get("access-control-allow-origin")
    if allow_origin != SIGHTLINE_ORIGIN:
        raise RuntimeError(f"CORS preflight returned unexpected origin {allow_origin!r}")
    allow_methods = response.headers.get("access-control-allow-methods", "").lower()
    if "post" not in allow_methods:
        raise RuntimeError(f"CORS preflight did not allow POST: {allow_methods!r}")
    allow_headers = response.headers.get("access-control-allow-headers", "").lower()
    for header in ("authorization", "content-type", "connect-protocol-version", "x-grpc-web"):
        if header not in allow_headers:
            raise RuntimeError(f"CORS preflight did not allow {header!r}: {allow_headers!r}")
    expose_headers = response.headers.get("access-control-expose-headers", "").lower()
    for header in ("grpc-status", "grpc-message"):
        if header not in expose_headers:
            raise RuntimeError(f"CORS response did not expose {header!r}: {expose_headers!r}")


def message_text(message):
    return "".join(
        part.get("content", "")
        for part in message.get("parts", [])
        if part.get("partType") in (1, "PART_TYPE_TEXT")
    )


def main():
    wait_for_health()
    check_cors_preflight()
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-cf-{run_id}"
    agent = "cf-agent"

    request_json(
        "POST",
        f"/v1/namespaces/{namespace}",
        json={"name": namespace, "recursive": True},
    )
    request_json(
        "POST",
        f"/v1/ns/{namespace}/agents",
        json={
            "ns": namespace,
            "name": agent,
            "definition": {
                "customSpec": {
                    "modelPolicy": {
                        "profiles": [
                            {
                                "name": "default",
                                "model": {
                                    "provider": "mock",
                                    "name": "minimax",
                                    "temperature": 0.7,
                                },
                            }
                        ]
                    },
                    "systemPrompt": "You are a Cloudflare E2E test assistant.",
                }
            },
        },
    )
    session = request_json(
        "POST",
        f"/v1/ns/{namespace}/agents/{agent}/sessions",
        json={"ns": namespace, "agent": agent},
    )
    session_id = session.get("sessionId") or session.get("session_id")
    if not session_id:
        raise RuntimeError(f"CreateSession did not return a session id: {session}")

    request_json(
        "POST",
        f"/v1/ns/{namespace}/agents/{agent}/sessions/{session_id}/message",
        json={
            "ns": namespace,
            "agent": agent,
            "sessionId": session_id,
            "message": "What is the square root of 144?",
        },
    )

    deadline = time.time() + 60
    last = None
    while time.time() < deadline:
        last = request_json(
            "GET",
            f"/v1/ns/{namespace}/agents/{agent}/sessions/{session_id}",
        )
        messages = last.get("messages", [])
        if last.get("state") == "IDLE" and len(messages) >= 2:
            if "12" not in message_text(messages[-1]):
                raise RuntimeError(f"Assistant reply did not contain expected answer: {last}")
            print("Cloudflare Talon E2E passed")
            return
        time.sleep(2)
    raise RuntimeError(f"Timed out waiting for assistant reply: {last}")


if __name__ == "__main__":
    main()
