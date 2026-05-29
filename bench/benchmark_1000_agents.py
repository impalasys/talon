#!/usr/bin/env python3
"""Run the Talon 1000-agent benchmark with Docker Compose.

Example:
    python bench/benchmark_1000_agents.py --agents 1000 --latencies 0,50,250 --memory 512m
"""

from __future__ import annotations

import argparse
import asyncio
import contextlib
import dataclasses
import json
import math
import os
import random
import re
import socket
import statistics
import subprocess
import sys
import time
from pathlib import Path
from typing import Any
from urllib.parse import quote, urlencode
from urllib.request import urlopen

try:
    import grpc
except ImportError as exc:
    raise SystemExit(
        "Missing benchmark dependencies. Install them with "
        "`python -m pip install grpcio`, or run from .venv-e2e."
    ) from exc


REPO_ROOT = Path(__file__).resolve().parents[1]
TESTS_DIR = REPO_ROOT / "tests"
GENERATED_DIR = TESTS_DIR / "generated"
sys.path.insert(0, str(GENERATED_DIR))
sys.path.insert(0, str(TESTS_DIR))

from proto.gateway_pb2 import (  # noqa: E402
    CreateAgentRequest,
    CreateNamespaceRequest,
    CreateSessionRequest,
    SendMessageRequest,
    StreamSessionPartsBatchRequest,
    StreamSessionPartsRequest,
)
from proto.gateway_pb2_grpc import GatewayServiceStub  # noqa: E402
from proto.events_pb2 import (  # noqa: E402
    SESSION_MESSAGE_PART_EVENT_KIND_DONE,
    SESSION_MESSAGE_PART_EVENT_KIND_ERROR,
)
from proto.manifests_pb2 import AgentDefinition, AgentSpec, Model  # noqa: E402
from proto.models_pb2 import SESSION_MESSAGE_PART_TYPE_TEXT  # noqa: E402


MOCK_LLM_PORT = 8000
TALON_GRPC_PORT = 50051
TALON_UI_PORT = 50052
POSTGRES_PORT = 5432
PROJECT_NAME_RE = re.compile(r"^talon-[a-z0-9]{6}$")


@dataclasses.dataclass
class RunTimings:
    stream_opened: float | None = None
    send_started: float | None = None
    send_finished: float | None = None
    first_part: float | None = None
    first_token: float | None = None
    completed: float | None = None
    errored: float | None = None
    error: str | None = None


def parse_latencies(value: str) -> list[int]:
    return [int(part.strip()) for part in value.split(",") if part.strip()]


def percentile(values: list[float], pct: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, round((pct / 100.0) * (len(ordered) - 1))))
    return ordered[index]


def summarize_seconds(values: list[float]) -> dict[str, float | None]:
    if not values:
        return {"min": None, "p50": None, "p90": None, "p95": None, "p99": None, "max": None}
    return {
        "min": min(values),
        "p50": percentile(values, 50),
        "p90": percentile(values, 90),
        "p95": percentile(values, 95),
        "p99": percentile(values, 99),
        "max": max(values),
    }


def summarize_numbers(values: list[int | float]) -> dict[str, int | float | None]:
    if not values:
        return {"count": 0, "min": None, "p50": None, "p90": None, "p95": None, "p99": None, "max": None}
    return {
        "count": len(values),
        "min": min(values),
        "p50": percentile([float(value) for value in values], 50),
        "p90": percentile([float(value) for value in values], 90),
        "p95": percentile([float(value) for value in values], 95),
        "p99": percentile([float(value) for value in values], 99),
        "max": max(values),
    }


def mib(value: int | float | None) -> str:
    if not isinstance(value, (int, float)):
        return "n/a"
    return f"{value / (1024 * 1024):.1f}MiB"


def wait_for_port(host: str, port: int, timeout_seconds: float = 60.0) -> None:
    deadline = time.monotonic() + timeout_seconds
    last_error: OSError | None = None
    while time.monotonic() < deadline:
        try:
            with socket.create_connection((host, port), timeout=1):
                return
        except OSError as exc:
            last_error = exc
            time.sleep(0.5)
    raise TimeoutError(f"timed out waiting for {host}:{port}: {last_error}")


async def wait_for_file(path: Path, timeout_seconds: float) -> bool:
    deadline = time.perf_counter() + timeout_seconds
    while time.perf_counter() < deadline:
        if path.exists() and path.stat().st_size > 0:
            return True
        await asyncio.sleep(0.5)
    return path.exists() and path.stat().st_size > 0


def write_talon_config(path: Path, database: str) -> None:
    if database == "sqlite":
        database_config = """
    driver: sqlite
    data_dir: /data/talon/bench
""".rstrip()
    elif database == "postgres":
        database_config = """
    driver: postgres
    url:
      source: env
      key: POSTGRES_URL
""".rstrip()
    else:
        raise ValueError(f"unsupported database backend: {database}")

    path.write_text(
        f"""
providers:
  mock:
    type: openai_compatible
    base_url: "http://mock-llm:8000"
    model: talon-bench-mock
    api_key:
      source: env
      key: NOVITA_API_KEY
server:
  host: "0.0.0.0"
  port: 50052
control_plane:
  database:
{database_config}
  message_broker:
    driver: local_socket
""".strip()
        + "\n",
        encoding="utf-8",
    )


def talon_command() -> str:
    return """
set -eu
talon-server &
server_pid=$!
until curl -sS --max-time 1 -o /dev/null http://127.0.0.1:50052/; do
  if ! kill -0 "$server_pid" 2>/dev/null; then
    wait "$server_pid"
  fi
  sleep 0.2
done
PULL_MODE=1 talon-worker &
worker_pid=$!
trap 'kill "$server_pid" "$worker_pid" 2>/dev/null || true' TERM INT
while kill -0 "$server_pid" 2>/dev/null && kill -0 "$worker_pid" 2>/dev/null; do
  sleep 1
done
kill "$server_pid" "$worker_pid" 2>/dev/null || true
wait "$server_pid" "$worker_pid"
""".strip()


def build_runtime_image(
    tag: str,
    no_cache: bool,
    dockerfile: str,
    cargo_features: str | None = None,
) -> None:
    env = os.environ.copy()
    if Path(dockerfile).name == "oss-runtime.Dockerfile":
        env["DOCKER_BUILDKIT"] = "1"
    cmd = [
        "docker",
        "build",
        "-f",
        dockerfile,
        "-t",
        tag,
    ]
    if cargo_features:
        cmd.extend(["--build-arg", f"CARGO_FEATURES={cargo_features}"])
    if no_cache:
        cmd.append("--no-cache")
    cmd.append(".")
    subprocess.run(cmd, cwd=REPO_ROOT, env=env, check=True)


def generate_project_name() -> str:
    alphabet = "abcdefghijklmnopqrstuvwxyz0123456789"
    return "talon-" + "".join(random.choice(alphabet) for _ in range(6))


