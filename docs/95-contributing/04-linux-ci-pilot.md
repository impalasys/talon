# Linux CI and pilot controls

The production **BuildBuddy Linux build/test** action runs on base-repository
pushes with 16 CPUs / 32 GB RAM. Same-repository PRs retain GitHub's `Bazel` job
as a status gate; main and fork PRs execute the existing GitHub job. macOS
releases, publishing, image jobs and separate E2E/reporting jobs stay on GitHub.
See [the migration path](./05-linux-ci-migration.md) for the cutover scope.

## Operate the PR gate

The gate verifies BuildBuddy's publisher, context and exact PR head, requires the
head tree to match GitHub's merge tree, and rechecks the PR head/base before
passing. Update the PR branch if main has advanced. Missing or failed remote
results cannot pass the gate. BuildBuddy logs are linked in its GitHub step
summary; the runner exports `full-job.json`, profiles and individual test logs.
The full job enables `TALON_DOCKER_SANDBOX_TEST=1` and executes uncached tests.

To route PR compilation back to GitHub without changing publishing or releases:

```sh
gh variable set BUILDBUDDY_LINUX_PR_BUILDS --repo impalasys/talon --body false
```

Rerun the affected GitHub CI run after changing the variable. Set it to `true`
(or delete it) to restore BuildBuddy routing. The unset default enables the
BuildBuddy path. This switch does not change main/fork execution, credentials,
branch protection or check names.

## Historical four-CPU control

The additional **BuildBuddy Linux pilot** action now runs only on
`codex/bb-pilot-control-*` pushes. The **GitHub Linux pilot** baseline in
`.github/workflows/buildbuddy-linux-pilot.yml` is manual only. These controls keep
the original matched four-CPU comparison reproducible; they are separate from
the production 16-CPU full workload.

## Enable and run

