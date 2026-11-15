#!/usr/bin/env python3
"""Run identical Linux workloads and compare exported provider measurements."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
import time
import uuid


PACKAGES = [line.split("=", 1)[0] for line in
            Path("scripts/ci/linux-pilot-packages.txt").read_text().splitlines()
            if line and not line.startswith("#")]
WORKLOADS = (
    ("build", ["build", "//:production_binaries", "//ui:dist"]),
    ("test", ["test", "//ui:unit_tests"]),
)
STARTUP_OPTIONS = ["--host_jvm_args=-Xmx4g"]
OPTIONS = [
    "--config=buildbuddy", "--remote_executor=",
    "--remote_upload_local_results=false", "--remote_accept_cached=true",
    "--remote_download_outputs=all", "--jobs=4", "--local_resources=cpu=4",
    "--local_resources=memory=12288", "--color=no", "--curses=no",
    "--repo_env=CC=/usr/bin/clang-18", "--repo_env=CXX=/usr/bin/clang++-18",
    "--action_env=CC=/usr/bin/clang-18", "--action_env=CXX=/usr/bin/clang++-18",
    "--action_env=LIBCLANG_PATH=/usr/lib/llvm-18/lib",
]


def capture(*command):
    return subprocess.check_output(command, text=True).strip()


def read_metrics(path):
    """Extract numeric metrics only: raw BEP command lines can contain keys."""
    metrics = {}
    if path.exists():
        for line in path.read_text().splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                # A killed Bazel process may leave a partial last event.
                continue
            if "buildMetrics" in event:
                metrics = numeric_metrics(event["buildMetrics"])
    return metrics


def numeric_metrics(value):
    if isinstance(value, dict):
        return {k: numeric_metrics(v) for k, v in value.items()
                if isinstance(v, (dict, int, float)) or
                (isinstance(v, str) and v.isdigit())}
    return value


def limit_cpu_affinity():
    """Cap all compiler descendants, including an already-running Bazel JVM."""
    cpus = sorted(os.sched_getaffinity(0))[:4]
    if len(cpus) != 4:
        raise SystemExit(f"Pilot needs four available CPUs; found {len(cpus)}")
    os.sched_setaffinity(0, cpus)
    # A recycled VM's JVM predates this process, so it does not inherit our
    # mask. Bind its existing threads too, without a warming bazel info call.
    processes = {}
    targets = set()
    for process in Path("/proc").glob("[0-9]*"):
        try:
            argv = (process / "cmdline").read_bytes().split(b"\0")
            parent = next(line for line in (process / "status").read_text().splitlines()
                          if line.startswith("PPid:"))
            processes[int(process.name)] = (process, int(parent.split()[1]))
            if argv[0].startswith(b"bazel(") or any(arg.endswith(b"/A-server.jar") for arg in argv):
                targets.add(int(process.name))
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            continue
    # Retained persistent compiler workers also predate the new Python process.
    while True:
        descendants = {pid for pid, (_, parent) in processes.items() if parent in targets}
        if descendants <= targets:
            break
        targets |= descendants
    for pid in targets:
        process = processes[pid][0]
        try:
            for thread in (process / "task").iterdir():
                os.sched_setaffinity(int(thread.name), cpus)
        except (FileNotFoundError, ProcessLookupError):
            continue
    return cpus


def memory_metadata():
    path = Path("/proc/meminfo")
    if not path.exists():
        return {}
    total = next(line for line in path.read_text().splitlines() if line.startswith("MemTotal:"))
    return {"total_kib": int(total.split()[1])}


def run(args):
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise SystemExit("Run the pilot on Ubuntu 24.04 x86_64.")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    affinity_cpus = limit_cpu_affinity()
    started = time.time()
    configuration = {"workloads": WORKLOADS, "options": OPTIONS, "startup_options": STARTUP_OPTIONS,
                     "bazel_version": Path(".bazelversion").read_text().strip(),
                     "compiler": capture("/usr/bin/clang-18", "--version"),
                     "native_packages": capture("dpkg-query", "-W", "-f=${Package}=${Version}\n", *PACKAGES),
                     "os": platform.freedesktop_os_release()["PRETTY_NAME"],
                     "cpu_affinity_count": len(affinity_cpus)}
    result = {
        "schema": 1, "provider": args.provider,
        "commit": capture("git", "rev-parse", "HEAD"),
        "configuration": configuration,
        "configuration_id": hashlib.sha256(json.dumps(configuration, sort_keys=True).encode()).hexdigest(),
        "requested_cpu": 4, "requested_memory_gb": 16,
        "visible_cpu": os.cpu_count(), "affinity_cpus": affinity_cpus,
        "runner_memory": memory_metadata(),
        "runner_kernel": platform.release(), "started_at": started,
        "setup_seconds": max(0, started - float(os.environ.get("PILOT_SETUP_STARTED_AT", started))),
        "samples": [], "success": False,
    }
    result_path = output / "results.json"
    # Do not run bazel info/version/shutdown/clean before the first sample: the
    # retained server and analysis state are exactly what this pilot measures.
    returncode = 0
    with tempfile.TemporaryDirectory(prefix="talon-pilot-bep-") as temporary:
        for iteration in ("initial", "warm"):
            for phase, workload in WORKLOADS:
                name = f"{iteration}-{phase}"
                bep = Path(temporary) / f"{name}.jsonl"
                invocation = str(uuid.uuid4())
                command = ["bazel", workload[0], *OPTIONS, *workload[1:],
                           f"--profile={output / (name + '.profile.gz')}",
                           f"--build_event_json_file={bep}", f"--invocation_id={invocation}"]
                before = time.monotonic()
                with (output / f"{name}.log").open("w") as log:
                    process = subprocess.Popen(command, stdout=subprocess.PIPE,
                                               stderr=subprocess.STDOUT, text=True)
                    for line in process.stdout:
                        print(line, end="", flush=True)
                        log.write(line)
                    returncode = process.wait()
                    process.stdout.close()
                result["samples"].append({
                    "iteration": iteration, "phase": phase,
                    "seconds": time.monotonic() - before, "exit_code": returncode,
                    "invocation_url": f"https://app.buildbuddy.io/invocation/{invocation}",
                    "metrics": read_metrics(bep),
                })
                result["success"] = returncode == 0 and len(result["samples"]) == 2 * len(WORKLOADS)
                result["measured_seconds"] = time.time() - started
                result_path.write_text(json.dumps(result, indent=2) + "\n")
                if returncode:
                    break
            if returncode:
                break
    print(f"Pilot measurements: {result_path}")
    return returncode


def percentile95(values):
    return sorted(values)[math.ceil(len(values) * 0.95) - 1]


def compare(args):
    groups = {}
    for path in args.results:
        result = json.loads(path.read_text())
        if not result["success"]:
            raise SystemExit(f"Cannot compare unsuccessful run: {path}")
        key = (result["commit"], result["configuration_id"])
        groups.setdefault(key, {"github": [], "buildbuddy": []})[result["provider"]].append(result)
    for (commit, _), providers in sorted(groups.items()):
        if any(not runs for runs in providers.values()):
            raise SystemExit(f"Missing matched provider for {commit}: check revision, compiler and configuration.")
    print("Build + test command wall time; excludes checkout, cache restore/save, setup and provider queues.\n")
    print("| Commit | Sample | GitHub median / p95 (s) | BuildBuddy median / p95 (s) | Median reduction | Runs GH / BB |")
    print("| --- | --- | --- | --- | --- | --- |")
    for (commit, _), providers in sorted(groups.items()):
        for iteration in ("initial", "warm"):
            values = {provider: [sum(s["seconds"] for s in run["samples"]
                                    if s["iteration"] == iteration) for run in runs]
                      for provider, runs in providers.items()}
            gh, bb = (statistics.median(values[p]) for p in ("github", "buildbuddy"))
            print(f"| {commit[:12]} | {iteration} | {gh:.2f} / {percentile95(values['github']):.2f} | "
                  f"{bb:.2f} / {percentile95(values['buildbuddy']):.2f} | {100 * (1 - bb / gh):.1f}% | "
                  f"{len(values['github'])} / {len(values['buildbuddy'])} |")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    runner = sub.add_parser("run")
    runner.add_argument("--provider", choices=("github", "buildbuddy"), required=True)
    runner.add_argument("--output", type=Path, required=True)
    comparison = sub.add_parser("compare")
    comparison.add_argument("results", type=Path, nargs="+")
    args = parser.parse_args()
    return run(args) if args.command == "run" else compare(args)


if __name__ == "__main__":
    raise SystemExit(main())
