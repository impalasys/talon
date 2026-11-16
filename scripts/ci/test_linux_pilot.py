import argparse
import contextlib
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import linux_pilot as pilot
import linux_cpu_sweep as cpu_sweep


class PilotTest(unittest.TestCase):
    def test_cpu_affinity_covers_retained_server_threads_and_workers(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for pid, parent, command, tids in [
                (10, 1, b"bazel(talon)\0-jar\0/tmp/A-server.jar\0", [10, 11]),
                (20, 10, b"persistent-compiler-worker\0", [20]),
                (30, 1, b"unrelated-process\0", [30]),
            ]:
                process = root / str(pid)
                process.mkdir()
                (process / "cmdline").write_bytes(command)
                (process / "status").write_text(f"PPid:\t{parent}\n")
                for tid in tids:
                    (process / "task" / str(tid)).mkdir(parents=True)
            with patch.object(pilot, "Path", return_value=root), \
                    patch.object(pilot.os, "sched_getaffinity", return_value=set(range(7)), create=True), \
                    patch.object(pilot.os, "sched_setaffinity", create=True) as bind:
                self.assertEqual(pilot.limit_cpu_affinity(), [0, 1, 2, 3])
            self.assertEqual({call.args[0] for call in bind.call_args_list}, {0, 10, 11, 20})
            self.assertTrue(all(call.args[1] == [0, 1, 2, 3] for call in bind.call_args_list))

    def test_cpu_affinity_rejects_a_smaller_runner(self):
        with patch.object(pilot.os, "sched_getaffinity", return_value={0, 1}, create=True):
            with self.assertRaisesRegex(SystemExit, "needs 4 available CPUs"):
                pilot.limit_cpu_affinity()

    def test_success_exports_four_samples_and_no_raw_event_stream(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            executable = root / "bazel"
            executable.write_text(
                "#!/bin/sh\n"
                'for arg in "$@"; do\n'
                '  case "$arg" in\n'
                "    --build_event_json_file=*) printf '%s\\n' "
                "'{\"buildMetrics\":{\"timingMetrics\":{\"wallTimeInMs\":100}}}' "
                '> "${arg#*=}" ;;\n'
                '    --profile=*) touch "${arg#*=}" ;;\n'
                '  esac\n'
                'done\n'
            )
            executable.chmod(0o755)
            args = argparse.Namespace(provider="buildbuddy", output=root / "output", cpus=8)
            with patch.dict(os.environ, {"PATH": f"{root}:{os.environ['PATH']}"}), \
                    patch.object(pilot.platform, "system", return_value="Linux"), \
                    patch.object(pilot.platform, "machine", return_value="x86_64"), \
                    patch.object(pilot.platform, "freedesktop_os_release", return_value={"PRETTY_NAME": "Ubuntu 24.04"}), \
                    patch.object(pilot, "capture", return_value="test"), \
                    patch.object(pilot, "limit_cpu_affinity", return_value=list(range(8))), \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(pilot.run(args), 0)
            result = json.loads((args.output / "results.json").read_text())
            self.assertTrue(result["success"])
            self.assertEqual(result["requested_cpu"], 8)
            self.assertIn("--jobs=8", result["configuration"]["options"])
            self.assertIn("--local_resources=cpu=8", result["configuration"]["options"])
            self.assertNotIn("--jobs=4", result["configuration"]["options"])
            self.assertEqual(len(result["samples"]), 4)
            self.assertEqual(len({s["invocation_url"] for s in result["samples"]}), 4)
            self.assertEqual(result["samples"][0]["metrics"]["timingMetrics"]["wallTimeInMs"], 100)
            self.assertEqual(len(list(args.output.glob("*.profile.gz"))), 4)
            self.assertEqual(len(list(args.output.glob("*.log"))), 4)
            self.assertFalse(list(args.output.glob("*.jsonl")))

    def test_metrics_exclude_command_lines_and_keep_nested_cache_counts(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "events.jsonl"
            path.write_text(json.dumps({"structuredCommandLine": {"secret": "private-key"}}) + "\n" +
                            json.dumps({"buildMetrics": {"timingMetrics": {"wallTimeInMs": "100"},
                                                        "actionSummary": {"actionCacheStatistics": {"hits": 7}}}}) +
                            '\n{"incomplete":')
            metrics = pilot.read_metrics(path)
            self.assertEqual(metrics["actionSummary"]["actionCacheStatistics"]["hits"], 7)
            self.assertNotIn("private-key", json.dumps(metrics))

    def test_process_failure_is_preserved_and_warm_run_is_not_attempted(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            executable = root / "bazel"
            executable.write_text("#!/bin/sh\necho 'simulated build failure'\nexit 17\n")
            executable.chmod(0o755)
            args = argparse.Namespace(provider="github", output=root / "output", cpus=4)
            with patch.dict(os.environ, {"PATH": f"{root}:{os.environ['PATH']}"}), \
                    patch.object(pilot.platform, "system", return_value="Linux"), \
                    patch.object(pilot.platform, "machine", return_value="x86_64"), \
                    patch.object(pilot.platform, "freedesktop_os_release", return_value={"PRETTY_NAME": "Ubuntu 24.04"}), \
                    patch.object(pilot, "capture", return_value="test"), \
                    patch.object(pilot, "limit_cpu_affinity", return_value=[0, 1, 2, 3]), \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(pilot.run(args), 17)
            result = json.loads((args.output / "results.json").read_text())
            self.assertFalse(result["success"])
            self.assertEqual(len(result["samples"]), 1)
            self.assertEqual(result["samples"][0]["exit_code"], 17)

    def test_cpu_sweep_restores_source_and_affinity_on_interruption(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "src").mkdir()
            source = root / "src/lib.rs"
            source.write_bytes(b"// Original source.\n")
            original = source.read_bytes()
            previous = Path.cwd()
            calls = []
            def simulate(args):
                calls.append(args.cpus)
                if len(calls) > 1:
                    self.assertNotEqual(source.read_bytes(), original)
                    raise RuntimeError("interrupted sample")
                args.output.mkdir(parents=True)
                (args.output / "results.json").write_text(json.dumps({
                    "success": True, "measured_seconds": 2,
                    "samples": [{"iteration": "initial", "seconds": 1},
                                {"iteration": "warm", "seconds": 1}],
                }))
                return 0
            try:
                os.chdir(root)
                with patch.object(cpu_sweep.pilot, "capture", return_value=""), \
                        patch.object(cpu_sweep, "capacity", return_value={}), \
                        patch.object(cpu_sweep.pilot, "run", side_effect=simulate), \
                        patch.object(cpu_sweep.os, "sched_getaffinity", return_value=set(range(16)), create=True), \
                        patch.object(cpu_sweep.os, "sched_setaffinity", create=True) as bind, \
                        contextlib.redirect_stdout(io.StringIO()):
                    with self.assertRaisesRegex(RuntimeError, "interrupted sample"):
                        cpu_sweep.sweep(argparse.Namespace(output=root / "results", cpus=[4, 8]))
                self.assertEqual(source.read_bytes(), original)
                self.assertEqual(calls, [8, 4])
                self.assertEqual(bind.call_args.args, (0, list(range(16))))
                report = json.loads((root / "results/cpu-sweep.json").read_text())
                self.assertFalse(report["success"])
                self.assertEqual(report["reservation_cpu"], 16)
            finally:
                os.chdir(previous)

    def write_result(self, root, provider, duration, config="same", success=True):
        path = root / f"{provider}-{duration}.json"
        path.write_text(json.dumps({
            "commit": "123456789abcdef", "configuration_id": config,
            "provider": provider, "success": success,
            "samples": [{"iteration": iteration, "phase": phase, "seconds": duration}
                        for iteration in ("initial", "warm") for phase in ("build", "test")],
        }))
        return path

    def test_comparison_uses_repeated_runs_and_reports_reduction(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            paths = [self.write_result(root, "github", 10), self.write_result(root, "github", 30),
                     self.write_result(root, "buildbuddy", 5), self.write_result(root, "buildbuddy", 15)]
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(pilot.compare(argparse.Namespace(results=paths)), 0)
            self.assertIn("40.00 / 60.00", output.getvalue())
            self.assertIn("20.00 / 30.00", output.getvalue())
            self.assertIn("50.0% | 2 / 2", output.getvalue())

    def test_comparison_rejects_mismatched_toolchains_and_failed_runs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            gh = self.write_result(root, "github", 10)
            bb = self.write_result(root, "buildbuddy", 5, config="different")
            with self.assertRaisesRegex(SystemExit, "Missing matched provider"):
                pilot.compare(argparse.Namespace(results=[gh, bb]))
            bb = self.write_result(root, "buildbuddy", 5, success=False)
            with self.assertRaisesRegex(SystemExit, "unsuccessful run"):
                pilot.compare(argparse.Namespace(results=[gh, bb]))


if __name__ == "__main__":
    unittest.main()