def validate_project_name(project_name: str) -> None:
    if not PROJECT_NAME_RE.match(project_name):
        raise ValueError(
            "--project-name must match talon-[6 lowercase alphanumeric chars], "
            f"got {project_name!r}"
        )


def json_quote(value: str | Path) -> str:
    return json.dumps(str(value))


def indent_block(value: str, spaces: int) -> str:
    prefix = " " * spaces
    return "\n".join(prefix + line for line in value.splitlines())


def write_compose_file(
    path: Path,
    project_name: str,
    image: str,
    config_path: Path,
    data_dir: Path,
    memory: str,
    worker_concurrency: int,
    local_socket_buffer_size: int,
    latency_ms: int,
    tokens_per_second: float,
    response_tokens: int,
    mock_request_backlog: int,
    mock_cpus: float | None,
    mock_memory: str | None,
    database: str,
    sqlite_pool_size: int,
    sqlite_busy_timeout_ms: int,
    postgres_cpus: float | None,
    postgres_memory: str | None,
    postgres_max_connections: int,
    postgres_server_max_connections: int,
    otel: bool,
    cpu_profile: bool,
    cpu_profile_path: Path,
    cpu_profile_seconds: int,
    cpu_profile_delay_seconds: int,
    cpu_profile_frequency_hz: int,
    heap_profile: bool,
    heap_profile_dir: Path,
    heap_profile_delay_seconds: int,
    heap_profile_label: str,
    ) -> None:
    rust_log = os.environ.get("RUST_LOG", "warn,talon=info")
    if otel and "talon::control::kv" not in rust_log:
        rust_log = f"{rust_log},talon::control::kv=debug"
    mock_script = (REPO_ROOT / "bench" / "mock_llm_server.py").resolve()
    mock_volume = f"{mock_script}:/bench/mock_llm_server.py:ro"
    config_volume = f"{config_path.resolve()}:/data/talon/talon.bench.yaml:ro"
    data_volume = f"{data_dir.resolve()}:/data/talon/bench:rw"
    command = talon_command().replace("$", "$$")
    jaeger_service = ""
    if otel:
        jaeger_service = f"""
  jaeger:
    image: jaegertracing/all-in-one:latest
    container_name: {project_name}-jaeger
    environment:
      COLLECTOR_OTLP_ENABLED: "true"
    ports:
      - "16686"
      - "4317"

"""
    otel_env = ""
    if otel:
        otel_env = """
      TALON_OTEL_ENABLED: "true"
      OTEL_SERVICE_NAME: talon-worker
      OTEL_EXPORTER_OTLP_ENDPOINT: http://jaeger:4317
      OTEL_BSP_SCHEDULE_DELAY: "1000"
      TALON_OTEL_SAMPLE_RATIO: "1.0"
"""
    cpu_profile_env = ""
    if cpu_profile:
        cpu_profile_env = f"""
      TALON_CPU_PROFILE_ENABLED: "true"
      TALON_CPU_PROFILE_PATH: {json_quote(cpu_profile_path)}
      TALON_CPU_PROFILE_SECONDS: "{cpu_profile_seconds}"
      TALON_CPU_PROFILE_DELAY_SECONDS: "{cpu_profile_delay_seconds}"
      TALON_CPU_PROFILE_FREQUENCY_HZ: "{cpu_profile_frequency_hz}"
"""
    heap_profile_env = ""
    if heap_profile:
        heap_profile_env = f"""
      MALLOC_CONF: "prof:true,prof_active:true,lg_prof_sample:17,prof_leak:false"
      _RJEM_MALLOC_CONF: "prof:true,prof_active:true,lg_prof_sample:17,prof_leak:false"
      TALON_HEAP_PROFILE_ENABLED: "true"
      TALON_HEAP_PROFILE_DIR: {json_quote(heap_profile_dir)}
      TALON_HEAP_PROFILE_DELAY_SECONDS: "{heap_profile_delay_seconds}"
      TALON_HEAP_PROFILE_LABEL: {json_quote(heap_profile_label)}
"""
    mock_resource_limits = ""
    if mock_cpus is not None:
        mock_resource_limits += f"    cpus: {mock_cpus}\n"
    if mock_memory:
        mock_resource_limits += f"    mem_limit: {mock_memory}\n    memswap_limit: {mock_memory}\n"
    postgres_service = ""
    postgres_env = ""
    postgres_resource_limits = ""
    if database == "postgres":
        postgres_env = """
      POSTGRES_URL: postgres://talon:talon@postgres:5432/talon
"""
        if postgres_cpus is not None:
            postgres_resource_limits += f"    cpus: {postgres_cpus}\n"
        if postgres_memory:
            postgres_resource_limits += (
                f"    mem_limit: {postgres_memory}\n    memswap_limit: {postgres_memory}\n"
            )
        postgres_data_dir = data_dir / "postgres"
        postgres_data_dir.mkdir(exist_ok=True)
        postgres_volume = f"{postgres_data_dir.resolve()}:/var/lib/postgresql/data:rw"
        postgres_service = f"""
  postgres:
    image: postgres:16-alpine
    container_name: {project_name}-postgres
    environment:
      POSTGRES_USER: talon
      POSTGRES_PASSWORD: talon
      POSTGRES_DB: talon
{postgres_resource_limits.rstrip()}
    command:
      - postgres
      - -c
      - max_connections={postgres_server_max_connections}
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U talon -d talon"]
      interval: 1s
      timeout: 5s
      retries: 30
    volumes:
      - {json_quote(postgres_volume)}
    ports:
      - "{POSTGRES_PORT}"

"""
    if database == "postgres":
        depends_on = """      mock-llm:
        condition: service_started
      postgres:
        condition: service_healthy"""
    else:
        depends_on = """      mock-llm:
        condition: service_started"""
    if otel:
        depends_on += """
      jaeger:
        condition: service_started"""
    path.write_text(
        f"""
name: {project_name}
services:
{jaeger_service}
{postgres_service}
  mock-llm:
    image: python:3.12-slim
    container_name: {project_name}-mock-llm
    command:
      - python
      - /bench/mock_llm_server.py
      - --host
      - 0.0.0.0
      - --port
      - "{MOCK_LLM_PORT}"
      - --latency-ms
      - "{latency_ms}"
      - --tokens-per-second
      - "{tokens_per_second}"
      - --response-tokens
      - "{response_tokens}"
      - --request-backlog
      - "{mock_request_backlog}"
{mock_resource_limits.rstrip()}
    volumes:
      - {json_quote(mock_volume)}
    ports:
      - "{MOCK_LLM_PORT}"

  talon:
    image: {image}
    container_name: {project_name}-talon
    depends_on:
{depends_on}
    cpus: 1.0
    mem_limit: {memory}
    memswap_limit: {memory}
    environment:
      TALON_CONFIG_PATH: /data/talon/talon.bench.yaml
      RUST_LOG: {json_quote(rust_log)}
      NOVITA_API_KEY: bench-dummy-key
      GRPC_ADDR: 0.0.0.0:{TALON_GRPC_PORT}
      GATEWAY_UI_ADDR: 0.0.0.0:{TALON_UI_PORT}
      PORT: "8081"
      TALON_TOKEN_BATCH_MS: "10000"
      TALON_WORKER_SESSION_CONCURRENCY: "{worker_concurrency}"
      TALON_LOCAL_SOCKET_SUBSCRIBER_BUFFER_SIZE: "{local_socket_buffer_size}"
      TALON_SQLITE_MAX_CONNECTIONS: "{sqlite_pool_size}"
      TALON_SQLITE_BUSY_TIMEOUT_MS: "{sqlite_busy_timeout_ms}"
      TALON_POSTGRES_MAX_CONNECTIONS: "{postgres_max_connections}"
{postgres_env.rstrip()}
{otel_env.rstrip()}
{cpu_profile_env.rstrip()}
{heap_profile_env.rstrip()}
    command:
      - /bin/sh
      - -lc
      - |
{indent_block(command, 8)}
    volumes:
      - {json_quote(config_volume)}
      - {json_quote(data_volume)}
    ports:
      - "{TALON_GRPC_PORT}"
      - "{TALON_UI_PORT}"
""".lstrip(),
        encoding="utf-8",
    )


