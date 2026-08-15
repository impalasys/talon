#!/usr/bin/env python3
"""Check that every resource envelope arm has CLI capability metadata."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python 3.11+ is used in CI.
    import tomli as tomllib  # type: ignore[no-redef]


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PROTO = ROOT / "proto/resources/resource.proto"
DEFAULT_REGISTRY = ROOT / "proto/resources.toml"
EXPECTED_ROUTES = {"generic", "legacy", "namespace", "internal"}
IGNORED_KINDS = {"Raw"}


def _block(source: str, pattern: str) -> str:
    match = re.search(pattern, source)
    if not match:
        raise ValueError(f"could not find {pattern!r}")
    opening = source.find("{", match.start())
    depth = 0
    for index in range(opening, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[opening + 1 : index]
    raise ValueError(f"unterminated block for {pattern!r}")


def _pascal_case(value: str) -> str:
    return "".join(part[:1].upper() + part[1:] for part in value.split("_"))


def protobuf_resource_kinds(source: str) -> set[str]:
    kinds: set[str] = set()
    for message in ("ResourceSpec", "ResourceStatus"):
        body = _block(source, rf"message\s+{message}\s*\{{")
        oneof = _block(body, r"oneof\s+kind\s*\{")
        for field in re.finditer(
            r"^\s*[A-Za-z_][A-Za-z0-9_.]*\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*\d+\s*;",
            oneof,
            re.MULTILINE,
        ):
            kinds.add(_pascal_case(field.group(1)))
    return kinds - IGNORED_KINDS


def registry_kinds(registry_text: str) -> tuple[dict[str, dict], list[str]]:
    data = tomllib.loads(registry_text)
    resources = data.get("resource")
    if not isinstance(resources, list):
        return {}, ["registry must define one or more [[resource]] entries"]

    entries: dict[str, dict] = {}
    errors: list[str] = []
    aliases: dict[str, str] = {}
    required = {
        "kind",
        "aliases",
        "apply_route",
        "lookup_namespace",
        "list_namespace",
        "name_policy",
        "user_authorable",
        "cli_lookup",
        "cli_list",
    }
    for index, entry in enumerate(resources, start=1):
        if not isinstance(entry, dict):
            errors.append(f"registry resource entry {index} must be a table")
            continue
        missing = sorted(required - entry.keys())
        if missing:
            errors.append(
                f"registry resource entry {index} is missing: {', '.join(missing)}"
            )
            continue
        kind = entry["kind"]
        if not isinstance(kind, str) or not kind:
            errors.append(f"registry resource entry {index} has an invalid kind")
            continue
        if kind in entries:
            errors.append(f"duplicate registry kind: {kind}")
        entries[kind] = entry

        if entry["apply_route"] not in EXPECTED_ROUTES:
            errors.append(
                f"{kind}: unsupported apply_route {entry['apply_route']!r}"
            )
        if entry["user_authorable"] and entry["apply_route"] == "internal":
            errors.append(f"{kind}: user_authorable resources cannot be internal")
        if not isinstance(entry["aliases"], list):
            errors.append(f"{kind}: aliases must be an array")
            continue
        for alias in [kind, *entry["aliases"]]:
            if not isinstance(alias, str) or not alias:
                errors.append(f"{kind}: aliases must contain non-empty strings")
                continue
            normalized = alias.casefold()
            previous = aliases.get(normalized)
            if previous and previous != kind:
                errors.append(f"alias {alias!r} is claimed by both {previous} and {kind}")
            aliases[normalized] = kind
    return entries, errors


def validate(proto_text: str, registry_text: str) -> list[str]:
    try:
        proto_kinds = protobuf_resource_kinds(proto_text)
    except ValueError as error:
        return [f"protobuf resource envelope: {error}"]
    try:
        entries, errors = registry_kinds(registry_text)
    except Exception as error:  # TOML parser errors should be actionable in CI.
        return [f"resource registry is invalid: {error}"]

    registered = set(entries)
    for kind in sorted(proto_kinds - registered):
        errors.append(f"{kind}: missing from proto/resources.toml")
    for kind in sorted(registered - proto_kinds):
        errors.append(f"{kind}: registry entry has no ResourceSpec/ResourceStatus arm")
    for kind, entry in sorted(entries.items()):
        if entry.get("user_authorable") and entry.get("apply_route") not in EXPECTED_ROUTES:
            errors.append(f"{kind}: user-authorable resource has no valid apply route")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--proto", type=Path, default=DEFAULT_PROTO)
    parser.add_argument("--registry", type=Path, default=DEFAULT_REGISTRY)
    args = parser.parse_args()

    errors = validate(args.proto.read_text(), args.registry.read_text())
    if errors:
        print("Resource CLI support check failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1
    print("Resource CLI support metadata matches the protobuf resource envelope.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
