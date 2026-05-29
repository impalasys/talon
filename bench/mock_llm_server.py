#!/usr/bin/env python3
"""Small OpenAI-compatible mock LLM for Talon load benchmarks.

This intentionally uses only the Python standard library so the benchmark can
run it in a stock python container without installing dependencies.
"""

from __future__ import annotations

import argparse
import asyncio
import contextlib
import json
import socket
import time
from typing import Any
from urllib.parse import urlparse
from uuid import uuid4


class Counters:
    def __init__(self) -> None:
        self._lock = asyncio.Lock()
        self.requests = 0
        self.streaming_requests = 0
        self.non_streaming_requests = 0
        self.in_flight = 0
        self.max_in_flight = 0
        self.started_at = time.time()

    async def begin(self, streaming: bool) -> None:
        async with self._lock:
            self.requests += 1
            if streaming:
                self.streaming_requests += 1
            else:
                self.non_streaming_requests += 1
            self.in_flight += 1
            self.max_in_flight = max(self.max_in_flight, self.in_flight)

    async def end(self) -> None:
        async with self._lock:
            self.in_flight = max(0, self.in_flight - 1)

    async def snapshot(self) -> dict[str, Any]:
        async with self._lock:
            return {
                "requests": self.requests,
                "streaming_requests": self.streaming_requests,
                "non_streaming_requests": self.non_streaming_requests,
                "in_flight": self.in_flight,
                "max_in_flight": self.max_in_flight,
                "uptime_seconds": time.time() - self.started_at,
            }


def reply_tokens(data: dict[str, Any], response_tokens: int) -> list[str]:
    messages = data.get("messages", [])
    agent = "agent"
    if messages:
        content = messages[-1].get("content", "")
        if isinstance(content, str):
            agent = content.rsplit(" ", 1)[-1] or "agent"
    tokens = ["benchmark", "response", "for", agent]
    while len(tokens) < response_tokens:
        tokens.append(f"tok{len(tokens):04d}")
    return tokens[:response_tokens]


def usage(response_tokens: int) -> dict[str, int]:
    return {
        "prompt_tokens": 10,
        "completion_tokens": response_tokens,
        "reasoning_tokens": 0,
        "total_tokens": 10 + response_tokens,
    }


def json_response(status: int, payload: dict[str, Any]) -> bytes:
    body = json.dumps(payload).encode("utf-8")
    reason = {
        200: "OK",
        400: "Bad Request",
        404: "Not Found",
        405: "Method Not Allowed",
    }.get(status, "OK")
    return (
        f"HTTP/1.1 {status} {reason}\r\n"
        "content-type: application/json\r\n"
        f"content-length: {len(body)}\r\n"
        "connection: close\r\n"
        "\r\n"
    ).encode("utf-8") + body


async def write_json(
    writer: asyncio.StreamWriter, status: int, payload: dict[str, Any]
) -> None:
    writer.write(json_response(status, payload))
    await writer.drain()


async def write_sse(writer: asyncio.StreamWriter, payload: dict[str, Any] | str) -> None:
    data = payload if isinstance(payload, str) else json.dumps(payload)
    writer.write(f"data: {data}\n\n".encode("utf-8"))
    await writer.drain()


async def read_http_request(
    reader: asyncio.StreamReader,
) -> tuple[str, str, dict[str, str], bytes] | None:
    try:
        header_bytes = await reader.readuntil(b"\r\n\r\n")
    except (asyncio.IncompleteReadError, asyncio.LimitOverrunError):
        return None

    header_text = header_bytes.decode("iso-8859-1")
    lines = header_text.split("\r\n")
    if not lines or not lines[0]:
        return None

    parts = lines[0].split()
    if len(parts) < 2:
        return None
    method, target = parts[0], parts[1]

    headers: dict[str, str] = {}
    for line in lines[1:]:
        if not line or ":" not in line:
            continue
        name, value = line.split(":", 1)
        headers[name.strip().lower()] = value.strip()

    try:
        content_length = int(headers.get("content-length", "0") or "0")
    except ValueError:
        return None
    body = b""
    if content_length > 0:
        try:
            body = await reader.readexactly(content_length)
        except asyncio.IncompleteReadError:
            return None

    return method, target, headers, body