1. In the [BuildBuddy Workflows UI](https://app.buildbuddy.io/workflows/), select
   the Impala organization and link `impalasys/talon` using the GitHub app. Use the
   checked-in `buildbuddy.yaml`, rather than enabling the default `//...` config.
   The config is inert until the repository is linked and the config is pushed.
2. The existing GitHub `BUILDBUDDY_API_KEY` secret enables manual baseline
   runs **on main only**. No new credential was created. Branch and fork runs do
   not receive the writer key; a manual branch run fails with an explicit setup
   message. Comparing unmerged source revisions requires a separately authorized
   read-only credential.
3. For a matched control at main's current commit, push a
   `codex/bb-pilot-control-*` branch at that exact commit to start BuildBuddy,
   then manually dispatch the GitHub baseline on main using `gh`:

   ```bash
   gh workflow run buildbuddy-linux-pilot.yml --repo impalasys/talon --ref main
   gh run list --repo impalasys/talon --workflow buildbuddy-linux-pilot.yml --branch main
   ```

   Verify matching resolved commit SHAs, especially if main advances between
   dispatches. PR events do not dispatch extra pilot jobs. Fork pushes do not
   trigger this repository's pilot. Exclude an initial seed run before comparing
   repeated samples. The production main snapshot uses a different resource
   configuration; it is not a matched four-CPU control snapshot.
4. Repeat each revision at least five times: rerun the GitHub pilot workflow
   with `gh` and the BuildBuddy action from its UI. Verify the resolved commit on
   each run. Do not dispatch `ci.yml`, whose manual entry point is the cache probe.

## Workload

Both providers run `scripts/ci/linux_pilot.py` on Ubuntu 24.04.5 x86-64, with Bazel
from `.bazelversion`, LLVM 18, four CPU slots and a 12 GiB local action-memory
budget on a runner requesting 16 GiB RAM, with a matching 4 GiB Bazel JVM heap
limit. The runner pins itself and every retained Bazel JVM
thread and compiler worker to four allowed CPUs before measurement; new compiler
processes inherit that affinity. The affinity count and JVM startup options are
part of the comparison fingerprint. This
matters because BuildBuddy may expose more vCPUs than the reservation requests.
BuildBuddy requests four CPUs and 16 GiB explicitly; exposed guest CPU count,
memory and kernel are recorded separately. The local action-memory and JVM heap
budgets match, but resource requests are not a hard memory cgroup limit. These
requests match the public repository's standard GitHub runner. It requests 40 GiB disk
for native compilation; disk capacity and CPU models can differ. The shared
setup script installs missing prerequisites and aligns exact native package
versions from `scripts/ci/linux-pilot-packages.txt`, including Ubuntu's
`base-files` release package, glibc, GCC runtime, binutils and LLVM. It verifies
the resulting Ubuntu 24.04.5 release; it does not rewrite the OS label or relax
comparison matching. Recycled runners skip installation when already aligned.
Exact compiler and native package versions are included in the comparison
fingerprint. Provider kernels are recorded separately because GitHub VMs and
BuildBuddy microVMs have different hosts/kernels; this is a userspace alignment.
The heap limit lives in ignored `.bazelrc.local` to make the same setting
available to workload and runner-cleanup commands. Cross-run JVM reuse must be
verified from the counters; it was not demonstrated in the controlled results.
The Workflow disables BuildBuddy CLI substitution (`bazel_use_cli: false`) to
use the same plain Bazel client as GitHub, including startup rc handling.

Each run executes these commands, then immediately repeats them:

```bash
bazel build //:production_binaries //ui:dist
bazel test //ui:unit_tests
```

Common flags enable the same BuildBuddy remote cache with uploads disabled,
download all outputs, set native compiler paths and disable remote execution.
GitHub restores and saves a dedicated `talon-linux-pilot` disk cache. This
workflow is its sole writer; existing production buckets and workflows retain
their owners. Separate storage is necessary because the controlled native
compiler flags can invalidate the existing compilation snapshot. The pilot
snapshot consumes GitHub cache allowance; check cache sizes/evictions alongside
the existing native release caches, and remove the pilot snapshots when the
experiment ends. BuildBuddy retains its workspace/server via VM snapshots. This is a
comparison of practical cache reuse on each provider, including warm analysis;
it does not isolate JVM startup alone. Pilot-only misses are compiled locally
on both providers and do not seed the shared remote action cache.

`initial` means the first invocation in this workflow run, **not necessarily a
cold build**. `warm` is the immediate same-server repeat. To measure fully cold
builds separately, use fresh provider workspaces with local/repository caches
and remote cache reads disabled on both; do not mix those results with this
default retained-cache experiment. Same-run warm repeats are diagnostic; the
cross-run `initial` samples are the primary migration evidence.

This control covers optimized runtime/UI compilation and UI unit tests. The
production BuildBuddy action separately validates the full Bazel job, including
Docker sandbox tests. Dedicated Python/browser E2E and release-archive jobs keep
their GitHub producers.

## Collect and compare

Each provider exports `results.json`, four Bazel profiles, four logs and links
to the individual BuildBuddy invocations. JSON includes per-command wall time,
exit status, numeric Bazel metrics (including analysis and cache statistics when
emitted), commit, setup time and configuration. Failed commands stop the run,
retain failure measurements and fail the provider check. Raw build event streams
are temporary and deleted: they can contain credential-bearing command lines.

Download the GitHub `linux-pilot-github-*` artifact and the BuildBuddy step's
artifacts into separate directories, then compare all matching results:

```bash
python3 scripts/ci/linux_pilot.py compare \
  /path/to/github-run-1/results.json /path/to/buildbuddy-run-1/results.json \
  /path/to/github-run-2/results.json /path/to/buildbuddy-run-2/results.json
```

The report groups matching commits and configuration fingerprints, rejects
failed/unmatched runs, and reports median, nearest-rank p95 and percentage
reduction for build + test command time. It includes sample counts; a p95 from
five samples is just their maximum, so collect more before treating tails as
stable. Different native package versions require aligning/reseeding runners
before comparison.

Command time excludes queueing, checkout, GitHub cache restore/save, VM snapshot
overhead, setup and artifact upload. Record provider job duration separately
from each UI for **end-to-end PR feedback**, and record billed resource/caching
costs. Setup time in JSON is only native prerequisite setup, not total job time.

Consider moving Linux jobs only after repeated cross-run improvements in both
median and tail feedback time at acceptable cost (a proposed threshold is at
least 30%). Keep macOS releases and publishing on GitHub throughout this pilot.
See [the measured assessment](./03-ci-execution.md) for repeated timings, source
probes, and the limits of the resulting migration recommendation.

## CPU scaling experiment

The matched-provider pilot remains fixed at four CPUs. A separate
`BuildBuddy Linux CPU sweep` action runs only on the explicit
`codex/bb-cpu-sweep` branch. It requests 16 CPUs and 16 GB RAM and invokes:

```sh
python3 scripts/ci/linux_cpu_sweep.py --output /tmp/cpu-sweep --cpus 4 8 16
```

The sweep records the VM's available CPU affinity, CPU model, memory, and any
exposed cgroup limits. It excludes one seed run, then tests ascending and
descending CPU counts on that same VM. Every sample appends a unique temporary
comment to `src/lib.rs` to force Rust compilation; the original bytes are
restored even if the Python runner fails. Each sample includes the production
binary/frontend build, UI unit tests, and immediate warm repeats. Affinity and
Bazel `--jobs`/`--local_resources=cpu` change together; the 4 GiB JVM heap and
12 GiB action-memory budget stay fixed. The existing RocksDB Cargo build-script
`NUM_JOBS=4` annotation is unchanged, so this sweep does not tune cold C++
compilation. Counts above the VM's available affinity are recorded as skipped rather than silently running with fewer CPUs.

For this tuning pass, prefer the smallest tested CPU count whose median source-
edit build/test time is within 10% of the fastest tested count. A 32-CPU request
was also tested and rejected by BuildBuddy: the Impala organization has a limit
of **16 requested vCPUs per runner**. The advertised free remote-build core
allowance is distinct from this per-runner limit.
Two samples per count detect large changes; they do not establish a stable tail.

`cpu-sweep.json` records per-size medians/ranges and points to each sample's
JSON, profiles and logs. The per-sample `requested_cpu` records the VM reservation;
`cpu_limit` and `configuration.cpu_affinity_count` record the tested cap. This is
a within-VM scaling experiment, not an end-to-end comparison of separately sized
runners. The source edit is a no-op input-invalidation probe; real PR changes,
queue latency and RBE scaling remain separate measurements.

The completed sweep selects **16 CPUs / 16 GB** for this workload: median source-
edit build/test time falls from 267.12 seconds at four CPUs to 183.82 at eight
and 145.10 at sixteen. The [assessment](./03-ci-execution.md#buildbuddy-cpu-scaling)
and [evidence](../../bench/linux-ci-cpu-sweep-2026-11-15.json) record the result.
Use `linux_pilot.py run --cpus 16` with a matching 16-CPU Workflow reservation
for performance-focused runs. The matched-provider command continues to default
to four CPUs; its strict comparator rejects unequal CPU configurations.

See [the migration path](./05-linux-ci-migration.md) for the proposed rollout
and the prerequisite for retrying 32 vCPUs.

References: [Workflows setup](https://www.buildbuddy.io/docs/workflows-setup/),
[runner resources and images](https://www.buildbuddy.io/docs/workflows-config/),
[artifact collection](https://www.buildbuddy.io/docs/remote-runner-features/).
