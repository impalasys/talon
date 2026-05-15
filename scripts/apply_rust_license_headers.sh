#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

mapfile -d '' files < <(git ls-files -z '*.rs')

if [[ "${#files[@]}" -eq 0 ]]; then
  exit 0
fi

go run github.com/google/addlicense@v1.2.0 \
  -f .github/license-header.txt \
  "${files[@]}"
