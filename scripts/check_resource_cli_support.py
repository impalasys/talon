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
EXPECTED_LOOKUP_NAMESPACES = {
    "agent",
    "default",
    "none",
    "required",
    "system",
    "system_fixed",
}
EXPECTED_LIST_NAMESPACES = {"default", "system"}
EXPECTED_NAME_POLICIES = {"agent", "channel_subscription", "plain"}
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


def protobuf_resource_kinds(source: str) -> tuple[set[str], set[str]]:
    envelope_kinds: dict[str, set[str]] = {}
    for message in ("ResourceSpec", "ResourceStatus"):
        kinds: set[str] = set()
        body = _block(source, rf"message\s+{message}\s*\{{")
        oneof = _block(body, r"oneof\s+kind\s*\{")
        for field in re.finditer(
            r"^\s*[A-Za-z_][A-Za-z0-9_.]*\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*\d+\s*;",
            oneof,
            re.MULTILINE,
        ):
            kinds.add(_pascal_case(field.group(1)))
        envelope_kinds[message] = kinds - IGNORED_KINDS
    return envelope_kinds["ResourceSpec"], envelope_kinds["ResourceStatus"]


def _validate_enum(
    errors: list[str], kind: str, field: str, value: object, expected: set[str]
) -> None:
    if not isinstance(value, str) or value not in expected:
        errors.append(
            f"{kind}: {field} must be one of {', '.join(sorted(expected))}; got {value!r}"
        )


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

        _validate_enum(errors, kind, "apply_route", entry["apply_route"], EXPECTED_ROUTES)
        _validate_enum(
            errors,
            kind,
            "lookup_namespace",
            entry["lookup_namespace"],
            EXPECTED_LOOKUP_NAMESPACES,
        )
        _validate_enum(
            errors,
            kind,
            "list_namespace",
            entry["list_namespace"],
            EXPECTED_LIST_NAMESPACES,
        )
        _validate_enum(
            errors, kind, "name_policy", entry["name_policy"], EXPECTED_NAME_POLICIES
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
        spec_kinds, status_kinds = protobuf_resource_kinds(proto_text)
    except ValueError as error:
        return [f"protobuf resource envelope: {error}"]
    try:
        entries, errors = registry_kinds(registry_text)
    except Exception as error:  # TOML parser errors should be actionable in CI.
        return [f"resource registry is invalid: {error}"]

    if spec_kinds != status_kinds:
        for kind in sorted(spec_kinds - status_kinds):
            errors.append(f"{kind}: ResourceSpec arm is missing from ResourceStatus")
        for kind in sorted(status_kinds - spec_kinds):
            errors.append(f"{kind}: ResourceStatus arm is missing from ResourceSpec")

    registered = set(entries)
    for envelope, proto_kinds in (
        ("ResourceSpec", spec_kinds),
        ("ResourceStatus", status_kinds),
    ):
        for kind in sorted(proto_kinds - registered):
            errors.append(f"{kind}: missing from proto/resources.toml ({envelope})")
    for kind in sorted(registered - (spec_kinds | status_kinds)):
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