async def wait_for_channel(address: str, timeout_seconds: float = 60.0) -> None:
    async with grpc.aio.insecure_channel(address) as channel:
        await asyncio.wait_for(channel.channel_ready(), timeout=timeout_seconds)


def agent_definition() -> AgentDefinition:
    return AgentDefinition(
        custom_spec=AgentSpec(
            model_policy={
                "profiles": [
                    {
                        "name": "default",
                        "model": Model(
                            provider="mock",
                            name="talon-bench-mock",
                            temperature=0.0,
                        ),
                    }
                ]
            },
            system_prompt="You are a deterministic benchmark assistant.",
        )
    )


async def provision(stub: GatewayServiceStub, ns: str, agents: int, concurrency: int) -> list[str]:
    await stub.CreateNamespace(CreateNamespaceRequest(name=ns, recursive=True))
    definition = agent_definition()
    sem = asyncio.Semaphore(concurrency)

    async def create_agent(index: int) -> str:
        name = f"agent-{index:04d}"
        async with sem:
            await stub.CreateAgent(
                CreateAgentRequest(ns=ns, name=name, definition=definition)
            )
            return name

    agent_names = await asyncio.gather(*(create_agent(i) for i in range(agents)))

    async def create_session(agent: str) -> tuple[str, str]:
        async with sem:
            response = await stub.CreateSession(CreateSessionRequest(ns=ns, agent=agent))
            return agent, response.session_id

    sessions = await asyncio.gather(*(create_session(agent) for agent in agent_names))
    return [session_id for _, session_id in sorted(sessions)]


async def consume_stream(
    stub: GatewayServiceStub,
    ns: str,
    agent: str,
    session_id: str,
    timings: RunTimings,
    stream_ready: asyncio.Event,
) -> None:
    request = StreamSessionPartsRequest(ns=ns, agent=agent, session_id=session_id)
    try:
        stream = stub.StreamSessionParts(request)
        timings.stream_opened = time.perf_counter()
        stream_ready.set()
        async for event in stream:
            now = time.perf_counter()
            if timings.first_part is None:
                timings.first_part = now
            part = event.part
            if part.part_type == SESSION_MESSAGE_PART_TYPE_TEXT and timings.first_token is None:
                timings.first_token = now
            if event.kind == SESSION_MESSAGE_PART_EVENT_KIND_DONE:
                timings.completed = now
                stream.cancel()
                return
            if event.kind == SESSION_MESSAGE_PART_EVENT_KIND_ERROR:
                timings.errored = now
                timings.error = part.content or "stream emitted error part"
                stream.cancel()
                return
    except asyncio.CancelledError:
        raise
    except Exception as exc:
        timings.errored = time.perf_counter()
        timings.error = repr(exc)
        stream_ready.set()


def canonical_session_name(ns: str, agent: str, session_id: str) -> str:
    return (
        f"@Namespace/{ns}/Agent/{quote(agent, safe='')}/"
        f"@/Session/{quote(session_id, safe='')}"
    )


async def consume_batch_stream(
    stub: GatewayServiceStub,
    ns: str,
    agent_names: list[str],
    session_ids: list[str],
    timings: list[RunTimings],
    stream_ready: asyncio.Event,
) -> None:
    session_names = [
        canonical_session_name(ns, agent, session_id)
        for agent, session_id in zip(agent_names, session_ids, strict=True)
    ]
    by_session = {
        (ns, agent, session_id): index
        for index, (agent, session_id) in enumerate(
            zip(agent_names, session_ids, strict=True)
        )
    }
    try:
        stream = stub.StreamSessionPartsBatch(
            StreamSessionPartsBatchRequest(session_names=session_names)
        )
        opened = time.perf_counter()
        for timing in timings:
            timing.stream_opened = opened
        stream_ready.set()

        completed_sessions = set()
        async for event in stream:
            index = by_session.get((event.ns, event.agent, event.session_id))
            if index is None:
                continue

            timing = timings[index]
            now = time.perf_counter()
            if timing.first_part is None:
                timing.first_part = now
            part = event.part
            if part.part_type == SESSION_MESSAGE_PART_TYPE_TEXT and timing.first_token is None:
                timing.first_token = now
            if (
                event.kind == SESSION_MESSAGE_PART_EVENT_KIND_DONE
                and timing.completed is None
            ):
                timing.completed = now
                completed_sessions.add(index)
            elif (
                event.kind == SESSION_MESSAGE_PART_EVENT_KIND_ERROR
                and timing.errored is None
            ):
                timing.errored = now
                timing.error = part.content or "stream emitted error part"
                completed_sessions.add(index)

            if len(completed_sessions) == len(timings):
                stream.cancel()
                return
    except asyncio.CancelledError:
        raise
    except Exception as exc:
        now = time.perf_counter()
        for timing in timings:
            if timing.completed is None and timing.error is None:
                timing.errored = now
                timing.error = repr(exc)
        stream_ready.set()


async def send_message(
    stub: GatewayServiceStub,
    ns: str,
    agent: str,
    session_id: str,
    timings: RunTimings,
) -> None:
    timings.send_started = time.perf_counter()
    await stub.SendMessage(
        SendMessageRequest(
            ns=ns,
            agent=agent,
            session_id=session_id,
            message=f"benchmark message for {agent}",
        )
    )
    timings.send_finished = time.perf_counter()