async def handle_connection(
    reader: asyncio.StreamReader,
    writer: asyncio.StreamWriter,
    *,
    latency_ms: int,
    tokens_per_second: float,
    response_tokens: int,
    counters: Counters,
) -> None:
    latency_seconds = latency_ms / 1000.0
    token_interval_seconds = 1.0 / tokens_per_second if tokens_per_second > 0 else 0.0

    try:
        request = await read_http_request(reader)
        if request is None:
            return
        method, target, _headers, body = request
        path = urlparse(target).path

        if method == "GET":
            if path == "/health":
                await write_json(
                    writer,
                    200,
                    {
                        "ok": True,
                        "latency_ms": latency_ms,
                        "tokens_per_second": tokens_per_second,
                        "response_tokens": response_tokens,
                    },
                )
                return
            if path == "/metrics":
                payload = await counters.snapshot()
                payload["latency_ms"] = latency_ms
                payload["tokens_per_second"] = tokens_per_second
                payload["response_tokens"] = response_tokens
                await write_json(writer, 200, payload)
                return
            await write_json(writer, 404, {"error": "not found"})
            return

        if method != "POST":
            await write_json(writer, 405, {"error": "method not allowed"})
            return

        if path != "/chat/completions":
            await write_json(writer, 404, {"error": "not found"})
            return

        try:
            data = json.loads(body or b"{}")
        except json.JSONDecodeError as exc:
            await write_json(writer, 400, {"error": f"invalid json: {exc}"})
            return

        streaming = bool(data.get("stream"))
        await counters.begin(streaming)
        try:
            if streaming:
                await stream_completion(
                    writer,
                    data,
                    latency_seconds,
                    token_interval_seconds,
                    response_tokens,
                )
            else:
                await json_completion(
                    writer,
                    data,
                    latency_seconds,
                    token_interval_seconds,
                    response_tokens,
                )
        finally:
            await counters.end()
    except (ConnectionError, asyncio.IncompleteReadError, BrokenPipeError):
        return
    finally:
        writer.close()
        with contextlib.suppress(Exception):
            await writer.wait_closed()


async def json_completion(
    writer: asyncio.StreamWriter,
    data: dict[str, Any],
    latency_seconds: float,
    token_interval_seconds: float,
    response_tokens: int,
) -> None:
    if latency_seconds:
        await asyncio.sleep(latency_seconds)
    if token_interval_seconds:
        await asyncio.sleep(token_interval_seconds * response_tokens)
    model = data.get("model", "talon-bench-mock")
    await write_json(
        writer,
        200,
        {
            "id": f"chatcmpl-{uuid4().hex[:8]}",
            "object": "chat.completion",
            "created": int(time.time()),
            "model": model,
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": " ".join(reply_tokens(data, response_tokens)),
                    },
                    "finish_reason": "stop",
                }
            ],
            "usage": usage(response_tokens),
        },
    )


async def stream_completion(
    writer: asyncio.StreamWriter,
    data: dict[str, Any],
    latency_seconds: float,
    token_interval_seconds: float,
    response_tokens: int,
) -> None:
    model = data.get("model", "talon-bench-mock")
    completion_id = f"chatcmpl-{uuid4().hex[:8]}"
    writer.write(
        b"HTTP/1.1 200 OK\r\n"
        b"content-type: text/event-stream\r\n"
        b"cache-control: no-cache\r\n"
        b"connection: close\r\n"
        b"\r\n"
    )
    await writer.drain()

    if latency_seconds:
        await asyncio.sleep(latency_seconds)

    tokens = reply_tokens(data, response_tokens)
    for index, token in enumerate(tokens):
        chunk = token if index + 1 == len(tokens) else token + " "
        await write_sse(
            writer,
            {
                "id": completion_id,
                "object": "chat.completion.chunk",
                "created": int(time.time()),
                "model": model,
                "choices": [
                    {
                        "index": 0,
                        "delta": {"content": chunk},
                        "finish_reason": None,
                    }
                ],
            },
        )
        if token_interval_seconds:
            await asyncio.sleep(token_interval_seconds)
    await write_sse(
        writer,
        {
            "id": completion_id,
            "object": "chat.completion.chunk",
            "created": int(time.time()),
            "model": model,
            "usage": usage(response_tokens),
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
        },
    )
    await write_sse(writer, "[DONE]")


async def amain() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=8000)
    parser.add_argument("--latency-ms", type=int, default=0)
    parser.add_argument("--tokens-per-second", type=float, default=10.0)
    parser.add_argument("--response-tokens", type=int, default=50)
    parser.add_argument("--request-backlog", type=int, default=4096)
    args = parser.parse_args()

    counters = Counters()
    server = await asyncio.start_server(
        lambda reader, writer: handle_connection(
            reader,
            writer,
            latency_ms=args.latency_ms,
            tokens_per_second=args.tokens_per_second,
            response_tokens=args.response_tokens,
            counters=counters,
        ),
        args.host,
        args.port,
        backlog=args.request_backlog,
        reuse_address=True,
        start_serving=True,
    )

    for sock in server.sockets or []:
        with contextlib.suppress(OSError):
            sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)

    print(
        "mock LLM listening on "
        f"{args.host}:{args.port} latency_ms={args.latency_ms} "
        f"tokens_per_second={args.tokens_per_second} "
        f"response_tokens={args.response_tokens} "
        f"request_backlog={args.request_backlog} "
        "server=asyncio",
        flush=True,
    )
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    asyncio.run(amain())
