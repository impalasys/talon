#!/usr/bin/env python3
"""Reject oneof fields in concrete resource-facing protos.

Resource specs and statuses are authored directly through generated SDKs.
Avoiding oneof in those schemas keeps SDK construction ergonomic across
languages. The generic ResourceSpec/ResourceStatus envelope is an existing
storage/RPC wrapper and is intentionally exempted here.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCANNED_PATHS = [
    *sorted((ROOT / "proto/resources").glob("*.proto")),
    ROOT / "proto/data/routing.proto",
]
ALLOWED_FILES = {
    ROOT / "proto/resources/resource.proto",
}
ONEOF_RE = re.compile(r"^\s*oneof\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{")


def strip_comments(line: str, in_block_comment: bool) -> tuple[str, bool]:
    output = []
    index = 0
    while index < len(line):
        if in_block_comment:
            end = line.find("*/", index)
            if end == -1:
                return "".join(output), True
            index = end + 2
            in_block_comment = False
            continue

        line_comment = line.find("//", index)
        block_comment = line.find("/*", index)
        if line_comment != -1 and (block_comment == -1 or line_comment < block_comment):
            output.append(line[index:line_comment])
            return "".join(output), False
        if block_comment != -1:
            output.append(line[index:block_comment])
            index = block_comment + 2
            in_block_comment = True
            continue
        output.append(line[index:])
        return "".join(output), False

    return "".join(output), in_block_comment


def main() -> int:
    violations: list[tuple[Path, int, str]] = []
    for path in SCANNED_PATHS:
        if path in ALLOWED_FILES:
            continue
        in_block_comment = False
        for line_number, raw_line in enumerate(path.read_text().splitlines(), start=1):
            line, in_block_comment = strip_comments(raw_line, in_block_comment)
            match = ONEOF_RE.match(line)
            if match:
                violations.append((path.relative_to(ROOT), line_number, match.group(1)))

    if violations:
        print("oneof is not allowed in concrete resource-facing protos.", file=sys.stderr)
        print(
            "Use explicit kind/tag fields plus optional payload fields for SDK ergonomics.",
            file=sys.stderr,
        )
        for path, line_number, name in violations:
            print(f"{path}:{line_number}: oneof {name}", file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
