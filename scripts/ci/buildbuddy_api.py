"""Read BuildBuddy artifacts without exposing the API credential to build scripts."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import urllib.request
from urllib.parse import urlparse

ENDPOINT = 'https://impalasys.buildbuddy.io/api/v1/'


def request(method, payload):
    key = os.environ.get('BUILDBUDDY_API_KEY')
    if not key:
        raise RuntimeError('BUILDBUDDY_API_KEY is required for artifact retrieval')
    req = urllib.request.Request(ENDPOINT + method, data=json.dumps(payload).encode(), headers={
        'Content-Type': 'application/json', 'x-buildbuddy-api-key': key})
    with urllib.request.urlopen(req, timeout=120) as response:
        return json.load(response)


def invocation(identifier):
    if not re.fullmatch(r'[0-9a-f-]{36}', identifier):
        raise ValueError('Invalid invocation ID')
    result = request('GetInvocation', {'selector': {'invocation_id': identifier}, 'include_artifacts': True})
    rows = result.get('invocation', [])
    if len(rows) != 1 or rows[0].get('id', {}).get('invocationId') != identifier:
        raise RuntimeError('Expected exactly the requested BuildBuddy invocation')
    if not rows[0].get('success'):
        raise RuntimeError('BuildBuddy invocation has not succeeded')
    return rows[0]


def artifact(row, name):
    matches = [f for f in row.get('artifacts', []) if f['name'] in (name, 'command-0/' + name)]
    if len(matches) != 1:
        raise RuntimeError(f'Expected one artifact named {name}')
    return matches[0]


def download(file, destination):
    uri = file['uri']
    parsed = urlparse(uri)
    match = re.search(r'/blobs/([0-9a-f]{64})/(\d+)$', parsed.path)
    if parsed.scheme != 'bytestream' or parsed.netloc != 'remote.buildbuddy.io' or not match:
        raise ValueError('Unexpected artifact CAS URI')
    digest, raw_size = match.groups()
    size = int(raw_size)
    if size > 8 * 1024**3:
        raise ValueError('Artifact exceeds the 8-GiB transfer limit')
    destination.parent.mkdir(parents=True, exist_ok=True)
    hasher = hashlib.sha256()
    try:
        with destination.open('wb') as out:
            # The bounded API keeps base64 JSON responses from exhausting runner RAM.
            for start in range(0, size, 8 * 1024**2):
                end = min(size, start + 8 * 1024**2)
                response = request('GetFileRange', {'uri': uri, 'start': start, 'end': end})
                data = base64.b64decode(response.get('data', ''), validate=True)
                if len(data) != end - start:
                    raise RuntimeError('Truncated BuildBuddy artifact')
                out.write(data)
                hasher.update(data)
        if hasher.hexdigest() != digest:
            raise RuntimeError('BuildBuddy artifact checksum mismatch')
    except BaseException:
        destination.unlink(missing_ok=True)
        raise


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--invocation', required=True)
    parser.add_argument('--artifact', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    row = invocation(args.invocation)
    download(artifact(row, args.artifact), args.output)
    print(f'Verified {args.artifact} from {args.invocation}')
