#!/usr/bin/env python3
"""Paired, cache-controlled production Rust measurements on a 16-CPU Linux VM."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import time
import uuid


REVISIONS = {
    "before": "4728e838ed08636eaec9bcde27b97a22669d589d",
    "after": "3894372ec109169b2c1bd757a74b89865ac49cd4",
}
TARGETS = ["//:talon-cli", "//:talon-server", "//:talon-worker", "//:talon-node"]
EDITS = {
    "cli": ("src/cli/entry.rs", "crates/talon-cli/src/cli/entry.rs"),
    "worker": ("src/worker/runtime.rs", "crates/talon-worker/src/worker/runtime.rs"),
    "shared_models": ("src/control/manifest/mod.rs", "crates/talon-protocol/src/manifest/mod.rs"),
}
UNAFFECTED = {"cli": ["server", "worker", "node"], "worker": ["cli", "server"]}


def capture(*args):
    return subprocess.check_output(args, text=True).strip()


def numeric(value):
    if isinstance(value, dict):
        return {k: numeric(v) for k, v in value.items()
                if isinstance(v, (dict, int, float)) or isinstance(v, str) and v.isdigit()}
    return value


def metrics(path):
    result = {}
    for line in path.read_text().splitlines() if path.exists() else []:
        event = json.loads(line)
        if "buildMetrics" in event:
            result = numeric(event["buildMetrics"])
    return result


def rust_actions(path):
    text = path.read_text() if path.exists() else ""
    decoder = json.JSONDecoder()
    offset = 0
    rows = []
    while offset < len(text):
        while offset < len(text) and text[offset].isspace():
            offset += 1
        if offset == len(text):
            break
        row, offset = decoder.raw_decode(text, offset)
        if row.get("mnemonic") == "Rustc" and not row.get("cacheHit"):
            rows.append(row.get("targetLabel", ""))
    return rows


def capacity():
    result = {"affinity": sorted(os.sched_getaffinity(0)), "visible_cpus": os.cpu_count(),
              "kernel": platform.release(), "os": platform.freedesktop_os_release(),
              "compiler": capture("/usr/bin/clang-18", "--version")}
    for name in ("cpu.max", "memory.max", "cpuset.cpus.effective"):
        path = Path("/sys/fs/cgroup") / name
        if path.exists():
            result[name] = path.read_text().strip()
    result["meminfo"] = Path("/proc/meminfo").read_text().splitlines()[:3]
    result["cpu_model"] = next(line.split(":", 1)[1].strip()
                               for line in Path("/proc/cpuinfo").read_text().splitlines()
                               if line.startswith("model name"))
    packages = [line.split("=", 1)[0] for line in
                Path("scripts/ci/linux-pilot-packages.txt").read_text().splitlines()
                if line and not line.startswith("#")]
    result["packages"] = capture("dpkg-query", "-W", "-f=${Package}=${Version}\n", *packages)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--round", choices=("AB", "BA"), required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise SystemExit("Use Ubuntu 24.04 x86_64 on BuildBuddy.")
    available = sorted(os.sched_getaffinity(0))
    if len(available) < 16:
        raise SystemExit(f"Expected 16 CPUs; found {len(available)}.")
    os.sched_setaffinity(0, available[:16])
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    control = capture("git", "rev-parse", "HEAD")
    order = ["before", "after"] if args.round == "AB" else ["after", "before"]
    report = {"schema": 1, "round": args.round, "order": order, "control_commit": control,
              "revisions": REVISIONS, "capacity": capacity(), "samples": [], "success": False,
              "method": {
                  "target": "Four production Rust binaries, opt + AWS + RocksDB; UI and tests excluded.",
                  "cold": "Fresh output base and JVM; local/disk/remote action caches empty or disabled. Repository downloads and toolchains prefetched outside timing.",
                  "warm": "Three unchanged repeats in the same output base/JVM after cold build.",
                  "incremental": "One unused public constant added to each real CLI, worker and shared-model source; metadata invalidation without runtime logic changes. Cumulative edits avoid untimed restoration rebuilds.",
                  "limits": "16 affinity CPUs, 16 Bazel jobs, 32 GB VM, 20 GiB action budget, 4 GiB JVM; local execution only.",
                  "timing": "Build command wall time includes Bazel startup/analysis/execution and BES finalization; excludes setup, prefetch, hashing, source edits and queues.",
                  "order": "Two separate paired VMs, AB and BA. Report per-pair results and ranges; no p95 estimate.",
              }}
    result_path = output / f"crate-split-{args.round}.json"
    nonce = uuid.uuid4().hex
    # Repository contents caches contain read-only trees. Cleanup of this
    # disposable VM's scratch files must not abort an otherwise valid sample.
    with tempfile.TemporaryDirectory(prefix="talon-crate-split-", ignore_cleanup_errors=True) as directory:
        work = Path(directory)
        shared = [f"--repository_cache={work / 'downloads'}",
                  f"--repo_contents_cache={work / 'repositories'}"]
        options = ["--config=buildbuddy", "--remote_executor=", "--remote_cache=", "--disk_cache=",
                   "--remote_accept_cached=false", "--remote_upload_local_results=false", "--nostamp",
                   "--jobs=16", "--local_resources=cpu=16", "--local_resources=memory=20480",
                   "--compilation_mode=opt", "--//:with_aws=true", "--//:with_rocksdb=true",
                   "--platforms=//:linux_ci", "--color=no", "--curses=no",
                   "--repo_env=CC=/usr/bin/clang-18", "--repo_env=CXX=/usr/bin/clang++-18",
                   "--action_env=CC=/usr/bin/clang-18", "--action_env=CXX=/usr/bin/clang++-18",
                   "--action_env=LIBCLANG_PATH=/usr/lib/llvm-18/lib",
                   "--build_metadata=DISABLE_COMMIT_STATUS_REPORTING=true", *shared]
        report["options"] = options

        def command(base, verb, variant, *extra):
            return ["bazel", f"--output_base={base}", "--host_jvm_args=-Xmx4g",
                    "--host_jvm_args=-XX:ActiveProcessorCount=16", "--max_idle_secs=120", verb,
                    *([] if verb == "shutdown" else options),
                    *([] if verb == "shutdown" else [f"--build_metadata=COMMIT_SHA={REVISIONS[variant]}",
                                                     f"--build_metadata=CRATE_SPLIT_ROUND={args.round}"]),
                    *extra]

        def checkout(variant):
            subprocess.run(["git", "checkout", "--detach", "--force", REVISIONS[variant]], check=True)
            assert capture("git", "rev-parse", "HEAD") == REVISIONS[variant]

        def hashes():
            return {name: hashlib.sha256(Path("bazel-bin", "talon-" + name).read_bytes()).hexdigest()
                    for name in ("cli", "server", "worker", "node")}

        def measure(base, variant, phase):
            identifier = str(uuid.uuid4())
            bep = work / "events.jsonl"
            execution = work / "execution.json"
            for path in (bep, execution):
                path.unlink(missing_ok=True)
            row = {"variant": variant, "revision": REVISIONS[variant], "phase": phase,
                   "invocation_url": "https://impalasys.buildbuddy.io/invocation/" + identifier}
            print("CRATE_SPLIT_START " + json.dumps(row), flush=True)
            with (work / "private-build.log").open("w") as log:
                started = time.monotonic()
                process = subprocess.run(command(base, "build", variant, *TARGETS,
                    f"--invocation_id={identifier}", f"--build_event_json_file={bep}",
                    f"--execution_log_json_file={execution}"), stdout=log, stderr=subprocess.STDOUT)
                row.update(seconds=time.monotonic() - started, exit_code=process.returncode)
            actions = rust_actions(execution)
            row.update(metrics=metrics(bep), rustc_actions=len(actions),
                       root_rustc_targets=sorted(set(label for label in actions if label.startswith("//"))))
            report["samples"].append(row)
            result_path.write_text(json.dumps(report, indent=2) + "\n")
            print("CRATE_SPLIT_SAMPLE " + json.dumps({k: v for k, v in row.items() if k != "metrics"}), flush=True)
            if process.returncode:
                raise RuntimeError(f"{variant}/{phase} failed; see {row['invocation_url']}")
            return row

        active = None
        try:
            subprocess.run(["git", "fetch", "--no-tags", "--filter=blob:none",
                            "https://github.com/impalasys/talon.git", *REVISIONS.values()], check=True)
            # Prefetch both dependency graphs before either revision is timed.
            for variant in order:
                checkout(variant)
                active = work / "prefetch"
                print("CRATE_SPLIT_PREFETCH " + variant, flush=True)
                with (work / "private-prefetch.log").open("w") as log:
                    subprocess.run(command(active, "fetch", variant, *TARGETS), stdout=log,
                                   stderr=subprocess.STDOUT, check=True)
                subprocess.run(command(active, "shutdown", variant), check=True)
                active = None
            shutil.rmtree(work / "prefetch", ignore_errors=True)
            for variant in order:
                checkout(variant)
                active = work / ("output-" + variant)
                row = measure(active, variant, "cold")
                assert int(row["metrics"]["cumulativeMetrics"]["numBuilds"]) == 1
                assert row["rustc_actions"] > 0, "Cold compilation did not execute Rustc."
                current_hashes = hashes()
                row["binary_hashes"] = current_hashes
                for index in range(3):
                    warm = measure(active, variant, f"warm-{index + 1}")
                    assert warm["rustc_actions"] == 0, "Warm repeat unexpectedly compiled Rust."
                for case, files in EDITS.items():
                    source = Path(files[0 if variant == "before" else 1])
                    source.write_bytes(source.read_bytes() +
                        f'\n#[allow(dead_code)]\npub const __TALON_CRATE_SPLIT_BENCHMARK: &str = "{nonce}-{case}";\n'.encode())
                    row = measure(active, variant, "incremental-" + case)
                    assert row["rustc_actions"] > 0, "Source change did not execute Rustc."
                    now = hashes()
                    row["source"] = str(source)
                    row["binary_hashes"] = now
                    if variant == "after":
                        unchanged = UNAFFECTED.get(case, [])
                        assert all(now[name] == current_hashes[name] for name in unchanged), (case, unchanged)
                        row["verified_unaffected_binaries"] = unchanged
                    current_hashes = now
                subprocess.run(command(active, "shutdown", variant), check=True)
                shutil.rmtree(active, ignore_errors=True)
                active = None
            report["success"] = True
        except BaseException as error:
            report["error"] = f"{type(error).__name__}: {error}"
            raise
        finally:
            if active is not None:
                subprocess.run(command(active, "shutdown", order[-1]), check=False)
            subprocess.run(["git", "checkout", "--detach", "--force", control], check=True)
            report["tracked_edits_restored"] = not capture("git", "diff", "--name-only")
            result_path.write_text(json.dumps(report, indent=2) + "\n")
            print("CRATE_SPLIT_REPORT " + json.dumps(report), flush=True)


if __name__ == "__main__":
    main()
