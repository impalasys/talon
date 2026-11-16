#!/usr/bin/env python3
"""Run the complete Linux Bazel job, including enabled Docker sandbox tests."""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import uuid

import linux_pilot as pilot

WORKLOADS = [
    ("test", ["test", "--build_tests_only", "//...", "--nocache_test_results",
              "--test_output=errors", "--test_env=TALON_DOCKER_SANDBOX_TEST=1",
              "--test_env=DOCKER_HOST=unix:///var/run/docker.sock"]),
    ("build", ["build", "//:binaries", "//:production_binaries", "//sdk/...",
               "//tools/install-hooks/...", "//ui:dist", "//docs-site:dist",
               "//packages/talon-chat/..."]),
]


def summaries(path):
    rows = []
    if path.exists():
        for line in path.read_text().splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if "testSummary" in event:
                summary = event["testSummary"]
                rows.append({"label": event["id"]["testSummary"]["label"],
                             **{k: summary[k] for k in ("overallStatus", "totalRunCount",
                                                       "totalNumCached") if k in summary}})
    return rows


def run(output):
    output.mkdir(parents=True, exist_ok=True)
    affinity = pilot.limit_cpu_affinity(16)
    options = [s.replace("--jobs=4", "--jobs=16")
               .replace("--local_resources=cpu=4", "--local_resources=cpu=16")
               .replace("--local_resources=memory=12288", "--local_resources=memory=20480")
               for s in pilot.OPTIONS]
    result = {"schema": 1, "commit": pilot.capture("git", "rev-parse", "HEAD"),
              "requested_cpu": 16, "requested_memory_gb": 32,
              "affinity_cpus": affinity, "runner_memory": pilot.memory_metadata(),
              "docker_version": pilot.capture("docker", "version", "--format", "{{.Server.Version}}"),
              "bazel_version": Path(".bazelversion").read_text().strip(),
              "os": pilot.platform.freedesktop_os_release()["PRETTY_NAME"],
              "compiler": pilot.capture("/usr/bin/clang-18", "--version"),
              "native_packages": pilot.capture("dpkg-query", "-W", "-f=${Package}=${Version}\n", *pilot.PACKAGES),
              "options": options, "workloads": WORKLOADS, "samples": [], "success": False}
    started = time.time()
    try:
        with tempfile.TemporaryDirectory(prefix="talon-full-bep-") as temporary:
            for phase, workload in WORKLOADS:
                event_file = Path(temporary) / (phase + ".jsonl")
                invocation = str(uuid.uuid4())
                command = ["bazel", workload[0], *options, *workload[1:],
                           f"--profile={output / (phase + '.profile.gz')}",
                           f"--build_event_json_file={event_file}", f"--invocation_id={invocation}"]
                before = time.monotonic()
                with (output / (phase + ".log")).open("w") as log:
                    process = subprocess.Popen(command, stdout=subprocess.PIPE,
                                               stderr=subprocess.STDOUT, text=True)
                    for line in process.stdout:
                        print(line, end="", flush=True)
                        log.write(line)
                    code = process.wait()
                    process.stdout.close()
                row = {"phase": phase, "exit_code": code, "seconds": time.monotonic() - before,
                       "invocation_url": f"https://app.buildbuddy.io/invocation/{invocation}",
                       "metrics": pilot.read_metrics(event_file)}
                if phase == "test":
                    row["tests"] = summaries(event_file)
                result["samples"].append(row)
                if phase == "test" and code == 0:
                    tests = row["tests"]
                    if (not any(t["label"] == "//:talon_test" for t in tests) or
                            any(t.get("overallStatus") != "PASSED" or
                                int(t.get("totalNumCached", 0)) for t in tests)):
                        raise RuntimeError("Full validation requires uncached passing tests, including //:talon_test")
                if code:
                    return code
        result["success"] = True
        return 0
    finally:
        result["seconds"] = time.time() - started
        for name in ("test.xml", "test.log"):
            for path in Path("bazel-testlogs").rglob(name):
                target = output / "testlogs" / path.relative_to("bazel-testlogs")
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, target)
        (output / "full-job.json").write_text(json.dumps(result, indent=2) + "\n")
        print("FULL_LINUX_RESULT " + json.dumps({k: result[k] for k in
                                                ("commit", "success", "seconds")}), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    raise SystemExit(run(parser.parse_args().output.resolve()))
