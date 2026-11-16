#!/usr/bin/env bash
set -euo pipefail
"$1"
docker run --rm talon-runtime:dev sh -ec 'talon-cli --help >/dev/null; python3 -c "import pytest, grpc, boto3"; monty --help >/dev/null; docker --version; curl --version'
