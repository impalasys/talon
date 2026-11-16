#!/usr/bin/env bash
set -euo pipefail
"$1"
scratch="$(mktemp -d "$TEST_TMPDIR/talon-ci-benchmark.XXXXXX")"
cleanup() {
  # Docker-created database directories can be root-owned. Remove them using
  # the already-loaded benchmark image, after all workload containers exit.
  docker run --rm --network none --entrypoint /bin/sh \
    --mount "type=bind,source=$scratch,target=/scratch" \
    talon-bench-runtime:dev -c 'rm -rf /scratch/*'
  rmdir "$scratch"
}
trap cleanup EXIT
for backend in sqlite rocksdb; do
  memory=512m
  options=()
  if [[ "$backend" == rocksdb ]]; then
    memory=768m
    options=(--rocksdb-compression none --rocksdb-write-buffer-size-mb 16 --rocksdb-max-write-buffer-number 2 --rocksdb-block-cache-size-mb 16 --rocksdb-max-background-jobs 2)
  fi
  output="$TEST_UNDECLARED_OUTPUTS_DIR/$backend"
  runtime="$scratch/$backend"
  "$2" --skip-build --database "$backend" --agents 7000 --latencies 0 --memory "$memory" \
    --timeout-seconds 900 --send-concurrency 7000 --worker-concurrency 7000 \
    --stream-mode batch --mock-request-backlog 8192 --progress-interval-seconds 10 \
    --container-platform linux/amd64 --image-tag talon-bench-runtime:dev \
    --output-dir "$runtime" "${options[@]}" || echo "Advisory benchmark threshold missed for $backend"
  # Only reports/logs are declared outputs. Never let Bazel validate Docker's
  # runtime sockets, database permissions or transient files as tree artifacts.
  mkdir -p "$output"
  while IFS= read -r -d '' path; do
    destination="$output/${path#"$runtime"/}"
    mkdir -p "$(dirname "$destination")"
    cp "$path" "$destination"
  done < <(find "$runtime" -type d -name data -prune -o -type f \
    \( -name 'talon-bench-*.json' -o -name '*.log' \) -print0)
  compgen -G "$output/talon-bench-*.json" >/dev/null || { echo "Missing benchmark result for $backend" >&2; exit 1; }
done
