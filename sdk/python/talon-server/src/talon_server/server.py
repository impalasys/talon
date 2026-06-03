from __future__ import annotations

import hashlib
import os
import shutil
import signal
import socket
import subprocess
import tarfile
import tempfile
import threading
import time
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path
from typing import Mapping


@dataclass(frozen=True)
class Provider:
    name: str = "mock"
    base_url: str = ""
    model: str = ""
    api_key: str = ""


@dataclass(frozen=True)
class Options:
    talon_node_path: str | Path | None = None
    version: str = "latest"
    grpc_port: int | None = None
    ui_port: int | None = None
    keep_temp_dir: bool = False
    env: Mapping[str, str] = field(default_factory=dict)
    startup_timeout_seconds: float = 30.0
    provider: Provider | None = None


class Server:
    def __init__(
        self,
        process: subprocess.Popen[bytes],
        temp_dir: Path,
        config_path: Path,
        grpc_port: int,
        ui_port: int,
        keep_temp_dir: bool,
    ) -> None:
        self._process = process
        self.temp_dir = temp_dir
        self.config_path = config_path
        self._grpc_port = grpc_port
        self._ui_port = ui_port
        self._keep_temp_dir = keep_temp_dir
        self._logs: list[bytes] = []
        self._logs_lock = threading.Lock()
        if self._process.stdout is not None:
            thread = threading.Thread(target=self._drain_logs, args=(self._process.stdout,), daemon=True)
            thread.start()

    @classmethod
    def start(cls, options: Options | None = None) -> "Server":
        options = options or Options()
        node_path = _resolve_talon_node(options)
        grpc_port = options.grpc_port or _free_port()
        ui_port = options.ui_port or _free_port()
        temp_dir = Path(tempfile.mkdtemp(prefix="talon-server-"))
        data_dir = temp_dir / "data"
        data_dir.mkdir(parents=True, exist_ok=True)
        config_path = temp_dir / "talon.yaml"
        config_path.write_text(_config_yaml(options.provider), encoding="utf-8")

        env = os.environ.copy()
        env.update(
            {
                "GRPC_ADDR": f"127.0.0.1:{grpc_port}",
                "GATEWAY_UI_ADDR": f"127.0.0.1:{ui_port}",
                "TALON_CONFIG_PATH": str(config_path),
                "RUST_LOG": "info",
            }
        )
        env.update(options.env)
        process = subprocess.Popen(
            [str(node_path)],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            env=env,
        )
        server = cls(process, temp_dir, config_path, grpc_port, ui_port, options.keep_temp_dir)
        try:
            _wait_for_port(grpc_port, options.startup_timeout_seconds)
        except Exception:
            logs = server.logs()
            server.stop()
            raise RuntimeError(f"talon-node did not become ready\n{logs}")
        return server

    @property
    def grpc_endpoint(self) -> str:
        return f"127.0.0.1:{self._grpc_port}"

    @property
    def ui_endpoint(self) -> str:
        return f"http://127.0.0.1:{self._ui_port}"

    def logs(self) -> str:
        with self._logs_lock:
            return b"".join(self._logs).decode("utf-8", errors="replace")

    def stop(self) -> None:
        if self._process.poll() is None:
            self._process.send_signal(signal.SIGINT)
            try:
                self._process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self._process.kill()
                self._process.wait(timeout=2)
        if not self._keep_temp_dir:
            shutil.rmtree(self.temp_dir, ignore_errors=True)

    def __enter__(self) -> "Server":
        return self

    def __exit__(self, exc_type: object, exc: object, traceback: object) -> None:
        self.stop()

    def _drain_logs(self, stream: object) -> None:
        while True:
            chunk = stream.readline()  # type: ignore[attr-defined]
            if not chunk:
                return
            with self._logs_lock:
                self._logs.append(chunk)


def start(options: Options | None = None) -> Server:
    return Server.start(options)


def _resolve_talon_node(options: Options) -> Path:
    if options.talon_node_path:
        return Path(options.talon_node_path)
    if os.environ.get("TALON_NODE_PATH"):
        return Path(os.environ["TALON_NODE_PATH"])
    return _download_talon_node(options.version)


def _download_talon_node(version: str) -> Path:
    platform = _platform_name()
    cache_root = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    target_dir = cache_root / "talon" / "node" / version / platform
    target = target_dir / "talon-node"
    if target.exists():
        return target
    target_dir.mkdir(parents=True, exist_ok=True)
    base = f"https://github.com/impalasys/talon/releases/{version}/download"
    if version != "latest":
        base = f"https://github.com/impalasys/talon/releases/download/{version}"
    archive_url = f"{base}/talon-node-{platform}.tar.gz"
    checksum_url = f"{archive_url}.sha256"
    archive = urllib.request.urlopen(archive_url, timeout=60).read()
    checksum = urllib.request.urlopen(checksum_url, timeout=60).read().decode("utf-8")
    actual = hashlib.sha256(archive).hexdigest()
    if actual != checksum.split()[0]:
        raise RuntimeError("talon-node checksum mismatch")
    archive_path = target_dir / "talon-node.tar.gz"
    archive_path.write_bytes(archive)
    with tarfile.open(archive_path, "r:gz") as tar:
        for member in tar.getmembers():
            if Path(member.name).name == "talon-node":
                member.name = "talon-node"
                tar.extract(member, target_dir)
                target.chmod(0o755)
                return target
    raise RuntimeError("talon-node not found in release archive")


def _platform_name() -> str:
    if os.uname().sysname == "Linux" and os.uname().machine in {"x86_64", "amd64"}:
        return "linux-x64"
    if os.uname().sysname == "Darwin" and os.uname().machine == "arm64":
        return "darwin-arm64"
    raise RuntimeError(f"unsupported talon-node platform: {os.uname().sysname}-{os.uname().machine}")


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def _wait_for_port(port: int, timeout_seconds: float) -> None:
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.25):
                return
        except OSError:
            time.sleep(0.1)
    raise TimeoutError(f"timeout waiting for 127.0.0.1:{port}")


def _config_yaml(provider: Provider | None) -> str:
    prefix = ""
    if provider is not None:
        name = provider.name or "mock"
        prefix = (
            "providers:\n"
            f"  {name}:\n"
            "    type: openai_compatible\n"
            f"    base_url: {provider.base_url!r}\n"
            f"    model: {provider.model!r}\n"
            f"    api_key: {provider.api_key!r}\n"
            f"default_provider: {name!r}\n"
        )
    return (
        prefix
        + "control_plane:\n"
        + "  database:\n"
        + "    driver: sqlite\n"
        + "    data_dir: ./data\n"
        + "  message_broker:\n"
        + "    driver: local_socket\n"
    )
