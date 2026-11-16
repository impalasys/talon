#!/usr/bin/env python3
"""One Linux CI runner: affected PR targets, full compatible monorepo on main."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import time
import uuid

import affected_targets as affected
import linux_pilot as pilot

PLATFORM = '//:linux_ci'
OPTIONS = [s.replace('--jobs=4', '--jobs=16').replace('--local_resources=cpu=4', '--local_resources=cpu=16')
           .replace('--local_resources=memory=12288', '--local_resources=memory=20480') for s in pilot.OPTIONS]
OPTIONS += ['--platforms=' + PLATFORM]


def git(*arguments):
    return subprocess.check_output(['git', *arguments], text=True).strip()


def plan(full=False):
    event = os.environ.get('BUILDBUDDY_TRIGGER_EVENT', '')
    branch = os.environ.get('GIT_BRANCH', git('branch', '--show-current'))
    full = full or (event == 'push' and branch == 'main')
    source_head = os.environ.get('GIT_COMMIT') or git('rev-parse', 'HEAD')
    base = None
    changed = []
    if full:
        try:
            changed = git('diff', '--name-only', '-z', '--no-renames', 'HEAD^', 'HEAD').rstrip('\0').split('\0')
        except subprocess.CalledProcessError:
            changed = git('ls-files', '-z').rstrip('\0').split('\0')
        changed = [p for p in changed if p]
    else:
        # Fetch the base repository, including for fork PRs. The runner already
        # merged the requested base; merge-base pins the base actually tested.
        subprocess.run(['git', 'fetch', '--no-tags', 'https://github.com/impalasys/talon.git',
                        'main:refs/remotes/ci-base/main'], check=True)
        base = git('merge-base', 'refs/remotes/ci-base/main', 'HEAD')
        changed = subprocess.check_output(['git', 'diff', '--name-only', '-z', '--no-renames',
                                          base + '...HEAD']).decode().rstrip('\0').split('\0')
        changed = [p for p in changed if p]
    targets = affected.select_targets(Path.cwd(), 'bazel', changed, [], PLATFORM, full)
    tests = affected.output_labels(affected.bazel_output('bazel', 'query',
                                    'tests(' + affected.label_set(targets) + ')', '--output=label')) if targets else []
    return {'schema': 2, 'repository': 'impalasys/talon', 'head': source_head,
            'built_commit': git('rev-parse', 'HEAD'), 'tree': git('rev-parse', 'HEAD^{tree}'),
            'base': base, 'mode': 'full' if full else 'affected', 'changed_files': changed,
            'targets': targets, 'tests': tests, 'samples': [], 'success': False}


def summaries(path):
    rows = []
    for line in path.read_text().splitlines() if path.exists() else []:
        event = json.loads(line)
        if 'testSummary' in event:
            summary = event['testSummary']
            rows.append({'label': event['id']['testSummary']['label'],
                         **{k: summary[k] for k in ('overallStatus', 'totalRunCount', 'totalNumCached') if k in summary}})
    return rows


def bazel(phase, verb, arguments, output, result):
    with tempfile.TemporaryDirectory(prefix='talon-ci-bep-') as temporary:
        events = Path(temporary) / 'events.jsonl'
        identifier = str(uuid.uuid4())
        command = ['bazel', verb, *OPTIONS, *arguments, '--color=no', '--curses=no',
                   '--profile=' + str(output / (phase + '.profile.gz')),
                   '--build_event_json_file=' + str(events), '--invocation_id=' + identifier]
        started = time.monotonic()
        with (output / (phase + '.log')).open('w') as log:
            process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            for line in process.stdout:
                print(line, end='', flush=True)
                log.write(line)
            code = process.wait()
            process.stdout.close()
        sample = {'phase': phase, 'exit_code': code, 'seconds': time.monotonic() - started,
                  'invocation': identifier, 'metrics': pilot.read_metrics(events)}
        if verb == 'test':
            sample['tests'] = summaries(events)
        result['samples'].append(sample)
        if code:
            raise RuntimeError(f'{phase} failed with Bazel exit {code}')
        if verb == 'test':
            rows = {t['label']: t for t in sample['tests']}
            missing = set(result['tests']) - rows.keys()
            if missing or any(t.get('overallStatus') != 'PASSED' or int(t.get('totalNumCached', 0)) for t in rows.values()):
                raise RuntimeError(f'Missing, cached or failing CI tests: {sorted(missing)}')


def copy_tree(source, destination):
    if source.exists():
        shutil.copytree(source, destination, dirs_exist_ok=True)


def export(output, result):
    handoff = output / 'handoff'
    handoff.mkdir(exist_ok=True)
    selected = set(result['targets'])
    for name in ('runtime', 'ui', 'acp'):
        if '//dockerfiles:' + name + '_tar' in selected:
            source = Path('bazel-bin/dockerfiles') / (name + '_image') / 'tarball.tar'
            target = handoff / 'images' / (name + '.tar')
            target.parent.mkdir(exist_ok=True)
            shutil.copyfile(source, target)
    if '//:release-dist' in selected:
        copy_tree(Path('bazel-bin/release-dist'), handoff / 'release')
    if '//packages/talon-chat:storybook' in selected:
        copy_tree(Path('bazel-bin/packages/talon-chat/storybook-static'), handoff / 'storybook')
    screenshots = {
        'ui/e2e': 'sightline-e2e-screenshots',
        'packages/talon-chat/screenshots': 'talon-chat-storybook-screenshots',
    }
    for label, name in screenshots.items():
        package, test = label.rsplit("/", 1)
        if "//" + package + ":" + test not in result["tests"]:
            continue
        source = Path('bazel-testlogs') / label / 'test.outputs'
        for path in source.glob('*.png'):
            target = handoff / name / path.name
            target.parent.mkdir(exist_ok=True)
            shutil.copyfile(path, target)
    preview = handoff / 'sightline-e2e-screenshots/sightline-chat-dark.png'
    if preview.exists():
        target = handoff / 'sightline-e2e-dark-preview' / preview.name
        target.parent.mkdir(exist_ok=True)
        shutil.copyfile(preview, target)
    if '//bench:ci_benchmark' in result['tests']:
        source = Path('bazel-testlogs/bench/ci_benchmark/test.outputs')
        for path in source.rglob('*'):
            relative = path.relative_to(source)
            if path.is_file() and (path.match('talon-bench-*.json') or
                                   (path.suffix == '.log' and 'data' not in relative.parts)):
                target = handoff / 'benchmark' / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, target)
    if '//ui:dist' in selected:
        bazel('deployment-ui', 'build', ['//ui:dist', '--define=GATEWAY_URL=https://talon.impala.systems'], output, result)
        copy_tree(Path('bazel-bin/ui/dist'), handoff / 'sightline-static-export')
    files = {}
    with tarfile.open(output / 'handoff.tar', 'w') as archive:
        for path in sorted(handoff.rglob('*')):
            if path.is_file():
                name = path.relative_to(handoff).as_posix()
                with path.open('rb') as source:
                    digest = hashlib.file_digest(source, 'sha256').hexdigest()
                files[name] = {'sha256': digest, 'size': path.stat().st_size}
                archive.add(path, arcname=name, recursive=False)
    result['files'] = files
    result['groups'] = sorted({p.split('/')[0] for p in files})
    shutil.rmtree(handoff)


def run(output, full=False, dry_run=False):
    output.mkdir(parents=True, exist_ok=True)
    affinity = pilot.limit_cpu_affinity(16)
    result = plan(full)
    result.update(requested_cpu=16, requested_memory_gb=32, affinity_cpus=affinity,
                  runner_memory=pilot.memory_metadata())
    print('CI_PLAN ' + json.dumps({k: result[k] for k in ('mode', 'head', 'base', 'changed_files', 'tests')}), flush=True)
    started = time.monotonic()
    try:
        if dry_run:
            return 0
        selected = set(result['tests'])
        for test, installer in [('//ui:e2e', '//ui:playwright'),
                                ('//packages/talon-chat:screenshots', '//packages/talon-chat:playwright')]:
            if test in selected:
                subprocess.run(['bazel', 'run', *OPTIONS, installer, '--', 'install', '--with-deps', 'chromium'], check=True)
        if result['targets']:
            patterns = output / 'targets.txt'
            patterns.write_text('\n'.join(result['targets']) + '\n')
            bazel('build', 'build', ['--target_pattern_file=' + str(patterns)], output, result)
        if result['tests']:
            patterns = output / 'tests.txt'
            patterns.write_text('\n'.join(result['tests']) + '\n')
            bazel('test', 'test', ['--target_pattern_file=' + str(patterns), '--nocache_test_results',
                  '--test_output=errors', '--test_env=CI=true', '--test_env=TALON_DOCKER_SANDBOX_TEST=1',
                  '--test_env=DOCKER_HOST=unix:///var/run/docker.sock', '--test_env=TESTCONTAINERS_RYUK_DISABLED=true',
                  '--test_env=ASTRO_TELEMETRY_DISABLED=1'], output, result)
        export(output, result)
        result['success'] = True
        return 0
    finally:
        result['seconds'] = time.monotonic() - started
        for name in ('test.log', 'test.xml'):
            for path in Path('bazel-testlogs').rglob(name):
                target = output / 'testlogs' / path.relative_to('bazel-testlogs')
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, target)
        (output / 'ci-manifest.json').write_text(json.dumps(result, indent=2) + '\n')
        print('CI_RESULT ' + json.dumps({k: result[k] for k in ('head', 'mode', 'success', 'seconds')}), flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--full', action='store_true')
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    raise SystemExit(run(args.output.resolve(), args.full, args.dry_run))
