#!/usr/bin/env python3
"""Select affected Bazel targets without treating query failures as empty results."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile


class SelectionError(RuntimeError):
    pass


def label_set(labels: list[str]) -> str:
    return "set(" + " ".join(json.dumps(label) for label in labels) + ")"


def bazel_output(bazel: str, *arguments: str) -> str:
    command = [*shlex.split(bazel), *arguments]
    # Linux limits each argv string. Large monorepo cquery sets use a file.
    with tempfile.TemporaryDirectory(prefix="talon-query-") as temporary:
        if len(arguments) > 1 and len(arguments[1]) > 30000:
            query = Path(temporary) / "query"
            query.write_text(arguments[1])
            command = [*shlex.split(bazel), arguments[0], "--query_file=" + str(query), *arguments[2:]]
        result = subprocess.run(command, text=True, stdout=subprocess.PIPE, check=False)
    if result.returncode:
        raise SelectionError(f"Bazel {arguments[0]} failed ({result.returncode}); refusing to skip CI.")
    return result.stdout


def normalize_label(label: str) -> str:
    if label.startswith("@@//"):
        return label[2:]
    return label


def output_labels(output: str) -> list[str]:
    labels = [normalize_label(line) for line in output.splitlines() if line]
    if any(not label.startswith("//") or ":" not in label for label in labels):
        raise SelectionError("Bazel returned malformed target labels; refusing to skip CI.")
    return sorted(set(labels))


def workspace_graph(bazel: str) -> tuple[dict[str, dict], set[str]]:
    output = bazel_output(bazel, "query", "//...:*", "--output=streamed_jsonproto", "--order_output=no")
    rules: dict[str, dict] = {}
    sources: set[str] = set()
    try:
        for line in output.splitlines():
            record = json.loads(line)
            kind = record["type"]
            if kind == "RULE":
                rule = record["rule"]
                if rule["name"] in rules:
                    raise ValueError("duplicate rule")
                for field in ["ruleInput", "ruleOutput"]:
                    labels = rule.get(field, [])
                    if not isinstance(labels, list) or any(not isinstance(label, str) for label in labels):
                        raise ValueError("invalid dependency labels")
                rules[rule["name"]] = rule
            elif kind == "SOURCE_FILE":
                sources.add(record["sourceFile"]["name"])
            elif kind not in {"GENERATED_FILE", "PACKAGE_GROUP", "ENVIRONMENT_GROUP"}:
                raise ValueError("unknown target type")
        if not rules:
            raise ValueError("empty rule graph")
        if any(not name.startswith("//") or ":" not in name for name in rules.keys() | sources):
            raise ValueError("invalid workspace label")
    except (KeyError, TypeError, ValueError) as error:
        raise SelectionError("Malformed Bazel workspace graph; refusing to skip CI.") from error
    return rules, sources


def ordinary_targets(rules: dict[str, dict]) -> list[str]:
    return sorted(
        name for name, rule in rules.items()
        if not any(
            attribute.get("name") == "tags" and "manual" in attribute.get("stringListValue", [])
            for attribute in rule.get("attribute", [])
        )
    )


def workspace_dependents(rules: dict[str, dict], seeds: list[str]) -> set[str]:
    # Reporting may request targets for a different host. The workspace query
    # includes every select() branch without loading external dependencies such
    # as Swift packages that need their native host just to load metadata.
    parents: dict[str, set[str]] = {}
    for name, rule in rules.items():
        for dependency in rule.get("ruleInput", []):
            dependency = normalize_label(dependency)
            if dependency.startswith("//"):
                parents.setdefault(dependency, set()).add(name)
        # Consumers can depend on a generated file instead of its generating rule.
        parents.setdefault(name, set()).update(normalize_label(label) for label in rule.get("ruleOutput", []))
    affected: set[str] = set()
    pending = list(seeds)
    while pending:
        label = pending.pop()
        if label not in affected:
            affected.add(label)
            pending.extend(parents.get(label, ()))
    return affected


def configured_rule_labels(configured: str) -> list[str]:
    try:
        graph = json.loads(configured)
        # Protobuf JSON omits the repeated results field for an empty cquery.
        if graph == {}:
            return []
        results = graph["results"]
        if not isinstance(results, list):
            raise ValueError("invalid configured results")
        roots = []
        for result in results:
            target = result["target"]
            if target["type"] != "RULE":
                raise ValueError("unexpected configured target type")
            roots.append(target["rule"]["name"])
        return output_labels("\n".join(roots))
    except (KeyError, TypeError, ValueError) as error:
        raise SelectionError("Malformed configured target graph; refusing to skip CI.") from error


def compatible_targets(bazel: str, targets: list[str], platform: str | None) -> list[str]:
    if not targets or not platform:
        return targets
    # Enumerate roots before cquery: an expression such as '//... except manual'
    # still analyzes the manual targets before applying the query's subtraction.
    arguments = ("cquery", label_set(targets), f"--platforms={platform}", "--skip_incompatible_explicit_targets")
    # JSON retains the requested alias name. Starlark target.label instead
    # resolves compatible aliases to their actual target, possibly in an
    # external repository. It also omits unconfigured test_suite collections.
    configured_labels = configured_rule_labels(bazel_output(bazel, *arguments, "--output=jsonproto"))
    # Incompatible aliases retain their original label, so this provider query
    # can safely subtract them. The second analysis uses the same cached graph.
    incompatible = output_labels(bazel_output(
        bazel, *arguments, "--output=starlark",
        '--starlark:expr=str(target.label) if "IncompatiblePlatformProvider" in providers(target) else ""',
    ))
    return sorted(set(configured_labels) - set(incompatible))


def global_change(path: str) -> bool:
    name = Path(path).name
    return (
        name in {"BUILD", "BUILD.bazel", "MODULE.bazel", "MODULE.bazel.lock", "WORKSPACE", "WORKSPACE.bazel",
                 ".bazelversion", ".bazelignore", "buildbuddy.yaml", "pnpm-lock.yaml", "pnpm-workspace.yaml", "package.json",
                 "Cargo.toml", "Cargo.lock", "requirements.lock", "requirements.in", "requirements.txt", "maven_install.json",
                 "go.work", "go.work.sum", "go.mod", "go.sum"}
        or name.startswith(".bazelrc") or path.endswith(".bzl") or path.startswith(("scripts/ci/", ".github/"))
    )


def source_label(root: Path, path: str) -> str | None:
    source = root / path
    parent = source.parent
    while parent == root or root in parent.parents:
        if (parent / "BUILD").is_file() or (parent / "BUILD.bazel").is_file():
            package = parent.relative_to(root).as_posix()
            return f"//{'' if package == '.' else package}:{source.relative_to(parent).as_posix()}"
        parent = parent.parent
    return None


def select_targets(
    root: Path, bazel: str, changed: list[str], targets: list[str], platform: str | None, full: bool,
) -> list[str]:
    if not full and not changed:
        return []
    rules, sources = workspace_graph(bazel)
    candidates = sorted(set(targets)) if targets else ordinary_targets(rules)
    unknown = set(candidates) - rules.keys()
    if unknown:
        raise SelectionError(f"Requested targets missing from the workspace: {', '.join(sorted(unknown))}")
    # Deletions and package/configuration edits can remove the old dependency
    # edges. Use the complete universe rather than querying an incomplete graph.
    broad = full or any(global_change(path) or not (root / path).exists() for path in changed)
    seeds = sorted({label for path in changed if (label := source_label(root, path)) in sources})
    if not broad and not seeds:
        return []
    candidates = compatible_targets(bazel, candidates, platform)
    if not candidates or broad:
        return candidates
    if platform:
        # Follow configured edges throughout selection. An unconfigured query
        # traverses select() branches for other hosts and fetches their SDKs.
        universe = label_set(candidates)
        expression = f"{universe} intersect rdeps(deps({universe}), {label_set(seeds)})"
        return configured_rule_labels(bazel_output(
            bazel, "cquery", expression, f"--platforms={platform}", "--output=jsonproto",
        ))
    return sorted(set(candidates) & workspace_dependents(rules, seeds))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bazel", default=os.environ.get("BAZEL", "bazel"))
    parser.add_argument("--platform")
    parser.add_argument("--full", action="store_true")
    parser.add_argument("--changed-file", action="append", default=[])
    parser.add_argument("--targets", nargs="*", default=[])
    arguments = parser.parse_args()
    try:
        selected = select_targets(Path.cwd().resolve(), arguments.bazel, arguments.changed_file,
                                  arguments.targets, arguments.platform, arguments.full)
        for target in selected:
            print(target)
    except (OSError, SelectionError) as error:
        print(f"CI selection failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
