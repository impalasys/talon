#!/usr/bin/env python3
"""Bump every public Talon SDK artifact to one coordinated version."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
VERSION_FILE = ROOT / "sdk" / "VERSION"

VERSION_FILES = [
    "sdk/VERSION",
    "sdk/go/talon-client/VERSION",
    "sdk/go/talon-server/VERSION",
]

MANIFEST_VERSION_PATTERNS = {
    "sdk/rust/talon-client/Cargo.toml": r'(^version\s*=\s*")[^"]+("\s*$)',
    "sdk/rust/talon-server/Cargo.toml": r'(^version\s*=\s*")[^"]+("\s*$)',
    "sdk/python/talon-client/pyproject.toml": r'(^version\s*=\s*")[^"]+("\s*$)',
    "sdk/python/talon-server/pyproject.toml": r'(^version\s*=\s*")[^"]+("\s*$)',
    "sdk/java/build.gradle.kts": r'(^\s*version\s*=\s*")[^"]+("\s*$)',
}

PACKAGE_JSON_FILES = [
    "sdk/js/talon-client/package.json",
    "sdk/js/talon-server/package.json",
    "sdk/js/talon-node-darwin-arm64/package.json",
    "sdk/js/talon-node-linux-x64/package.json",
    "packages/talon-chat/package.json",
]

WORKSPACE_REFERENCE_FILES = [
    "sdk/js/talon-server/package.json",
    "packages/talon-chat/package.json",
    "ui/package.json",
]


def parse_core_version(value: str) -> tuple[int, int, int]:
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", value)
    if not match:
        raise ValueError(f"unsupported version {value!r}; expected MAJOR.MINOR.PATCH")
    return tuple(int(part) for part in match.groups())  # type: ignore[return-value]


def replace_once(path: str, pattern: str, replacement: str, *, flags: int = re.MULTILINE) -> None:
    file = ROOT / path
    text = file.read_text(encoding="utf-8")
    updated, count = re.subn(pattern, replacement, text, count=1, flags=flags)
    if count != 1:
        raise RuntimeError(f"expected one version match in {path}")
    file.write_text(updated, encoding="utf-8")


def require_matches(path: str, pattern: str, expected: int | None = 1) -> int:
    text = (ROOT / path).read_text(encoding="utf-8")
    count = len(re.findall(pattern, text, flags=re.MULTILINE))
    if expected is not None and count != expected:
        raise RuntimeError(f"expected {expected} matches in {path}, found {count}")
    if expected is None and count == 0:
        raise RuntimeError(f"expected at least one match in {path}")
    return count


def validate_inputs(old_version: str) -> None:
    for path in VERSION_FILES:
        require_matches(path, r".+", expected=1)
    for path, pattern in MANIFEST_VERSION_PATTERNS.items():
        require_matches(path, pattern)
    for path in PACKAGE_JSON_FILES:
        require_matches(path, r'(^\s*"version"\s*:\s*")[^"]+("\s*,?$)')

    require_matches("sdk/js/talon-server/package.json", rf"workspace:{re.escape(old_version)}", expected=2)
    require_matches("packages/talon-chat/package.json", rf"workspace:{re.escape(old_version)}")
    require_matches(
        "packages/talon-chat/package.json",
        r'("@impalasys/talon-client"\s*:\s*"\^)[^"]+("\s*,?$)',
    )
    require_matches("ui/package.json", rf"workspace:{re.escape(old_version)}")
    require_matches("pnpm-lock.yaml", rf"workspace:{re.escape(old_version)}", expected=None)
    require_matches(
        "Cargo.lock",
        r'(\[\[package\]\]\nname = "talon-client"\nversion = ")[^"]+("\n)',
    )
    for package_name in ("talon-client", "talon-server"):
        require_matches(
            "sdk/rust/Cargo.lock",
            rf'(\[\[package\]\]\nname = "{re.escape(package_name)}"\nversion = ")[^"]+("\n)',
        )


def update_version_files(version: str) -> None:
    for path in VERSION_FILES:
        (ROOT / path).write_text(f"{version}\n", encoding="utf-8")

    for path, pattern in MANIFEST_VERSION_PATTERNS.items():
        replace_once(path, pattern, rf"\g<1>{version}\g<2>")

    for path in PACKAGE_JSON_FILES:
        replace_once(
            path,
            r'(^\s*"version"\s*:\s*")[^"]+("\s*,?$)',
            rf"\g<1>{version}\g<2>",
        )


def update_workspace_references(old_version: str, version: str) -> None:
    for path in WORKSPACE_REFERENCE_FILES:
        file = ROOT / path
        text = file.read_text(encoding="utf-8")
        updated, count = re.subn(
            rf"workspace:{re.escape(old_version)}",
            f"workspace:{version}",
            text,
        )
        if path == "packages/talon-chat/package.json":
            updated, peer_count = re.subn(
                r'("@impalasys/talon-client"\s*:\s*"\^)[^"]+("\s*,?$)',
                rf"\g<1>{version}\g<2>",
                updated,
                count=1,
                flags=re.MULTILINE,
            )
            if peer_count != 1:
                raise RuntimeError("expected talon-chat SDK peer dependency")
        if count == 0 and path != "packages/talon-chat/package.json":
            raise RuntimeError(f"expected workspace reference in {path}")
        file.write_text(updated, encoding="utf-8")


def update_pnpm_lock(old_version: str, version: str) -> None:
    path = ROOT / "pnpm-lock.yaml"
    text = path.read_text(encoding="utf-8")
    updated, count = re.subn(
        rf"workspace:{re.escape(old_version)}",
        f"workspace:{version}",
        text,
    )
    if count == 0:
        raise RuntimeError("expected workspace version references in pnpm-lock.yaml")
    path.write_text(updated, encoding="utf-8")


def update_cargo_lock(path: str, version: str, package_names: tuple[str, ...]) -> None:
    file = ROOT / path
    text = file.read_text(encoding="utf-8")
    updated = text
    for package_name in package_names:
        pattern = rf'(\[\[package\]\]\nname = "{re.escape(package_name)}"\nversion = ")[^"]+("\n)'
        updated, count = re.subn(pattern, rf"\g<1>{version}\g<2>", updated, count=1)
        if count != 1:
            raise RuntimeError(f"expected {package_name} in {path}")
    file.write_text(updated, encoding="utf-8")


def bump(version: str) -> None:
    target = parse_core_version(version)
    old_version = VERSION_FILE.read_text(encoding="utf-8").strip()
    current = parse_core_version(old_version)
    if target <= current:
        raise ValueError(f"target version {version} must be greater than current version {old_version}")

    validate_inputs(old_version)
    update_version_files(version)
    update_workspace_references(old_version, version)
    update_pnpm_lock(old_version, version)
    update_cargo_lock("Cargo.lock", version, ("talon-client",))
    update_cargo_lock("sdk/rust/Cargo.lock", version, ("talon-client", "talon-server"))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="new SDK version in MAJOR.MINOR.PATCH form")
    args = parser.parse_args()
    try:
        bump(args.version)
    except (OSError, RuntimeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"bumped public SDK artifacts to {args.version}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
