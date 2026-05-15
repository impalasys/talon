#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

files=()
while IFS= read -r -d '' file; do
  files+=("$file")
done < <(git ls-files -z '*.rs')

if [[ "${#files[@]}" -eq 0 ]]; then
  exit 0
fi

go run github.com/google/addlicense@v1.2.0 \
  -check \
  -f .github/license-header.txt \
  "${files[@]}"