async def run_workload(
    grpc_target: str,
    agents: int,
    provision_concurrency: int,
    send_timeout_seconds: float,
    worker_warmup_seconds: float,
    progress_interval_seconds: float,
    mock_metrics_url: str | None,
    stats_samples: list[dict[str, Any]],
    stream_mode: str,
) -> dict[str, Any]:
    ns = f"bench-{agents}-{int(time.time())}"
    options = [
        ("grpc.max_receive_message_length", 64 * 1024 * 1024),
        ("grpc.max_send_message_length", 64 * 1024 * 1024),
    ]
    async with grpc.aio.insecure_channel(grpc_target, options=options) as channel:
        await channel.channel_ready()
        stub = GatewayServiceStub(channel)

        if worker_warmup_seconds > 0:
            await asyncio.sleep(worker_warmup_seconds)

        provision_started = time.perf_counter()
        session_ids = await provision(stub, ns, agents, provision_concurrency)
        provision_finished = time.perf_counter()
        print(
            f"provisioned {agents} agents/sessions in {provision_finished - provision_started:.2f}s",
            flush=True,
        )

        timings = [RunTimings() for _ in range(agents)]
        agent_names = [f"agent-{index:04d}" for index in range(agents)]
        task_timing_indexes: dict[asyncio.Task[Any], int | None] = {}
        if stream_mode == "batch":
            stream_ready = [asyncio.Event()]
            stream_tasks = [
                asyncio.create_task(
                    consume_batch_stream(
                        stub,
                        ns,
                        agent_names,
                        session_ids,
                        timings,
                        stream_ready[0],
                    )
                )
            ]
            task_timing_indexes[stream_tasks[0]] = None
        else:
            stream_ready = [asyncio.Event() for _ in range(agents)]
            stream_tasks = []
            for index in range(agents):
                task = asyncio.create_task(
                    consume_stream(
                        stub,
                        ns,
                        agent_names[index],
                        session_ids[index],
                        timings[index],
                        stream_ready[index],
                    )
                )
                stream_tasks.append(task)
                task_timing_indexes[task] = index

        await asyncio.wait_for(asyncio.gather(*(event.wait() for event in stream_ready)), timeout=60)
        await asyncio.sleep(1.0)

        workload_started = time.perf_counter()
        await asyncio.gather(
            *(
                send_message(
                    stub,
                    ns,
                    agent_names[index],
                    session_ids[index],
                    timings[index],
                )
                for index in range(agents)
            )
        )

        async def print_progress() -> None:
            completed = sum(1 for timing in timings if timing.completed is not None)
            errored = sum(1 for timing in timings if timing.error is not None)
            first_parts = sum(1 for timing in timings if timing.first_part is not None)
            first_tokens = sum(1 for timing in timings if timing.first_token is not None)
            sent = sum(1 for timing in timings if timing.send_finished is not None)
            outstanding = agents - completed - errored

            mock_part = ""
            if mock_metrics_url:
                try:
                    mock_metrics = await asyncio.to_thread(fetch_json, mock_metrics_url, 2.0)
                    mock_part = (
                        f" mock_requests={mock_metrics.get('requests')}"
                        f" mock_in_flight={mock_metrics.get('in_flight')}"
                        f" mock_max_in_flight={mock_metrics.get('max_in_flight')}"
                    )
                except Exception as exc:
                    mock_part = f" mock_metrics_error={type(exc).__name__}"

            stats_part = ""
            if stats_samples:
                latest = stats_samples[-1]
                memory_usage = latest.get("memory_usage_bytes")
                memory_limit = latest.get("memory_limit_bytes")
                cpu_percent = latest.get("cpu_percent")
                if isinstance(cpu_percent, (int, float)):
                    stats_part += f" talon_cpu={cpu_percent:.1f}%"
                if isinstance(memory_usage, int):
                    stats_part += f" talon_mem={mib(memory_usage)}/{mib(memory_limit)}"

            print(
                "progress "
                f"elapsed={time.perf_counter() - workload_started:.1f}s "
                f"sent={sent}/{agents} first_part={first_parts} first_token={first_tokens} "
                f"completed={completed} errors={errored} outstanding={outstanding}"
                f"{mock_part}{stats_part}",
                flush=True,
            )

        pending = set(stream_tasks)
        done: set[asyncio.Task[Any]] = set()
        deadline = time.perf_counter() + send_timeout_seconds
        next_progress_at = time.perf_counter()
        if progress_interval_seconds > 0:
            await print_progress()
            next_progress_at = time.perf_counter() + progress_interval_seconds
        while pending and time.perf_counter() < deadline:
            timeout = deadline - time.perf_counter()
            if progress_interval_seconds > 0:
                timeout = min(timeout, max(0.1, next_progress_at - time.perf_counter()))
            newly_done, pending = await asyncio.wait(
                pending,
                timeout=timeout,
                return_when=asyncio.FIRST_COMPLETED,
            )
            done.update(newly_done)
            if progress_interval_seconds > 0 and time.perf_counter() >= next_progress_at:
                await print_progress()
                next_progress_at = time.perf_counter() + progress_interval_seconds
        done.update(task for task in stream_tasks if task.done())
        pending = {task for task in stream_tasks if not task.done()}
        workload_finished = time.perf_counter()
        for task in pending:
            task.cancel()
        if pending:
            await asyncio.gather(*pending, return_exceptions=True)
        for task in stream_tasks:
            if task in done and task.exception():
                index = task_timing_indexes.get(task)
                exc_repr = repr(task.exception())
                if index is not None:
                    timings[index].errored = timings[index].errored or time.perf_counter()
                    timings[index].error = timings[index].error or exc_repr
                else:
                    for timing in timings:
                        if timing.completed is None and timing.error is None:
                            timing.errored = timing.errored or time.perf_counter()
                            timing.error = timing.error or exc_repr

    successes = [t for t in timings if t.completed is not None and t.error is None]
    errors = [t for t in timings if t.error is not None]
    timeouts = [t for t in timings if t.completed is None and t.error is None]
    send_to_done = [
        t.completed - t.send_started
        for t in successes
        if t.completed is not None and t.send_started is not None
    ]
    send_to_first_part = [
        t.first_part - t.send_started
        for t in timings
        if t.first_part is not None and t.send_started is not None
    ]
    send_to_first_token = [
        t.first_token - t.send_started
        for t in timings
        if t.first_token is not None and t.send_started is not None
    ]
    return {
        "namespace": ns,
        "agents": agents,
        "stream_mode": stream_mode,
        "provision_seconds": provision_finished - provision_started,
        "workload_seconds": workload_finished - workload_started,
        "success_count": len(successes),
        "error_count": len(errors),
        "timeout_count": len(timeouts),
        "throughput_completed_per_second": len(successes)
        / max(0.001, workload_finished - workload_started),
        "send_to_done_seconds": summarize_seconds(send_to_done),
        "send_to_first_part_seconds": summarize_seconds(send_to_first_part),
        "send_to_first_token_seconds": summarize_seconds(send_to_first_token),
        "errors": [t.error for t in errors[:20]],
    }


