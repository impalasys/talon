#!/usr/bin/env python3
"""Trusted GitHub-side relay for an exact successful BuildBuddy CI revision.

Always run this file from the base/default branch. Never execute PR code in the
job which receives BUILDBUDDY_API_KEY; extracted artifacts are data only.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import time
from urllib.parse import urlparse
import buildbuddy_api as bb

CONTEXT = 'Talon CI'
GROUPS = {'images', 'release', 'storybook', 'sightline-static-export', 'sightline-e2e-screenshots',
          'sightline-e2e-dark-preview', 'talon-chat-storybook-screenshots', 'benchmark'}


def github(endpoint):
    return json.loads(subprocess.check_output(['gh', 'api', endpoint], text=True))


def revision(event, repository, sha=None):
    if repository != 'impalasys/talon':
        raise RuntimeError('Unexpected repository')
    if 'pull_request' in event:
        pr = github(f'repos/{repository}/pulls/{event["number"]}')
        if pr['head']['sha'] != event['pull_request']['head']['sha'] or pr['base']['sha'] != event['pull_request']['base']['sha']:
            raise RuntimeError('PR revision changed; rerun CI for the updated head/base')
        head = pr['head']['sha']
        merge = pr.get('merge_commit_sha')
        if not merge:
            raise RuntimeError('PR merge revision unavailable')
        base = pr['base']['sha']
        metadata = {'head': head, 'head_repository': pr['head']['repo']['full_name'],
                    'branch': pr['head']['ref'], 'pr': pr['number']}
    else:
        head = sha or os.environ['GITHUB_SHA']
        merge, base = head, None
        metadata = {'head': head, 'head_repository': repository, 'branch': 'main', 'pr': None}
    if not re.fullmatch(r'[0-9a-f]{40}', head) or not re.fullmatch(r'[0-9a-f]{40}', merge):
        raise RuntimeError('Expected immutable revision SHAs')
    tree = github(f'repos/{repository}/git/commits/{merge}')['tree']['sha']
    return head, base, tree, metadata


def wait_status(repository, head, timeout):
    deadline = time.monotonic() + timeout
    last = None
    while True:
        rows = json.loads(subprocess.check_output(['gh', 'api',
                    f'repos/{repository}/commits/{head}/statuses?per_page=100', '--paginate', '--slurp'], text=True))
        status = next((s for page in rows for s in page if s['context'] == CONTEXT), None)
        state = status['state'] if status else 'missing'
        if state != last:
            print(f'{CONTEXT}: {state} for {head}', flush=True)
            last = state
        if status:
            url = urlparse(status.get('target_url') or '')
            match = re.fullmatch(r'/invocation/([0-9a-f-]{36})', url.path)
            if (status.get('creator', {}).get('login') != 'buildbuddy-io[bot]' or
                    url.scheme != 'https' or url.netloc != 'impalasys.buildbuddy.io' or not match):
                raise RuntimeError('Unexpected BuildBuddy status publisher or URL')
            if state == 'success':
                return match[1]
            if state in ('failure', 'error'):
                raise RuntimeError(f'BuildBuddy CI failed: {status["target_url"]}')
        if time.monotonic() >= deadline:
            raise RuntimeError(f'Timed out waiting for {CONTEXT} ({state})')
        time.sleep(min(15, max(0, deadline - time.monotonic())))


def validate_manifest(manifest, head, base, tree):
    if (manifest.get('schema') != 2 or manifest.get('repository') != 'impalasys/talon' or
            manifest.get('head') != head or manifest.get('tree') != tree or
            manifest.get('base') != base or not manifest.get('success')):
        raise RuntimeError('Artifact manifest does not match the successful requested revision/tree')
    if base is None and manifest.get('mode') != 'full':
        raise RuntimeError('Main requires a full monorepo invocation')
    files = manifest.get('files')
    if not isinstance(files, dict) or len(files) > 50000:
        raise RuntimeError('Invalid artifact inventory')
    for name, info in files.items():
        path = PurePosixPath(name)
        if (path.is_absolute() or '..' in path.parts or not path.parts or
                path.parts[0] not in GROUPS or '\\' in name or
                not re.fullmatch('[0-9a-f]{64}', info.get('sha256', '')) or
                not isinstance(info.get('size'), int) or not 0 <= info['size'] <= 8 * 1024**3):
            raise RuntimeError('Unsafe artifact inventory')
    if sum(info['size'] for info in files.values()) > 8 * 1024**3:
        raise RuntimeError('Artifact inventory exceeds transfer limit')
    if base is None:
        required = {'images/runtime.tar', 'images/ui.tar', 'images/acp.tar'}
        if not required <= files.keys():
            raise RuntimeError('Full main invocation is missing published images')


def unpack(archive, output, files):
    seen = set()
    with tarfile.open(archive, 'r:') as bundle:
        for member in bundle:
            name = member.name
            if not member.isfile() or member.sparse or name not in files or name in seen or member.size != files[name]['size']:
                raise RuntimeError('Unexpected, linked, duplicate or truncated artifact member')
            target = output / name
            target.parent.mkdir(parents=True, exist_ok=True)
            digest = hashlib.sha256()
            with bundle.extractfile(member) as source, target.open('wb') as destination:
                while chunk := source.read(1024 * 1024):
                    digest.update(chunk)
                    destination.write(chunk)
            target.chmod(0o644)
            if digest.hexdigest() != files[name]['sha256']:
                raise RuntimeError('Artifact member checksum mismatch')
            if name.endswith('.png'):
                with target.open('rb') as screenshot:
                    if screenshot.read(8) != b'\x89PNG\r\n\x1a\n':
                        raise RuntimeError('Screenshot is not a PNG')
            seen.add(name)
    if seen != files.keys():
        raise RuntimeError('Missing artifacts in handoff bundle')


def fetch(output, timeout=6000, manifest_only=False):
    event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text())
    repository = os.environ['GITHUB_REPOSITORY']
    expected = revision(event, repository)
    head, base, tree, metadata = expected
    identifier = wait_status(repository, head, timeout)
    invocation = bb.invocation(identifier)
    output.mkdir(parents=True, exist_ok=True)
    manifest_path = output / 'ci-manifest.json'
    bb.download(bb.artifact(invocation, manifest_path.name), manifest_path)
    manifest = json.loads(manifest_path.read_text())
    validate_manifest(manifest, head, base, tree)
    if not manifest_only:
        archive = output / 'handoff.tar'
        bb.download(bb.artifact(invocation, archive.name), archive)
        unpack(archive, output, manifest['files'])
        archive.unlink()
    if revision(event, repository) != expected:
        raise RuntimeError('PR revision changed during artifact retrieval')
    (output / 'pr.json').write_text(json.dumps(metadata) + '\n')
    destination = os.environ.get('GITHUB_OUTPUT')
    if destination:
        with open(destination, 'a') as stream:
            stream.write(f'head={head}\n')
            for group in sorted(GROUPS):
                stream.write(group.replace('-', '_') + '=' + str(any(n.startswith(group + '/') for n in manifest['files'])).lower() + '\n')
    summary = os.environ.get('GITHUB_STEP_SUMMARY')
    if summary:
        with open(summary, 'a') as stream:
            stream.write(f'Artifacts verified against `{head}` / tree `{tree}` from [BuildBuddy](https://impalasys.buildbuddy.io/invocation/{identifier}).\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--timeout', type=int, default=6000)
    parser.add_argument('--manifest-only', action='store_true')
    args = parser.parse_args()
    fetch(args.output.resolve(), args.timeout, args.manifest_only)
