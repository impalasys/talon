#!/usr/bin/env bash
set -euo pipefail
"$1"
for backend in sqlite rocksdb; do
  memory=512m
  options=()
  if [[ "$backend" == rocksdb ]]; then
    memory=768m
    options=(--rocksdb-compression none --rocksdb-write-buffer-size-mb 16 --rocksdb-max-write-buffer-number 2 --rocksdb-block-cache-size-mb 16 --rocksdb-max-background-jobs 2)
  fi
  output="$TEST_UNDECLARED_OUTPUTS_DIR/$backend"
  "$2" --skip-build --database "$backend" --agents 7000 --latencies 0 --memory "$memory" \
    --timeout-seconds 900 --send-concurrency 7000 --worker-concurrency 7000 \
    --stream-mode batch --mock-request-backlog 8192 --progress-interval-seconds 10 \
    --container-platform linux/amd64 --image-tag talon-bench-runtime:dev \
    --output-dir "$output" "${options[@]}" || echo "Advisory benchmark threshold missed for $backend"
  # Runtime sockets/FIFOs are not valid Bazel test artifacts. Containers have
  # exited, so only these transient endpoints need removal from the output tree.
  find "$output" \( -type s -o -type p \) -delete
  compgen -G "$output/talon-bench-*.json" >/dev/null || { echo "Missing benchmark result for $backend" >&2; exit 1; }
done
