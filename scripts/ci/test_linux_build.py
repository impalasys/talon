import contextlib
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import linux_build as runner


class RunnerTest(unittest.TestCase):
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
''')
            executable.chmod(0o755)
            log = root / 'calls'
            previous = Path.cwd()
            try:
                os.chdir(root)
                with patch.dict(os.environ, {'PATH': str(root) + ':' + os.environ['PATH'],
                                              'CI_CALLS': str(log), 'GIT_COMMIT': 'a' * 40}), \
                        patch.object(runner, 'git', return_value='a' * 40), \
                        patch.object(runner.pilot, 'limit_cpu_affinity'), contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(runner.run(root / 'out', full=True), 0)
            finally:
                os.chdir(previous)
            calls = [json.loads(line) for line in log.read_text().splitlines()]
            self.assertTrue(any(c['args'][0] == 'build' and target in c['targets'] for c in calls))
            self.assertTrue(any(c['args'][0] == 'test' and target in c['targets'] for c in calls))
            manifest = json.loads((root / 'out/ci-manifest.json').read_text())
            self.assertTrue(manifest['success'])
            self.assertEqual(manifest['tests'], [target])


if __name__ == '__main__':
    unittest.main()
