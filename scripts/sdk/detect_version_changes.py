#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Callable


ROOT = Path(__file__).resolve().parents[2]


def git_show(rev: str, path: str) -> str | None:
    try:
        return subprocess.check_output(
            ["git", "show", f"{rev}:{path}"],
            cwd=ROOT,
            text=True,
            stderr=subprocess.DEVNULL,
        )
    except subprocess.CalledProcessError:
        return None


def current_text(path: str) -> str | None:
    file = ROOT / path
    return file.read_text() if file.exists() else None


def package_json_version(text: str) -> str:
    return json.loads(text)["version"]


def pyproject_version(text: str) -> str:
    return tomllib.loads(text)["project"]["version"]


def cargo_version(text: str) -> str:
    return tomllib.loads(text)["package"]["version"]


def version_file(text: str) -> str:
    return text.strip()


def gradle_version(text: str) -> str:
    match = re.search(r'^\s*version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if not match:
        raise ValueError("Gradle version assignment not found")
    return match.group(1)


def changed(path: str, parser: Callable[[str], str], base: str) -> tuple[bool, str]:
    now_text = current_text(path)
    if now_text is None:
        return False, ""
    now_version = parser(now_text)
    before_text = git_show(base, path)
    before_version = parser(before_text) if before_text is not None else ""
    return before_version != now_version, now_version


def main() -> int:
    base = os.environ.get("BASE_SHA") or "HEAD^"
    packages: dict[str, tuple[str, Callable[[str], str]]] = {
        "go_client": ("sdk/go/talon-client/VERSION", version_file),
        "go_server": ("sdk/go/talon-server/VERSION", version_file),
        "rust_client": ("sdk/rust/talon-client/Cargo.toml", cargo_version),
        "rust_server": ("sdk/rust/talon-server/Cargo.toml", cargo_version),
        "python_client": ("sdk/python/talon-client/pyproject.toml", pyproject_version),
        "python_server": ("sdk/python/talon-server/pyproject.toml", pyproject_version),
        "js_client": ("sdk/js/talon-client/package.json", package_json_version),
        "js_server": ("sdk/js/talon-server/package.json", package_json_version),
        "js_node_linux": ("sdk/js/talon-node-linux-x64/package.json", package_json_version),
        "js_node_darwin": ("sdk/js/talon-node-darwin-arm64/package.json", package_json_version),
        "js_chat": ("packages/talon-chat/package.json", package_json_version),
        "java_client": ("sdk/java/build.gradle.kts", gradle_version),
        "java_server": ("sdk/java/build.gradle.kts", gradle_version),
    }
    outputs: dict[str, str] = {}
    outputs["sdk_version"] = version_file(current_text("sdk/VERSION") or "")
    for key, (path, parser) in packages.items():
        did_change, version = changed(path, parser, base)
        outputs[f"{key}_changed"] = "true" if did_change else "false"
        outputs[f"{key}_version"] = version

    outputs["go_changed"] = str(
        outputs["go_client_changed"] == "true" or outputs["go_server_changed"] == "true"
    ).lower()
    outputs["rust_changed"] = str(
        outputs["rust_client_changed"] == "true" or outputs["rust_server_changed"] == "true"
    ).lower()
    outputs["python_changed"] = str(
        outputs["python_client_changed"] == "true" or outputs["python_server_changed"] == "true"
    ).lower()
    outputs["js_changed"] = str(
        any(
            outputs[f"{key}_changed"] == "true"
            for key in ["js_client", "js_server", "js_node_linux", "js_node_darwin", "js_chat"]
        )
    ).lower()
    outputs["java_changed"] = str(
        outputs["java_client_changed"] == "true" or outputs["java_server_changed"] == "true"
    ).lower()
    outputs["needs_node_binaries"] = str(
        outputs["go_server_changed"] == "true"
        or outputs["rust_server_changed"] == "true"
        or outputs["python_server_changed"] == "true"
        or outputs["java_server_changed"] == "true"
        or outputs["js_server_changed"] == "true"
        or outputs["js_node_linux_changed"] == "true"
        or outputs["js_node_darwin_changed"] == "true"
    ).lower()

    out_path = os.environ.get("GITHUB_OUTPUT")
    if out_path:
        with open(out_path, "a", encoding="utf-8") as out:
            for key, value in outputs.items():
                out.write(f"{key}={value}\n")
    else:
        for key, value in outputs.items():
            print(f"{key}={value}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