def run_command(args: list[str], check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=REPO_ROOT,
        check=check,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def compose_command(
    project_name: str,
    compose_path: Path,
    *args: str,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    return run_command(
        ["docker", "compose", "-p", project_name, "-f", str(compose_path), *args],
        check=check,
    )


def compose_up(project_name: str, compose_path: Path) -> None:
    result = compose_command(project_name, compose_path, "up", "-d", "--remove-orphans")
    if result.stdout.strip():
        print(result.stdout.strip(), flush=True)


def compose_down(project_name: str, compose_path: Path) -> None:
    result = compose_command(
        project_name,
        compose_path,
        "down",
        "--remove-orphans",
        check=False,
    )
    if result.returncode != 0:
        print(result.stderr.strip(), flush=True)


def compose_port(project_name: str, compose_path: Path, service: str, port: int) -> tuple[str, int]:
    result = compose_command(project_name, compose_path, "port", service, str(port))
    endpoint = result.stdout.strip().splitlines()[-1]
    host, raw_port = endpoint.rsplit(":", 1)
    host = host.strip("[]")
    if host in {"0.0.0.0", "::"}:
        host = "127.0.0.1"
    return host, int(raw_port)


def docker_inspect(container_name: str) -> dict[str, Any]:
    result = run_command(["docker", "inspect", container_name])
    inspected = json.loads(result.stdout)
    return inspected[0] if inspected else {}


def parse_percent(value: str | None) -> float | None:
    if not value:
        return None
    try:
        return float(value.strip().rstrip("%"))
    except ValueError:
        return None


def parse_size_bytes(value: str | None) -> int | None:
    if not value:
        return None
    value = value.strip()
    match = re.match(r"^([0-9]+(?:\.[0-9]+)?)\s*([A-Za-z]+)?$", value)
    if not match:
        return None
    amount = float(match.group(1))
    unit = (match.group(2) or "B").lower()
    multipliers = {
        "b": 1,
        "kb": 1000,
        "kib": 1024,
        "mb": 1000**2,
        "mib": 1024**2,
        "gb": 1000**3,
        "gib": 1024**3,
        "tb": 1000**4,
        "tib": 1024**4,
    }
    multiplier = multipliers.get(unit)
    if multiplier is None:
        return None
    return int(amount * multiplier)


def docker_stats_sample(container_name: str) -> dict[str, Any]:
    result = run_command(
        ["docker", "stats", "--no-stream", "--format", "{{json .}}", container_name]
    )
    if not result.stdout.strip():
        return {"timestamp": time.time(), "error": "no docker stats output"}
    stats = json.loads(result.stdout.strip().splitlines()[-1])
    memory_usage = None
    memory_limit = None
    memory_parts = (stats.get("MemUsage") or "").split("/")
    if memory_parts:
        memory_usage = parse_size_bytes(memory_parts[0].strip())
    if len(memory_parts) > 1:
        memory_limit = parse_size_bytes(memory_parts[1].strip())
    return {
        "timestamp": time.time(),
        "cpu_percent": parse_percent(stats.get("CPUPerc")),
        "memory_usage_bytes": memory_usage,
        "memory_limit_bytes": memory_limit,
        "raw": stats,
    }


def summarize_container_samples(samples: list[dict[str, Any]]) -> dict[str, Any]:
    memory_values = [
        sample["memory_usage_bytes"]
        for sample in samples
        if isinstance(sample.get("memory_usage_bytes"), int)
    ]
    cpu_values = [
        sample["cpu_percent"]
        for sample in samples
        if isinstance(sample.get("cpu_percent"), (int, float))
    ]
    return {
        "peak_memory_usage_bytes": max(memory_values) if memory_values else None,
        "avg_cpu_percent": statistics.mean(cpu_values) if cpu_values else None,
        "peak_cpu_percent": max(cpu_values) if cpu_values else None,
    }


async def sample_container_stats(
    container_name: str,
    samples: list[dict[str, Any]],
    stop_event: asyncio.Event,
    interval_seconds: float = 1.0,
) -> None:
    while not stop_event.is_set():
        try:
            samples.append(await asyncio.to_thread(docker_stats_sample, container_name))
        except Exception as exc:
            samples.append({"timestamp": time.time(), "error": repr(exc)})
        with contextlib.suppress(asyncio.TimeoutError):
            await asyncio.wait_for(stop_event.wait(), timeout=interval_seconds)


def sqlite_size_bytes(data_dir: Path) -> int:
    return sum(path.stat().st_size for path in data_dir.glob("*.db*") if path.is_file())


def fetch_json(url: str, timeout_seconds: float = 5.0) -> dict[str, Any]:
    with urlopen(url, timeout=timeout_seconds) as response:
        return json.loads(response.read().decode("utf-8"))


def jaeger_tag_value(span: dict[str, Any], key: str) -> Any:
    for tag in span.get("tags") or []:
        if tag.get("key") == key:
            return tag.get("value")
    return None


def numeric_tag(span: dict[str, Any], key: str) -> int | float | None:
    value = jaeger_tag_value(span, key)
    if isinstance(value, (int, float)):
        return value
    if isinstance(value, str):
        with contextlib.suppress(ValueError):
            if "." in value:
                return float(value)
            return int(value)
    return None


def summarize_jaeger_db_spans(
    jaeger_url: str,
    trace_limit: int,
    wait_seconds: float = 10.0,
) -> dict[str, Any]:
    params = urlencode(
        {
            "service": "talon-worker",
            "lookback": "1h",
            "limit": trace_limit,
        }
    )
    url = f"{jaeger_url}/api/traces?{params}"
    deadline = time.monotonic() + wait_seconds
    payload: dict[str, Any] = {}
    while True:
        try:
            payload = fetch_json(url, timeout_seconds=30.0)
            if payload.get("data"):
                break
        except Exception:
            pass
        if time.monotonic() >= deadline:
            break
        time.sleep(1.0)
    by_operation: dict[str, dict[str, list[int | float]]] = {}
    trace_count = 0
    span_count = 0
    for trace in payload.get("data") or []:
        trace_count += 1
        for span in trace.get("spans") or []:
            operation = span.get("operationName") or ""
            if not (
                operation.startswith("SqliteKvStore.")
                or operation.startswith("PostgresKvStore.")
                or operation.startswith("sqlite.")
            ):
                continue
            span_count += 1
            bucket = by_operation.setdefault(
                operation,
                {
                    "duration_us": [],
                    "pool_wait_us": [],
                    "query_elapsed_us": [],
                    "rows_returned": [],
                    "rows_affected": [],
                    "value_bytes": [],
                },
            )
            if isinstance(span.get("duration"), (int, float)):
                bucket["duration_us"].append(span["duration"])
            for key in (
                "pool_wait_us",
                "query_elapsed_us",
                "rows_returned",
                "rows_affected",
                "value_bytes",
            ):
                value = numeric_tag(span, key)
                if isinstance(value, (int, float)):
                    bucket[key].append(value)

    return {
        "trace_limit": trace_limit,
        "trace_count": trace_count,
        "span_count": span_count,
        "operations": {
            operation: {
                key: summarize_numbers(values)
                for key, values in sorted(metrics.items())
                if values
            }
            for operation, metrics in sorted(by_operation.items())
        },
    }


def decode_logs(container_name: str, max_chars: int = 20000) -> dict[str, str]:
    try:
        result = run_command(["docker", "logs", "--tail", "1000", container_name], check=False)
    except Exception as exc:
        return {"error": repr(exc)}
    return {
        "stdout_tail": result.stdout[-max_chars:],
        "stderr_tail": result.stderr[-max_chars:],
    }


async def run_profile(
    image: str,
    latency_ms: int,
    args: argparse.Namespace,
    output_dir: Path,
) -> dict[str, Any]:
    project_name = args.project_name
    profile_dir = output_dir / project_name / f"{latency_ms}ms"
    profile_dir.mkdir(parents=True, exist_ok=True)
    config_path = profile_dir / "talon.bench.yaml"
    data_dir = profile_dir / "data"
    compose_path = profile_dir / "compose.yaml"
    data_dir.mkdir(exist_ok=True)
    cpu_profile_container_path = Path(
        f"/data/talon/bench/talon-worker-cpu-{latency_ms}ms.svg"
    )
    cpu_profile_host_path = data_dir / f"talon-worker-cpu-{latency_ms}ms.svg"
    heap_profile_container_dir = Path("/data/talon/bench")
    heap_profile_label = f"heap-{latency_ms}ms"
    heap_profile_host_paths = [
        data_dir / f"talon-server-{heap_profile_label}.heap",
        data_dir / f"talon-worker-{heap_profile_label}.heap",
    ]
    heap_profile_stats_host_paths = [
        data_dir / f"talon-server-{heap_profile_label}.json",
        data_dir / f"talon-worker-{heap_profile_label}.json",
    ]
    write_talon_config(config_path, args.database)
    write_compose_file(
        path=compose_path,
        project_name=project_name,
        image=image,
        config_path=config_path,
        data_dir=data_dir,
        memory=args.memory,
        worker_concurrency=args.worker_concurrency,
        local_socket_buffer_size=args.local_socket_buffer_size,
        latency_ms=latency_ms,
        tokens_per_second=args.tokens_per_second,
        response_tokens=args.response_tokens,
        mock_request_backlog=args.mock_request_backlog,
        mock_cpus=args.mock_cpus,
        mock_memory=args.mock_memory,
        database=args.database,
        sqlite_pool_size=args.sqlite_pool_size,
        sqlite_busy_timeout_ms=args.sqlite_busy_timeout_ms,
        postgres_cpus=args.postgres_cpus,
        postgres_memory=args.postgres_memory,
        postgres_max_connections=args.postgres_max_connections,
        postgres_server_max_connections=args.postgres_server_max_connections,
        otel=args.otel,
        cpu_profile=args.cpu_profile,
        cpu_profile_path=cpu_profile_container_path,
        cpu_profile_seconds=args.cpu_profile_seconds,
        cpu_profile_delay_seconds=args.cpu_profile_delay_seconds,
        cpu_profile_frequency_hz=args.cpu_profile_frequency_hz,
        heap_profile=args.heap_profile,
        heap_profile_dir=heap_profile_container_dir,
        heap_profile_delay_seconds=args.heap_profile_delay_seconds,
        heap_profile_label=heap_profile_label,
    )

    talon_container = f"{project_name}-talon"
    mock_container = f"{project_name}-mock-llm"
    postgres_container = f"{project_name}-postgres" if args.database == "postgres" else None
    stats_samples: list[dict[str, Any]] = []
    postgres_stats_samples: list[dict[str, Any]] = []
    stop_stats = asyncio.Event()
    stats_tasks: list[asyncio.Task[Any]] = []
    workload: dict[str, Any] | None = None
    mock_metrics: dict[str, Any] = {}
    db_trace_summary: dict[str, Any] | None = None
    started_at = time.perf_counter()

    try:
        compose_down(project_name, compose_path)
        compose_up(project_name, compose_path)
        mock_host, mock_port = compose_port(project_name, compose_path, "mock-llm", MOCK_LLM_PORT)
        grpc_host, grpc_port = compose_port(project_name, compose_path, "talon", TALON_GRPC_PORT)
        jaeger_url = None
        if args.otel:
            jaeger_host, jaeger_port = compose_port(project_name, compose_path, "jaeger", 16686)
            jaeger_url = f"http://{jaeger_host}:{jaeger_port}"
        postgres_endpoint = None
        if args.database == "postgres":
            pg_host, pg_port = compose_port(
                project_name, compose_path, "postgres", POSTGRES_PORT
            )
            postgres_endpoint = f"{pg_host}:{pg_port}"
        mock_metrics_url = f"http://{mock_host}:{mock_port}/metrics"
        grpc_target = f"{grpc_host}:{grpc_port}"
        print(
            f"compose project={project_name} talon={talon_container} "
            f"mock={mock_container} grpc={grpc_target} mock_metrics={mock_metrics_url} "
            f"database={args.database} "
            f"stream_mode={args.stream_mode} "
            f"sqlite_pool={args.sqlite_pool_size} sqlite_busy_timeout_ms={args.sqlite_busy_timeout_ms} "
            f"postgres_pool={args.postgres_max_connections} "
            f"postgres_server_max={args.postgres_server_max_connections}",
            flush=True,
        )
        if postgres_endpoint:
            print(
                f"postgres=postgres://talon:talon@{postgres_endpoint}/talon",
                flush=True,
            )
        if jaeger_url:
            print(f"jaeger_ui={jaeger_url}", flush=True)
        wait_for_port(mock_host, mock_port)
        await wait_for_channel(grpc_target, timeout_seconds=90)

        stats_tasks.append(
            asyncio.create_task(sample_container_stats(talon_container, stats_samples, stop_stats))
        )
        if postgres_container:
            stats_tasks.append(
                asyncio.create_task(
                    sample_container_stats(
                        postgres_container, postgres_stats_samples, stop_stats
                    )
                )
            )
        workload = await run_workload(
            grpc_target=grpc_target,
            agents=args.agents,
            provision_concurrency=args.provision_concurrency,
            send_timeout_seconds=args.timeout_seconds,
            worker_warmup_seconds=args.worker_warmup_seconds,
            progress_interval_seconds=args.progress_interval_seconds,
            mock_metrics_url=mock_metrics_url,
            stats_samples=stats_samples,
            stream_mode=args.stream_mode,
        )
        try:
            mock_metrics = await asyncio.to_thread(fetch_json, mock_metrics_url)
        except Exception as exc:
            print(f"warning: failed to fetch mock LLM metrics: {exc}", flush=True)
            mock_metrics = {}
        if jaeger_url and args.jaeger_trace_limit > 0:
            try:
                db_trace_summary = await asyncio.to_thread(
                    summarize_jaeger_db_spans,
                    jaeger_url,
                    args.jaeger_trace_limit,
                )
                print(
                    "db trace summary "
                    f"traces={db_trace_summary['trace_count']} "
                    f"spans={db_trace_summary['span_count']}",
                    flush=True,
                )
            except Exception as exc:
                db_trace_summary = {"error": repr(exc)}
        if args.cpu_profile:
            profile_wait_seconds = max(
                1.0,
                args.cpu_profile_delay_seconds
                + args.cpu_profile_seconds
                - (time.perf_counter() - started_at)
                + 5.0,
            )
            if await wait_for_file(cpu_profile_host_path, profile_wait_seconds):
                print(f"cpu profile={cpu_profile_host_path}", flush=True)
            else:
                print(
                    f"cpu profile was not written within {profile_wait_seconds:.1f}s",
                    flush=True,
                )
        if args.heap_profile:
            profile_wait_seconds = max(
                1.0,
                args.heap_profile_delay_seconds - (time.perf_counter() - started_at) + 10.0,
            )
            missing = []
            for heap_path in heap_profile_host_paths:
                if not await wait_for_file(heap_path, profile_wait_seconds):
                    missing.append(heap_path)
            for stats_path in heap_profile_stats_host_paths:
                if not await wait_for_file(stats_path, 1.0):
                    missing.append(stats_path)
            if missing:
                print(
                    "heap profile files missing: "
                    + ", ".join(str(path) for path in missing),
                    flush=True,
                )
            else:
                print(
                    "heap profiles="
                    + ", ".join(str(path) for path in heap_profile_host_paths),
                    flush=True,
                )
    finally:
        stop_stats.set()
        if stats_tasks:
            await asyncio.gather(*stats_tasks, return_exceptions=True)

    finished_at = time.perf_counter()
    container_state: dict[str, Any] = {}
    with contextlib.suppress(Exception):
        container_state = docker_inspect(talon_container).get("State", {})

    talon_stats = summarize_container_samples(stats_samples)
    postgres_stats = summarize_container_samples(postgres_stats_samples)
    result = {
        "project_name": project_name,
        "compose_file": str(compose_path),
        "containers": {
            "talon": talon_container,
            "mock_llm": mock_container,
            "postgres": postgres_container,
            "jaeger": f"{project_name}-jaeger" if args.otel else None,
        },
        "latency_ms": latency_ms,
        "resource_limits": {"cpu": "1 vCPU", "memory": args.memory},
        "database": {
            "driver": args.database,
            "postgres_max_connections": args.postgres_max_connections
            if args.database == "postgres"
            else None,
            "postgres_server_max_connections": args.postgres_server_max_connections
            if args.database == "postgres"
            else None,
            "postgres_cpus": args.postgres_cpus if args.database == "postgres" else None,
            "postgres_memory": args.postgres_memory if args.database == "postgres" else None,
            "trace_summary": db_trace_summary,
        },
        "sqlite": {
            "pool_size": args.sqlite_pool_size,
            "busy_timeout_ms": args.sqlite_busy_timeout_ms,
            "trace_summary": db_trace_summary if args.database == "sqlite" else None,
        },
        "total_seconds": finished_at - started_at,
        "cpu_profile": {
            "enabled": args.cpu_profile,
            "path": str(cpu_profile_host_path) if cpu_profile_host_path.exists() else None,
            "seconds": args.cpu_profile_seconds if args.cpu_profile else None,
            "delay_seconds": args.cpu_profile_delay_seconds if args.cpu_profile else None,
            "frequency_hz": args.cpu_profile_frequency_hz if args.cpu_profile else None,
        },
        "heap_profile": {
            "enabled": args.heap_profile,
            "paths": [
                str(path) for path in heap_profile_host_paths if path.exists()
            ],
            "stats_paths": [
                str(path) for path in heap_profile_stats_host_paths if path.exists()
            ],
            "delay_seconds": args.heap_profile_delay_seconds if args.heap_profile else None,
        },
        "workload": workload or {},
        "mock_llm": mock_metrics,
        "mock_llm_config": {
            "request_backlog": args.mock_request_backlog,
            "cpus": args.mock_cpus,
            "memory": args.mock_memory,
        },
        "talon_container": {
            "oom_killed": bool(container_state.get("OOMKilled")),
            "exit_code": container_state.get("ExitCode"),
            "status": container_state.get("Status"),
            **talon_stats,
        },
        "postgres_container": postgres_stats if postgres_container else None,
        "sqlite_size_bytes": sqlite_size_bytes(data_dir),
        "stats_samples": stats_samples,
        "postgres_stats_samples": postgres_stats_samples,
        "logs": {
            "talon": decode_logs(talon_container),
            "mock_llm": decode_logs(mock_container),
            "postgres": decode_logs(postgres_container) if postgres_container else None,
        },
    }

    profile_path = output_dir / f"talon-bench-{args.agents}-agents-{latency_ms}ms.json"
    profile_path.write_text(json.dumps(result, indent=2), encoding="utf-8")

    if not args.keep_compose_up:
        compose_down(project_name, compose_path)
    return result


def write_summary(output_dir: Path, results: list[dict[str, Any]]) -> None:
    lines = [
        "# Talon 1000-Agent Benchmark",
        "",
        "| LLM latency | Success | Errors | Timeouts | Workload seconds | Completed/sec | p95 send-to-done | Peak memory | OOM |",
        "| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | :---: |",
    ]
    for result in results:
        workload = result["workload"]
        p95 = workload["send_to_done_seconds"]["p95"]
        peak_memory = result["talon_container"]["peak_memory_usage_bytes"]
        lines.append(
            "| {latency} ms | {success} | {errors} | {timeouts} | {seconds:.2f} | "
            "{throughput:.2f} | {p95} | {peak_memory} | {oom} |".format(
                latency=result["latency_ms"],
                success=workload["success_count"],
                errors=workload["error_count"],
                timeouts=workload["timeout_count"],
                seconds=workload["workload_seconds"],
                throughput=workload["throughput_completed_per_second"],
                p95=f"{p95:.3f}s" if isinstance(p95, (int, float)) else "n/a",
                peak_memory=f"{peak_memory / (1024 * 1024):.1f} MiB"
                if isinstance(peak_memory, int)
                else "n/a",
                oom="yes" if result["talon_container"]["oom_killed"] else "no",
            )
        )
    db_lines = []
    for result in results:
        database = (result.get("database") or {}).get("driver") or "sqlite"
        trace_summary = (result.get("database") or {}).get("trace_summary")
        if not trace_summary:
            trace_summary = (result.get("sqlite") or {}).get("trace_summary")
        summary = (trace_summary or {}).get("operations") or {}
        prefix = "PostgresKvStore" if database == "postgres" else "SqliteKvStore"
        acquire = summary.get(f"{prefix}.acquire_connection") or summary.get("sqlite.pool_acquire")
        query = summary.get(f"{prefix}.query") or summary.get("sqlite.query")
        acquire_p95 = ((acquire or {}).get("pool_wait_us") or {}).get("p95")
        query_p95 = ((query or {}).get("query_elapsed_us") or {}).get("p95")
        if acquire_p95 is None and query_p95 is None:
            continue
        pool = (
            (result.get("database") or {}).get("postgres_max_connections")
            if database == "postgres"
            else (result.get("sqlite") or {}).get("pool_size", "n/a")
        )
        db_lines.append(
            "| {latency} ms | {database} | {pool} | {acquire} | {query} |".format(
                latency=result["latency_ms"],
                database=database,
                pool=pool,
                acquire=f"{acquire_p95 / 1000:.2f} ms"
                if isinstance(acquire_p95, (int, float))
                else "n/a",
                query=f"{query_p95 / 1000:.2f} ms"
                if isinstance(query_p95, (int, float))
                else "n/a",
            )
        )
    if db_lines:
        lines.extend(
            [
                "",
                "## Database Trace Summary",
                "",
                "| LLM latency | Database | Pool max connections | p95 pool wait | p95 query elapsed |",
                "| ---: | :--- | ---: | ---: | ---: |",
                *db_lines,
            ]
        )
    (output_dir / "summary.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def validate_result(result: dict[str, Any], agents: int) -> None:
    workload = result["workload"]
    required_successes = max(1, math.ceil(agents * 0.99))
    if workload["success_count"] < required_successes:
        raise RuntimeError(
            f"latency {result['latency_ms']}ms failed success threshold: "
            f"{workload['success_count']}/{agents}"
        )
    if result["talon_container"]["oom_killed"]:
        raise RuntimeError(f"latency {result['latency_ms']}ms OOM-killed Talon")


async def amain() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--agents", type=int, default=1000)
    parser.add_argument("--latencies", default="0,50,250")
    parser.add_argument("--memory", default="512m")
    parser.add_argument("--tokens-per-second", type=float, default=10.0)
    parser.add_argument("--response-tokens", type=int, default=50)
    parser.add_argument("--mock-request-backlog", type=int, default=4096)
    parser.add_argument("--mock-cpus", type=float, default=None)
    parser.add_argument("--mock-memory", default=None)
    parser.add_argument("--timeout-seconds", type=float, default=900)
    parser.add_argument("--provision-concurrency", type=int, default=100)
    parser.add_argument("--worker-concurrency", type=int, default=1000)
    parser.add_argument("--local-socket-buffer-size", type=int, default=10000)
    parser.add_argument(
        "--stream-mode",
        choices=("per-session", "batch"),
        default="per-session",
        help="Use one stream per session or one batched stream for all sessions.",
    )
    parser.add_argument("--database", choices=("sqlite", "postgres"), default="sqlite")
    parser.add_argument("--sqlite-pool-size", type=int, default=5)
    parser.add_argument("--sqlite-busy-timeout-ms", type=int, default=5000)
    parser.add_argument("--postgres-max-connections", type=int, default=200)
    parser.add_argument("--postgres-server-max-connections", type=int, default=None)
    parser.add_argument("--postgres-cpus", type=float, default=None)
    parser.add_argument("--postgres-memory", default=None)
    parser.add_argument("--jaeger-trace-limit", type=int, default=200)
    parser.add_argument("--worker-warmup-seconds", type=float, default=3.0)
    parser.add_argument("--progress-interval-seconds", type=float, default=5.0)
    parser.add_argument("--image-tag", default="talon-bench-runtime:latest")
    parser.add_argument("--dockerfile", default="bench/runtime.Dockerfile")
    parser.add_argument("--project-name")
    parser.add_argument("--keep-compose-up", action="store_true")
    parser.add_argument("--otel", action="store_true")
    parser.add_argument("--cpu-profile", action="store_true")
    parser.add_argument("--cpu-profile-seconds", type=int, default=15)
    parser.add_argument("--cpu-profile-delay-seconds", type=int, default=2)
    parser.add_argument("--cpu-profile-frequency-hz", type=int, default=99)
    parser.add_argument("--heap-profile", action="store_true")
    parser.add_argument("--heap-profile-delay-seconds", type=int, default=8)
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--no-cache", action="store_true")
    parser.add_argument("--output-dir", default="bench/results")
    args = parser.parse_args()
    args.project_name = args.project_name or generate_project_name()
    validate_project_name(args.project_name)

    output_dir = Path(args.output_dir).resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    latencies = parse_latencies(args.latencies)
    if args.sqlite_pool_size <= 0:
        raise ValueError("--sqlite-pool-size must be greater than 0")
    if args.sqlite_busy_timeout_ms < 0:
        raise ValueError("--sqlite-busy-timeout-ms must be non-negative")
    if args.jaeger_trace_limit < 0:
        raise ValueError("--jaeger-trace-limit must be non-negative")
    if args.mock_request_backlog <= 0:
        raise ValueError("--mock-request-backlog must be greater than 0")
    if args.local_socket_buffer_size <= 0:
        raise ValueError("--local-socket-buffer-size must be greater than 0")
    if args.mock_cpus is not None and args.mock_cpus <= 0:
        raise ValueError("--mock-cpus must be greater than 0")
    if args.postgres_max_connections <= 0:
        raise ValueError("--postgres-max-connections must be greater than 0")
    if args.postgres_server_max_connections is None:
        args.postgres_server_max_connections = max(200, args.postgres_max_connections * 2 + 50)
    if args.postgres_server_max_connections <= 0:
        raise ValueError("--postgres-server-max-connections must be greater than 0")
    if args.postgres_server_max_connections < args.postgres_max_connections:
        raise ValueError(
            "--postgres-server-max-connections must be at least --postgres-max-connections"
        )
    if args.postgres_cpus is not None and args.postgres_cpus <= 0:
        raise ValueError("--postgres-cpus must be greater than 0")
    if args.cpu_profile_seconds <= 0:
        raise ValueError("--cpu-profile-seconds must be greater than 0")
    if args.cpu_profile_delay_seconds < 0:
        raise ValueError("--cpu-profile-delay-seconds must be non-negative")
    if args.cpu_profile_frequency_hz <= 0:
        raise ValueError("--cpu-profile-frequency-hz must be greater than 0")
    if args.heap_profile_delay_seconds < 0:
        raise ValueError("--heap-profile-delay-seconds must be non-negative")

    if not args.skip_build:
        cargo_features = ",".join(
            feature
            for feature in (
                "cpu-profile" if args.cpu_profile else None,
                "heap-profile" if args.heap_profile else None,
            )
            if feature
        )
        build_runtime_image(
            args.image_tag,
            args.no_cache,
            args.dockerfile,
            cargo_features or None,
        )

    results = []
    print(f"using docker compose project {args.project_name}", flush=True)
    for latency_ms in latencies:
        print(
            f"running profile latency_ms={latency_ms} agents={args.agents} "
            f"memory={args.memory} database={args.database}",
            flush=True,
        )
        result = await run_profile(args.image_tag, latency_ms, args, output_dir)
        validate_result(result, args.agents)
        results.append(result)
        print(
            f"completed latency_ms={latency_ms}: "
            f"{result['workload']['success_count']}/{args.agents} succeeded",
            flush=True,
        )
    write_summary(output_dir, results)
    print(f"wrote benchmark results to {output_dir}", flush=True)


if __name__ == "__main__":
    asyncio.run(amain())
