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


class PilotTest(unittest.TestCase):
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
            args = argparse.Namespace(provider="buildbuddy", output=root / "output")
            with patch.dict(os.environ, {"PATH": f"{root}:{os.environ['PATH']}"}), \
                    patch.object(pilot.platform, "system", return_value="Linux"), \
                    patch.object(pilot.platform, "machine", return_value="x86_64"), \
                    patch.object(pilot.platform, "freedesktop_os_release", return_value={"PRETTY_NAME": "Ubuntu 24.04"}), \
                    patch.object(pilot, "capture", return_value="test"), \
                    patch.object(pilot, "limit_cpu_affinity", return_value=[0, 1, 2, 3]), \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(pilot.run(args), 0)
            result = json.loads((args.output / "results.json").read_text())
            self.assertTrue(result["success"])
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
            args = argparse.Namespace(provider="github", output=root / "output")
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
