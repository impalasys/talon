from __future__ import annotations

import base64
import hashlib
import hmac
import json
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
from copy import deepcopy
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Mapping


@dataclass(frozen=True)
class Provider:
    name: str = "mock"
    base_url: str = ""
    model: str = ""
    api_key: str = ""


@dataclass(frozen=True)
class Options:
    talon_node_path: str | Path | None = None
    config_path: str | Path | None = None
    config: Mapping[str, Any] | None = None
    data_dir: str | Path | None = None
    version: str = "latest"
    grpc_port: int | None = None
    ui_port: int | None = None
    keep_temp_dir: bool = False
    env: Mapping[str, str] = field(default_factory=dict)
    startup_timeout_seconds: float = 30.0
    provider: Provider | None = None
    jwt_secret: str | None = None


@dataclass(frozen=True)
class JwtOptions:
    subject: str = "talon-sdk"
    ttl_seconds: int = 3600
    namespace: str | None = None
    agent: str | None = None
    session: str | None = None
    channel: str | None = None


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
        if options.config_path is not None and (
            options.config is not None or options.data_dir is not None or options.provider is not None
        ):
            raise ValueError("config_path cannot be combined with config, data_dir, or provider; put those settings in the config file")
        if options.config is not None and options.provider is not None:
            raise ValueError("config cannot be combined with provider; put providers in the config object")
        node_path = _resolve_talon_node(options)
        grpc_port = options.grpc_port or _free_port()
        ui_port = options.ui_port or _free_port()
        temp_dir = Path(tempfile.mkdtemp(prefix="talon-server-"))
        if options.config_path is not None:
            config_path = Path(options.config_path).expanduser().resolve()
        else:
            data_dir = Path(options.data_dir).expanduser().resolve() if options.data_dir is not None else None
            config = (
                _config_with_data_dir(options.config, data_dir)
                if options.config is not None
                else _default_config(options.provider, data_dir or temp_dir / "data")
            )
            config_data_dir = _control_plane_data_dir(config)
            if config_data_dir is not None:
                if not config_data_dir.is_absolute():
                    config_data_dir = temp_dir / config_data_dir
                config_data_dir.mkdir(parents=True, exist_ok=True)
            config_path = temp_dir / "talon.json"
            config_path.write_text(json.dumps(config, indent=2) + "\n", encoding="utf-8")

        env = os.environ.copy()
        env.update(
            {
                "GRPC_ADDR": f"127.0.0.1:{grpc_port}",
                "GATEWAY_UI_ADDR": f"127.0.0.1:{ui_port}",
                "TALON_CONFIG_PATH": str(config_path),
                "RUST_LOG": "info",
            }
        )
        if options.jwt_secret:
            env["GATEWAY_JWT_SECRET"] = options.jwt_secret
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


def mint_jwt(secret: str, options: JwtOptions | None = None) -> str:
    if not secret:
        raise ValueError("secret is required")
    options = options or JwtOptions()
    if not options.subject.strip():
        raise ValueError("subject is required")
    if options.ttl_seconds <= 0:
        raise ValueError("ttl_seconds must be positive")
    if options.channel is not None and options.namespace is None:
        raise ValueError("channel-scoped JWTs require namespace")

    claims: dict[str, str | int] = {
        "sub": options.subject,
        "aud": "talon",
        "exp": int(time.time()) + int(options.ttl_seconds),
    }
    _add_jwt_claim(claims, "talon:ns", options.namespace)
    _add_jwt_claim(claims, "talon:agent", options.agent)
    _add_jwt_claim(claims, "talon:session", options.session)
    _add_jwt_claim(claims, "talon:channel", options.channel)

    header = _jwt_segment({"alg": "HS256", "typ": "JWT"})
    payload = _jwt_segment(claims)
    message = f"{header}.{payload}"
    signature = _base64url(hmac.new(secret.encode("utf-8"), message.encode("utf-8"), hashlib.sha256).digest())
    return f"{message}.{signature}"


def authorization_header(token: str) -> str:
    if not token.strip():
        raise ValueError("token is required")
    return f"Bearer {token}"


def _add_jwt_claim(claims: dict[str, str | int], key: str, value: str | None) -> None:
    if value is None:
        return
    if not value.strip():
        raise ValueError(f"{key} must not be empty")
    claims[key] = value


def _jwt_segment(value: object) -> str:
    return _base64url(json.dumps(value, separators=(",", ":")).encode("utf-8"))


def _base64url(value: bytes) -> str:
    return base64.urlsafe_b64encode(value).decode("ascii").rstrip("=")


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


def _default_config(provider: Provider | None, data_dir: str | Path) -> dict[str, Any]:
    config: dict[str, Any] = {
        "control_plane": {
            "database": {
                "driver": "sqlite",
                "data_dir": str(data_dir),
            },
            "message_broker": {
                "driver": "local_socket",
            },
        }
    }
    if provider is not None:
        name = provider.name or "mock"
        config["providers"] = {
            name: {
                "type": "openai_compatible",
                "base_url": provider.base_url,
                "model": provider.model,
                "api_key": provider.api_key,
            }
        }
        config["default_provider"] = name
    return config


def _config_with_data_dir(config: Mapping[str, Any], data_dir: Path | None) -> dict[str, Any]:
    copy = deepcopy(dict(config))
    if data_dir is None:
        return copy
    control_plane = copy.setdefault("control_plane", {})
    if not isinstance(control_plane, dict):
        control_plane = {}
        copy["control_plane"] = control_plane
    database = control_plane.setdefault("database", {})
    if not isinstance(database, dict):
        database = {}
        control_plane["database"] = database
    database["data_dir"] = str(data_dir)
    return copy


def _control_plane_data_dir(config: Mapping[str, Any]) -> Path | None:
    control_plane = config.get("control_plane")
    if not isinstance(control_plane, Mapping):
        return None
    database = control_plane.get("database")
    if not isinstance(database, Mapping):
        return None
    data_dir = database.get("data_dir")
    if not isinstance(data_dir, str) or not data_dir.strip():
        return None
    return Path(data_dir)
