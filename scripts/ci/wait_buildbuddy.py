#!/usr/bin/env python3
"""Keep GitHub's Bazel gate tied to a successful BuildBuddy build of this PR."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import time
from urllib.parse import urlparse

CONTEXT = "BuildBuddy Linux build/test"
PUBLISHER = "buildbuddy-io[bot]"


def api(endpoint, paginate=False):
    command = ["gh", "api", endpoint]
    if paginate:
        command.extend(["--paginate", "--slurp"])
    return json.loads(subprocess.check_output(command, text=True))


def revision(event, repository, merge_sha):
    pr = event["pull_request"]
    if pr["head"]["repo"]["full_name"] != repository:
        raise SystemExit("Fork PRs must use the GitHub Bazel fallback.")
    sha = pr["head"]["sha"]
    if not re.fullmatch(r"[0-9a-f]{40}", sha) or not re.fullmatch(r"[0-9a-f]{40}", merge_sha):
        raise SystemExit("Expected immutable commit SHAs.")
    current = api(f"repos/{repository}/pulls/{pr['number']}")
    if current["head"]["sha"] != sha or current["base"]["sha"] != pr["base"]["sha"]:
        raise SystemExit("PR head/base changed; update the branch and rerun CI.")
    head_tree = api(f"repos/{repository}/git/commits/{sha}")["tree"]["sha"]
    merge_tree = api(f"repos/{repository}/git/commits/{merge_sha}")["tree"]["sha"]
    if head_tree != merge_tree:
        raise SystemExit("BuildBuddy head differs from the GitHub merge tree; update the PR branch.")
    return sha


def latest_status(repository, sha):
    pages = api(f"repos/{repository}/commits/{sha}/statuses?per_page=100", paginate=True)
    # GitHub orders statuses newest first. Never fall back to an older success.
    status = next((s for page in pages for s in page if s["context"] == CONTEXT), None)
    if status is not None:
        url = urlparse(status.get("target_url") or "")
        if (status.get("creator", {}).get("login") != PUBLISHER or url.scheme != "https" or
                url.hostname != "impalasys.buildbuddy.io" or
                not re.fullmatch(r"/invocation/[0-9a-f-]{36}", url.path)):
            raise SystemExit("Unexpected publisher or invocation URL for the BuildBuddy status.")
    return status


def wait(event, repository, merge_sha, timeout=4200):
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise SystemExit("Invalid GitHub repository.")
    sha = revision(event, repository, merge_sha)
    deadline = time.monotonic() + timeout
    last = None
    while True:
        status = latest_status(repository, sha)
        state = status["state"] if status else "missing"
        if state != last:
            print(f"{CONTEXT} for {sha}: {state}", flush=True)
            last = state
        if state == "success":
            revision(event, repository, merge_sha)
            print(status["target_url"], flush=True)
            summary = os.environ.get("GITHUB_STEP_SUMMARY")
            if summary:
                with Path(summary).open("a") as out:
                    out.write(f"Bazel passed on [BuildBuddy]({status['target_url']}) for `{sha}`.\n")
            return 0
        if state in ("failure", "error"):
            raise SystemExit(f"BuildBuddy failed: {status['target_url']}")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise SystemExit(f"Timed out waiting for {CONTEXT} on {sha} ({state}).")
        time.sleep(min(15, remaining))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--timeout-seconds", type=int, default=4200)
    args = parser.parse_args()
    if args.timeout_seconds < 0:
        parser.error("timeout must be nonnegative")
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    raise SystemExit(wait(event, os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_SHA"], args.timeout_seconds))
