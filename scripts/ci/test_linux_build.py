import contextlib
import io
import json
import os
import subprocess
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import linux_build as runner


class RunnerTest(unittest.TestCase):
    def test_read_only_bazel_output_can_be_exported_and_cleaned_up(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'bazel-bin/release-dist/nested'
            source.mkdir(parents=True)
            (source / 'archive').write_bytes(b'release bytes')
            source.chmod(0o555)
            source.parent.chmod(0o555)
            artifacts = root / 'artifacts'
            artifacts.mkdir()
            previous = Path.cwd()
            manifest = {'targets': ['//:release-dist'], 'tests': []}
            try:
                os.chdir(root)
                runner.export(artifacts, manifest)
            finally:
                os.chdir(previous)
                source.parent.chmod(0o755)
                source.chmod(0o755)
            self.assertIn('release/nested/archive', manifest['files'])
            self.assertTrue((artifacts / 'handoff.tar').exists())
            self.assertFalse((artifacts / 'handoff').exists())

    def test_missing_selected_benchmark_reports_fail_export(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(RuntimeError, 'no reports'):
                runner.export(Path(temporary), {'targets': [], 'tests': ['//bench:ci_benchmark']})

    def test_new_discovered_target_is_built_and_tested_without_registration(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = '//new:unregistered_test'
            record = {'type': 'RULE', 'rule': {'name': target, 'ruleClass': 'py_test', 'ruleInput': []}}
            graph = json.dumps(record)
            configured = json.dumps({'results': [{'target': record}]})
            executable = root / 'bazel'
            executable.write_text('#!/usr/bin/env python3\n' + '''import json, os, sys
from pathlib import Path
args = sys.argv[1:]
patterns = next((a.split('=', 1)[1] for a in args if a.startswith('--target_pattern_file=')), None)
with open(os.environ['CI_CALLS'], 'a') as log:
    log.write(json.dumps({'args': args, 'targets': Path(patterns).read_text() if patterns else ''}) + '\\n')
'''+f'''if args[0] == 'query': print({graph!r} if args[1] == '//...:*' else {target!r})
if args[0] == 'cquery' and '--output=jsonproto' in args: print({configured!r})
if args[0] == 'test':
    output = next(a.split('=', 1)[1] for a in args if a.startswith('--build_event_json_file='))
    Path(output).write_text(json.dumps({{'id': {{'testSummary': {{'label': {target!r}}}}}, 'testSummary': {{'overallStatus': 'PASSED', 'totalRunCount': 1}}}}) + '\\n')
    if os.environ.get('CI_TRUNCATED_EVENTS'): Path(output).write_text('{{')
if args[0] == 'test' and os.environ.get('CI_FAIL_TEST'):
    sys.exit(17)
''')
            executable.chmod(0o755)
            log = root / 'calls'
            previous = Path.cwd()
            try:
                os.chdir(root)
                with patch.dict(os.environ, {'PATH': str(root) + ':' + os.environ['PATH'],
                                              'CI_CALLS': str(log), 'GIT_COMMIT': 'a' * 40}), \
                        patch.object(runner, 'git', return_value='a' * 40), \
                        patch.object(runner.pilot, 'limit_cpu_affinity', return_value=list(range(16))), contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(runner.run(root / 'out', full=True), 0)
                    with patch.dict(os.environ, {'CI_FAIL_TEST': '1'}):
                        with self.assertRaisesRegex(RuntimeError, 'exit 17'):
                            runner.run(root / 'failed', full=True)
                        with patch.dict(os.environ, {'CI_TRUNCATED_EVENTS': '1'}):
                            with self.assertRaisesRegex(RuntimeError, 'exit 17'):
                                runner.run(root / 'truncated', full=True)
            finally:
                os.chdir(previous)
            calls = [json.loads(line) for line in log.read_text().splitlines()]
            self.assertTrue(any(c['args'][0] == 'build' and target in c['targets'] for c in calls))
            self.assertTrue(any(c['args'][0] == 'test' and target in c['targets'] for c in calls))
            manifest = json.loads((root / 'out/ci-manifest.json').read_text())
            self.assertTrue(manifest['success'])
            self.assertEqual(manifest['tests'], [target])
            failed = json.loads((root / 'failed/ci-manifest.json').read_text())
            self.assertFalse(failed['success'])
            self.assertEqual(failed['samples'][-1]['exit_code'], 17)
            truncated = json.loads((root / 'truncated/ci-manifest.json').read_text())
            self.assertFalse(truncated['success'])
            self.assertEqual(truncated['samples'][-1]['exit_code'], 17)

    def test_benchmark_cleanup_preserves_reports_and_removes_runtime_sockets(self):
        with tempfile.TemporaryDirectory(prefix='talon-ci-', dir='/tmp') as temporary:
            root = Path(temporary)
            loader = root / 'loader'
            loader.write_text('#!/bin/sh\nexit 0\n')
            loader.chmod(0o755)
            benchmark = root / 'benchmark'
            benchmark.write_text("""#!/usr/bin/env python3
import json, socket, sys
from pathlib import Path
output = Path(sys.argv[sys.argv.index('--output-dir') + 1])
(output / 'data').mkdir(parents=True)
endpoint = socket.socket(socket.AF_UNIX)
endpoint.bind(str(output / 'data/broker.sock'))
endpoint.close()
(output / 'data/database.db').write_bytes(b'not a report')
(output / 'talon-bench-test.json').write_text(json.dumps({'success': True}))
""")
            benchmark.chmod(0o755)
            outputs = root / 'bazel-testlogs/bench/ci_benchmark/test.outputs'
            environment = dict(os.environ, TEST_UNDECLARED_OUTPUTS_DIR=str(outputs))
            script = Path('bench/ci_benchmark.sh').resolve()
            result = subprocess.run(['bash', str(script), str(loader), str(benchmark)],
                                    env=environment, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            for backend in ('sqlite', 'rocksdb'):
                self.assertFalse((outputs / backend / 'data/broker.sock').exists())
                self.assertTrue((outputs / backend / 'talon-bench-test.json').exists())
            artifacts = root / 'artifacts'
            artifacts.mkdir()
            manifest = {'targets': [], 'tests': ['//bench:ci_benchmark']}
            previous = Path.cwd()
            try:
                os.chdir(root)
                runner.export(artifacts, manifest)
            finally:
                os.chdir(previous)
            self.assertEqual(set(manifest['files']), {
                'benchmark/sqlite/talon-bench-test.json', 'benchmark/rocksdb/talon-bench-test.json'})


if __name__ == '__main__':
    unittest.main()
