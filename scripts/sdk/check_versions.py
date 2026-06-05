#!/usr/bin/env python3
from __future__ import annotations

import json
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SDK_VERSION = (ROOT / "sdk" / "VERSION").read_text(encoding="utf-8").strip()


def version_file(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8").strip()


def assignment_version(path: str, label: str = "version") -> str:
    text = (ROOT / path).read_text(encoding="utf-8")
    match = re.search(rf'^\s*{label}\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if not match:
        raise ValueError(f"{label} assignment not found in {path}")
    return match.group(1)


def cargo_version(path: str) -> str:
    return assignment_version(path)


def pyproject_version(path: str) -> str:
    return assignment_version(path)


def package_json_version(path: str) -> str:
    return json.loads((ROOT / path).read_text(encoding="utf-8"))["version"]


def gradle_version(path: str) -> str:
    return assignment_version(path)


PACKAGES = {
    "sdk/go/talon-client/VERSION": version_file,
    "sdk/go/talon-server/VERSION": version_file,
    "sdk/rust/talon-client/Cargo.toml": cargo_version,
    "sdk/rust/talon-server/Cargo.toml": cargo_version,
    "sdk/python/talon-client/pyproject.toml": pyproject_version,
    "sdk/python/talon-server/pyproject.toml": pyproject_version,
    "sdk/java/build.gradle.kts": gradle_version,
    "sdk/js/talon-client/package.json": package_json_version,
    "sdk/js/talon-server/package.json": package_json_version,
    "sdk/js/talon-node-darwin-arm64/package.json": package_json_version,
    "sdk/js/talon-node-linux-x64/package.json": package_json_version,
}


def main() -> int:
    ok = True
    for path, parser in PACKAGES.items():
        try:
            version = parser(path)
            if version != SDK_VERSION:
                print(f"{path} has version {version}, expected sdk/VERSION {SDK_VERSION}", file=sys.stderr)
                ok = False
        except Exception as error:
            print(f"error parsing {path}: {error}", file=sys.stderr)
            ok = False
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
