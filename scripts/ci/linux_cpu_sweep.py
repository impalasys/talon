#!/usr/bin/env python3
"""Measure local Rust source-invalidation scaling on one BuildBuddy VM."""

import argparse
import json
import os
from pathlib import Path
import statistics
import time
import uuid

import linux_pilot as pilot


def capacity():
    result = {"available_cpus": sorted(os.sched_getaffinity(0)),
              "visible_cpus": os.cpu_count(), "memory": pilot.memory_metadata()}
    for name in ("cpu.max", "memory.max", "cpuset.cpus.effective"):
        path = Path("/sys/fs/cgroup") / name
        if path.exists():
            result[name] = path.read_text().strip()
    cpuinfo = Path("/proc/cpuinfo").read_text().splitlines()
    result["cpu_model"] = next((s.split(":", 1)[1].strip() for s in cpuinfo
                                if s.startswith("model name")), "unknown")
    return result


def sweep(args):
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    source = Path("src/lib.rs")
    if pilot.capture("git", "status", "--porcelain", "--", str(source)):
        raise SystemExit("CPU sweep requires a clean src/lib.rs")
    original = source.read_bytes()
    available = sorted(os.sched_getaffinity(0))
    eligible = sorted(set(c for c in args.cpus if c <= len(available)))
    if not eligible:
        raise SystemExit("No requested CPU size is available")
    report = {"schema": 1, "commit": pilot.capture("git", "rev-parse", "HEAD"),
              "capacity": capacity(), "reservation_cpu": len(available), "requested_sizes": args.cpus,
              "skipped_sizes": sorted(set(args.cpus) - set(eligible)),
              "method": "One excluded seed; unique comment-only Rust input per sample; ascending then descending CPU order. Same VM, 16 GB reservation, 4 GiB JVM, 12 GiB action-memory budget, local execution, remote uploads disabled.",
              "samples": [], "success": False}
    nonce = uuid.uuid4().hex
    failed = set()
    def measure(cpus, name):
        os.sched_setaffinity(0, available)
        os.environ["PILOT_SETUP_STARTED_AT"] = str(time.time())
        code = pilot.run(argparse.Namespace(provider="buildbuddy", output=output / name, cpus=cpus))
        result = json.loads((output / name / "results.json").read_text())
        result["requested_cpu"] = len(available)
        result["cpu_limit"] = cpus
        (output / name / "results.json").write_text(json.dumps(result, indent=2) + "\n")
        row = {"name": name, "cpus": cpus, "success": result["success"],
               "result": f"{name}/results.json", "measured_seconds": result["measured_seconds"]}
        for iteration in ("initial", "warm"):
            row[iteration + "_seconds"] = sum(s["seconds"] for s in result["samples"]
                                               if s["iteration"] == iteration)
        print("CPU_SWEEP_SAMPLE " + json.dumps(row), flush=True)
        return code, row
    try:
        # Populate native dependencies before any timing sample, without a source edit.
        code, seed = measure(max(eligible), "seed")
        report["excluded_seed"] = seed
        if code:
            raise SystemExit(code)
        for index, cpus in enumerate(eligible + eligible[::-1]):
            if cpus in failed:
                continue
            source.write_bytes(original + f"\n// CPU sweep input {nonce} {index:03d}.\n".encode())
            name = f"sample-{index + 1}-{cpus}cpu"
            code, row = measure(cpus, name)
            report["samples"].append(row)
            if code:
                failed.add(cpus)
            (output / "cpu-sweep.json").write_text(json.dumps(report, indent=2) + "\n")
        report["statistics"] = {}
        for cpus in eligible:
            rows = [r for r in report["samples"] if r["cpus"] == cpus and r["success"]]
            if rows:
                report["statistics"][str(cpus)] = {
                    "count": len(rows),
                    **{metric: {"median": statistics.median(r[metric] for r in rows),
                                "min": min(r[metric] for r in rows),
                                "max": max(r[metric] for r in rows)}
                       for metric in ("initial_seconds", "warm_seconds", "measured_seconds")},
                }
        report["success"] = not failed and len(report["samples"]) == 2 * len(eligible)
        return 0 if report["success"] else 1
    finally:
        source.write_bytes(original)
        os.sched_setaffinity(0, available)
        pilot.limit_cpu_affinity(len(available))
        (output / "cpu-sweep.json").write_text(json.dumps(report, indent=2) + "\n")
        print("CPU_SWEEP_REPORT " + json.dumps(report), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cpus", nargs="+", type=int, choices=range(1, 81), default=[4, 8, 16])
    return sweep(parser.parse_args())


if __name__ == "__main__":
    raise SystemExit(main())
