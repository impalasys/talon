# Talon Benchmark

This benchmark runs Talon under a 1 vCPU and 512 MiB memory limit in a generated
Docker Compose project, using SQLite by default and the local socket broker. It creates
distinct agents, opens one stream per session, sends one message to each agent,
and waits for terminal stream events.
The harness sets `TALON_WORKER_SESSION_CONCURRENCY=1000` by default so the lone
worker process can run session turns concurrently. The mock LLM streams 50
tokens at 10 tokens per second by default.
SQLite defaults to a 5-connection SQLx pool and a 5000 ms busy timeout; use
`--sqlite-pool-size` and `--sqlite-busy-timeout-ms` to test pool contention vs.
SQLite lock contention.

Each run gets a Compose project name like `talon-a1b2c3`. Containers are named:

- `<project>-talon`
- `<project>-mock-llm`
- `<project>-postgres` when `--database postgres` is used
- `<project>-jaeger` when `--otel` is used

Smoke run:

```bash
.venv-e2e/bin/python bench/benchmark_1000_agents.py --agents 10 --latencies 0 --memory 512m
```

Full run:

```bash
.venv-e2e/bin/python bench/benchmark_1000_agents.py --agents 1000 --latencies 0,50,250 --memory 512m
```

Keep the final Compose stack up for OrbStack inspection:

```bash
.venv-e2e/bin/python bench/benchmark_1000_agents.py --agents 1000 --latencies 0,50,250 --memory 512m --keep-compose-up
```

Enable OpenTelemetry tracing with a Jaeger UI in the generated Compose project:

```bash
.venv-e2e/bin/python bench/benchmark_1000_agents.py --agents 1000 --latencies 0,50,250 --memory 512m --otel --keep-compose-up
```

Inspect SQLite contention by comparing `SqliteKvStore.acquire_connection` and
`SqliteKvStore.query` spans. A high `pool_wait_us` points at SQLx pool
contention; a high `query_elapsed_us` points at SQLite lock, I/O, or scheduler
wait inside query execution. Detailed KV spans are debug-level and the harness
enables `talon::control::kv=debug` only when `--otel` is set. With `--otel`, the
harness also samples Jaeger's trace API and writes a SQLite span summary into the
result JSON and `summary.md`; raise `--jaeger-trace-limit` if you want more
traces included in that aggregate.

```bash
.venv-e2e/bin/python bench/benchmark_1000_agents.py --agents 250 --latencies 0 --memory 512m --otel --keep-compose-up --sqlite-pool-size 5 --sqlite-busy-timeout-ms 5000
```

For a lock-contention probe, temporarily lower the busy timeout:

```bash
.venv-e2e/bin/python bench/benchmark_1000_agents.py --agents 250 --latencies 0 --memory 512m --otel --keep-compose-up --sqlite-busy-timeout-ms 10
```

Run the same benchmark against Postgres instead of SQLite:

```bash
.venv-e2e/bin/python bench/benchmark_1000_agents.py --skip-build --database postgres --agents 250 --latencies 0 --memory 512m --otel --keep-compose-up --mock-cpus 4 --mock-memory 2g
```

The harness prints a mapped Postgres URL. While the stack is up, connect with:

```bash
psql postgres://talon:talon@127.0.0.1:<postgres-port>/talon
```

Talon's Postgres SQLx pool defaults to `--postgres-max-connections 200`.
The Postgres server cap defaults to `2 * pool + 50` because Talon runs separate
gateway and worker pools in the benchmark container. Override it with
`--postgres-server-max-connections`. Use `--postgres-cpus` and
`--postgres-memory` if you want to cap the database container separately from
Talon's 1 vCPU / memory limit.

Run the benchmark against the embedded RocksDB store:

```bash
.venv-e2e/bin/python bench/benchmark_1000_agents.py --skip-build --database rocksdb --agents 250 --latencies 0 --memory 512m --otel --keep-compose-up --mock-cpus 4 --mock-memory 2g
```

RocksDB is an embedded store with a single read/write process lock. The harness
therefore uses `talon-bench-colocated` for `--database rocksdb`, running gateway
and worker in one process with one shared control-plane handle. SQLite and
Postgres keep the normal separate `talon-server` and `talon-worker` processes.

For RocksDB tuning experiments, the harness can pass through:
`--rocksdb-disable-wal`, `--rocksdb-compression`, `--rocksdb-write-buffer-size-mb`,
`--rocksdb-max-write-buffer-number`, `--rocksdb-block-cache-size-mb`,
`--rocksdb-max-background-jobs`, and `--rocksdb-serialize-writes` /
`--no-rocksdb-serialize-writes`. Disabling WAL is a benchmark-only durability
tradeoff unless the workload can tolerate crash-time data loss.

Very large fan-out runs may also need higher container file-descriptor limits.
Use `--talon-nofile` for the Talon container and `--mock-nofile` for the mock LLM
container.

Use a fixed project name when you want a predictable OrbStack group:

```bash
.venv-e2e/bin/python bench/benchmark_1000_agents.py --project-name talon-a1b2c3 --agents 1000 --latencies 0,50,250 --memory 512m
```

Live inspection examples:

```bash
docker compose ls
docker stats talon-a1b2c3-talon talon-a1b2c3-mock-llm
docker stats talon-a1b2c3-postgres
curl http://127.0.0.1:<mock-metrics-port>/metrics
open http://127.0.0.1:<jaeger-ui-port>
docker compose -p talon-a1b2c3 -f bench/results/talon-a1b2c3/0ms/compose.yaml logs -f
```

The harness prints the assigned gRPC port, mock metrics URL, and Jaeger UI URL
when `--otel` is enabled.
It writes each generated `compose.yaml`, per-profile JSON, SQLite data directory,
and `summary.md` under `bench/results/`.
The mock LLM is intentionally outside the Talon CPU and memory budget.
It uses a threaded Python HTTP server with a large listen backlog by default.
If the mock server becomes the bottleneck, raise its resources without changing
Talon's limits:

```bash
.venv-e2e/bin/python bench/benchmark_1000_agents.py --agents 250 --latencies 0 --memory 512m --otel --keep-compose-up --sqlite-pool-size 20 --mock-cpus 4 --mock-memory 2g --mock-request-backlog 8192
```

The default benchmark image uses `bench/runtime.Dockerfile`, which avoids
BuildKit-only syntax so it works on Docker installations without Buildx. To use
the production runtime Dockerfile instead, pass
`--dockerfile dockerfiles/oss-runtime.Dockerfile` on systems with BuildKit.
