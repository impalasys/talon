# CI execution: GitHub Actions and BuildBuddy

Assessment updated 2026-11-15: the Linux pilot now has repeated, matched
measurements. The controlled workload finishes sooner on BuildBuddy, primarily
because it avoids GitHub cache restoration and native prerequisite setup.
A CPU scaling follow-up on a 16-CPU / 16-GB BuildBuddy VM found the source-edit
workload is 46% faster than the four-CPU cap on the same VM. macOS releases and
publishing remain on GitHub Actions. See the results below before generalizing
to Docker-backed or full-pipeline CI.

The first migration step moves the **Bazel build/test job for same-repository
PRs** to BuildBuddy at 16 CPUs / 32 GB RAM, behind GitHub's existing `Bazel`
check. [Full validation](./05-linux-ci-migration.md#first-cutover-configuration)
passed all 45 uncached test targets, including enabled Docker sandbox tests, and
the complete production/SDK/UI/docs/chat build. Main and forks keep GitHub
execution; macOS releases, publishing and the other Linux jobs retain their
GitHub producers. This is separate from the historical timing experiments below.

The [Linux pilot runbook](./04-linux-ci-pilot.md) describes the checked-in
side-by-side configuration, activation and measurement procedure.

## Controlled Linux results (2026-11-15)

Five sequential successful runs per provider used commit `12d5137a0caf` after a
separate configuration/seed run. Every sample passed the strict comparison:
Ubuntu 24.04.5, Bazel 9.3.2, identical versions of 14 pinned native packages,
four CPUs enforced by affinity (including retained Bazel workers), a 4 GiB JVM
heap and a 12 GiB local action-memory budget. Both used plain Bazel, the same
remote-cache endpoint with uploads disabled, and local execution (RBE disabled).
The workload builds `//:production_binaries` and `//ui:dist`, tests
`//ui:unit_tests`, then immediately repeats both commands.

| Measurement | GitHub median | BuildBuddy median | Median reduction |
| --- | ---: | ---: | ---: |
| Request/queue to success, including setup | 194.0 s | 62.0 s | 68.0% |
| Reported job/workflow duration | 190.0 s | 60.0 s | 68.4% |
| First build + test command wall time | 45.23 s | 43.26 s | 4.4% |
| Immediate warm build + test command wall time | 15.12 s | 13.26 s | 12.3% |

Request-to-success ranged from **178–197 seconds on GitHub** and **59–64 seconds
on BuildBuddy**. With five samples, the nearest-rank p95 is simply the maximum;
these data do not establish a stable population tail. Request-to-success uses
GitHub run creation through job completion, and BuildBuddy's GitHub commit-status
`Queued...` timestamp through `success`, so queue/startup time is included.
Reported job/workflow durations exclude that queue interval.

The GitHub median `setup-bazel` step was 76 seconds, native prerequisite setup
was 38 seconds, and checkout was 5 seconds. BuildBuddy's median native setup was
1.19 seconds. Most of the measured latency improvement is preparation/cache
movement, rather than compiler throughput. Tests can legitimately be cached on
these unchanged revisions; this is a retained-cache experiment, not a cold
compilation or five uncached test-suite executions.

| Sample | GitHub queue-to-success | BuildBuddy queue-to-success | GitHub evidence | BuildBuddy evidence |
| --- | ---: | ---: | --- | --- |
| 1 | 196 s | 59 s | [Run 1](https://github.com/impalasys/talon/actions/runs/43773120947) | [Run 1](https://impalasys.buildbuddy.io/invocation/ab7a5b8d-9205-456a-92b5-56aebc6b1cf5?queued=true) |
| 2 | 178 s | 64 s | [Run 2](https://github.com/impalasys/talon/actions/runs/43773453721) | [Run 2](https://impalasys.buildbuddy.io/invocation/902c39ab-69cb-4eae-adf7-2aaf74ba6608?queued=true) |
| 3 | 194 s | 63 s | [Run 3](https://github.com/impalasys/talon/actions/runs/43773752957) | [Run 3](https://impalasys.buildbuddy.io/invocation/b01c505c-61ce-482d-9eae-7d746ac8f497?queued=true) |
| 4 | 192 s | 61 s | [Run 4](https://github.com/impalasys/talon/actions/runs/43774085831) | [Run 4](https://impalasys.buildbuddy.io/invocation/a03d584a-2e5f-4290-8284-8d53c06c2d9b?queued=true) |
| 5 | 197 s | 62 s | [Run 5](https://github.com/impalasys/talon/actions/runs/43774403520) | [Run 5](https://impalasys.buildbuddy.io/invocation/51472017-667f-45e8-b0f6-cdbb0747860b?queued=true) |

### Source-invalidation probes

After the repeats, each provider ran one comment-only Rust edit (`98282a8c1092`),
then one comment-only UI edit retaining that Rust comment (`65e5cd117038`). Both
providers passed both probes with the same configuration fingerprint. These
force input invalidation without changing behavior; they are single samples,
not representative distributions of functional changes. The temporary comments
were removed after measurement.

| Probe | GitHub request to success | BuildBuddy request to success | Reduction | First build + test, GitHub / BuildBuddy |
| --- | ---: | ---: | ---: | ---: |
| Rust source edit | [460 s](https://github.com/impalasys/talon/actions/runs/43775098473) | [353 s](https://impalasys.buildbuddy.io/invocation/af4fd5a7-099c-428a-a417-5f33dd8c168a) | 23.3% | 339.79 / 334.49 s |
| UI edit after Rust | [499 s](https://github.com/impalasys/talon/actions/runs/43775850291) | [418 s](https://impalasys.buildbuddy.io/invocation/03a418b2-77d2-4d14-bb47-27d5eb42a176) | 16.2% | 354.26 / 403.16 s |

The Rust compilation work took nearly the same time on both providers. Both
also rebuilt the Rust library and production binaries on the subsequent UI-only
edit. GitHub's immutable restored snapshot explains why the preceding Rust edit
was absent there; the cause on BuildBuddy remains unresolved. Treat the second
probe as a cache/invalidation test, not a clean frontend benchmark. BuildBuddy's
command time was slower in that probe, while its reduced preparation still
improved total turnaround.

The [machine-readable evidence](../../bench/linux-ci-pilot-2026-11-15.json)
contains the configuration, per-phase metrics, run links, timing boundaries,
excluded seeds, and source probes. Earlier exploratory runs are not included.

### BuildBuddy CPU scaling

A follow-up allocation test accepted a 16-CPU / 16 GB Workflow on the existing
free-tier account. A 32-CPU request was rejected before startup with:

> Runner requests 32 vCPUs, exceeding your organization's per-runner CPU limit of 16 vCPUs.

This establishes Impala's current limit of **16 requested vCPUs per runner**.
The pricing page's remote-build core allowance is a different limit; it does not entitle a
single Workflow VM to 80 CPUs. The rejected request is recorded in the CPU-sweep
evidence and the
[commit status](https://api.github.com/repos/impalasys/talon/commits/db71e0f0/status).

On one 16-CPU VM, the experiment excludes a cold seed and forces Rust source
invalidation at caps of 4, 8 and 16 CPUs, then repeats in reverse order. It holds
RAM, heap and the action-memory budget fixed and changes affinity plus Bazel's
CPU scheduling limits together. This measures within-VM CPU scaling, not total
turnaround across independently allocated runners. All six samples passed,
with two samples per count:

| CPU cap | Source-edit build + test median | Range | Reduction vs 4 CPUs |
| --- | ---: | ---: | ---: |
| 4 | 267.12 s | 265.98–268.27 s | — |
| 8 | 183.82 s | 176.89–190.75 s | 31.2% |
| 16 | 145.10 s | 143.35–146.85 s | 45.7% |

**Select 16 CPUs and 16 GB RAM for a performance-focused BuildBuddy Linux run.**
It improves the eight-CPU median by another 21.1%; eight CPUs is outside the
preselected 10% margin from the fastest median. Keep the JVM heap at 4 GiB and
Bazel's action-memory budget at 12 GiB, with `--jobs=16`,
`--local_resources=cpu=16`, and a matching 16-CPU affinity cap. This is the fastest
tested size and the organization's maximum accepted CPU request.

The VM exposed 19 CPUs and approximately 16.4 GiB RAM; the measured workload was
restricted to at most 16 CPUs. Its CPU model was AMD EPYC. The seed took 631.58 s
including warm repeats and is excluded from the table. Each source-edit build
executed six actions; first-build counters progressed from 5 to 25, confirming
within-workflow JVM reuse. Immediate warm build/test medians were 7.78, 6.18 and
5.91 seconds at 4, 8 and 16 CPUs, respectively.

This is a small, within-VM experiment with cached UI tests and comment-only Rust
edits. It establishes useful CPU scaling for this workload, not a 46% reduction
in complete PR latency. The existing RocksDB `NUM_JOBS=4` annotation was unchanged,
so it does not evaluate higher native C++ build-script parallelism. The successful measurement
series stops at 16 CPUs; a subsequent 32-vCPU retry was also rejected before
allocation, as recorded in the migration path.

The [successful sweep](https://impalasys.buildbuddy.io/invocation/c9ebad51-8c55-4a86-9f1c-36df89d75e30)
and [CPU-scaling evidence](../../bench/linux-ci-cpu-sweep-2026-11-15.json) include
all per-sample measurements, native package/configuration fingerprints, the
excluded seed and the rejected 32-CPU request. The original four-CPU matched
pilot remains available for controlled provider comparisons.

### Limits of the controlled timing comparison

- The native userspace matches, but the cloud hosts are not identical. Guest
  kernels differ, and exposed RAM was approximately 15.3 GiB on GitHub versus
  16.4 GiB on BuildBuddy. Four-CPU affinity, JVM heap and action-memory budgets
  match; there is no shared hard memory cgroup limit or matched CPU model.
- BuildBuddy exposed seven vCPUs for the four-CPU reservation. Exploratory runs
  before affinity enforcement are excluded from the decision dataset, as are
  runs with differing Ubuntu labels or client/startup settings.
- The first build in each final BuildBuddy run reported `numBuilds=1`; a retained
  Bazel JVM/analysis cache across workflows was **not demonstrated** under this
  heap/startup configuration. Immediate within-run reuse was measured. The data
  must not be attributed to the advertised warm-JVM mechanism without further
  investigation of the runner's startup/cleanup behavior.
- The GitHub baseline's upstream `setup-bazel@0.19.2` disk-cache key hashes MODULE
  and BUILD files. Exact hits are immutable, so source-only edits do not refresh
  that snapshot. This can cause repeated compilation on subsequent source edits.
  It is a tunable GitHub cache-policy issue, not evidence of an intrinsic
  compiler-speed advantage for BuildBuddy. See the
  [pinned upstream implementation](https://github.com/bazel-contrib/setup-bazel/blob/0.19.2/dist/main/index.js).
- Docker/Testcontainers, browser E2E, full runtime tests, artifact handoff to
  release jobs, and paid-plan costs are outside the matched timing workload.
  Later full-job qualification establishes Docker compatibility, not a matched
  provider performance comparison. The
  current full PR critical path is not proven to improve by the same percentage.

See [the staged migration path](./05-linux-ci-migration.md) for the full-job
qualification, GitHub status bridge, cache ownership, artifact dependencies,
32-vCPU retry, and later modularization investigation.

## Earlier measurements

The [successful baseline run](https://github.com/impalasys/talon/actions/runs/43135197791)
at commit `b08bf05e`, before the packaging cleanup, recorded these step durations:

| Step | Earlier cold run (`b08bf05e`) | Cleanup run with GitHub disk-cache reuse (`13523444`) |
| --- | --- | --- |
| Build and test all Bazel targets | 45m 56s | 3m 11s |
| Python E2E, including building its binaries | 36m 55s | 6m 36s |
| Live UI E2E, including building its backend | 30m 00s | 1m 52s |
| Linux release archives | 29m 39s | 1m 03s |
| macOS ARM64 release archives | 41m 15s | 2m 10s |

These are individual step wall times, not additive pipeline times. The release
matrix waits for the Bazel, runtime-image and UI jobs. The PR run did not use
BuildBuddy: its writer credential is restricted to main-branch pushes. These
numbers therefore cannot establish the performance of GitHub Actions *with*
BuildBuddy caching. Repository downloads were cached; native compilation still
accounted for much of the work in the earlier run.

The [cleanup run](https://github.com/impalasys/talon/actions/runs/43372372395)
used GitHub's existing per-job disk caches, not BuildBuddy. The commits differ:
packaging changed, two helper tests were replaced by actual release-archive
verification, and the default unit-target count changed from 42 to 40. This is
observational evidence of cache reuse, not a controlled provider benchmark.
It shows why the cold times alone are not a reason to replace the orchestrator.

Separately, [authenticated cache verification passed](https://github.com/impalasys/talon/actions/runs/43372492922).
A fresh input forced an upload, then an independent output base with disk caching
disabled obtained **1 remote cache hit** and matching output. The
[BuildBuddy read invocation](https://app.buildbuddy.io/invocation/929f75b6-0433-4945-875f-6f87c291ba77)
records the read. This proves cache connectivity and read/write behavior, not
production-build performance or Workflows execution speed.

## GitHub cache storage

Pre-merge validation also exposed cache eviction around the repository's default
storage allowance: a single default-test snapshot occupied 4.56 GB, and recently
saved native release snapshots disappeared while unrelated jobs saved duplicate
snapshots. This caused repeated cold native builds even when the source changes
did not affect runtime code.

The shared setup action now uses upstream `setup-bazel` cache ownership controls.
Only the full Bazel job writes `talon-build`; the other Linux build/test workflows
restore that bucket with `cache-save: false`. Native release jobs write
their existing separate bucket, isolated by upstream OS/architecture keys, to
retain warm release outputs. Native SDK publication
restores the release bucket. The shared producer tests with
`--build_tests_only`, then builds runtime binaries, all SDKs/examples, the hook
installer, frontend/chat and documentation. Image and runtime-release assembly
stay in their dedicated jobs so their large outputs do not inflate this shared
compilation snapshot. Rust already uses `debuginfo=0` for these compilation
modes; no additional compiler flag or debug-behavior change is needed.

Verify the resulting snapshot sizes against the repository allowance and rerun
the same revision to confirm restored action-cache hits in both native release
jobs. This policy uses the existing cache allowance and credential boundaries.

## Where Workflows could be faster

Both providers can reuse the same BuildBuddy action cache when toolchains,
inputs and action environments match. BuildBuddy Workflows additionally keeps
warm workspaces, Bazel JVM/analysis state and local outputs using Linux VM
snapshots. It also places runners close to its cache and remote executors.
Those advantages can reduce analysis, repository preparation and artifact
transfer costs beyond remote caching alone. See the
[remote runner architecture](https://www.buildbuddy.io/docs/remote-runner-introduction/).

Moving orchestration alone does not make a cache-missing Rust or RocksDB build
faster. CPU, memory, compiler parallelism and optionally remote execution still
matter. BuildBuddy documents default Linux Workflow resources of 3 CPUs, 8 GB
RAM and 20 GB disk; a meaningful comparison must request comparable resources
and a compatible Ubuntu 24.04 image. See
[Workflow configuration](https://www.buildbuddy.io/docs/workflows-config/).

## Constraints for Talon

| Area | Current GitHub Actions setup | BuildBuddy Workflows evaluation |
| --- | --- | --- |
| Linux compilation | Bazel graph and native prerequisites verified | Good pilot candidate; match compiler/toolchain and resource settings |
| Docker-backed Rust/Python/browser tests | Verified with Docker and Testcontainers | Verify daemon access, networking, services, cleanup and screenshots in the runner |
| macOS ARM64 releases | Hosted `macos-15` runner verified | Documentation requires a dedicated self-hosted Mac executor, or a separate arrangement with BuildBuddy |
| Publication/deployment | Existing GitHub environments, GHCR token, PyPI publishing and deployment integrations | Keep these in GitHub initially; transferring artifacts and credentials is additional migration work |
| Warm state | Ephemeral runner plus disk/repository caches and optional remote cache | Persistent runner state may save startup/analysis/transfer time |

The macOS requirement is documented in the
[Mac configuration](https://www.buildbuddy.io/docs/workflows-config/#mac-configuration)
and [runner recycling](https://www.buildbuddy.io/docs/remote-runner-introduction/#runner-recycling-macos-only)
guides. Check the organization's actual plan and executor/cache-transfer costs
against [current pricing](https://www.buildbuddy.io/pricing/); no cost saving is
assumed here.

## Decision and next steps

**Run same-repository PR Bazel builds/tests on BuildBuddy at 16 CPUs / 32 GB.**
Keep the controlled pilot for measuring further migration. The matched four-CPU experiment
shows 68% lower unchanged-revision turnaround and 16–23% lower turnaround on the
two source probes. The later CPU sweep shows an additional 46% reduction in
source-edit command time at 16 versus four CPUs on the same VM. More available
compute is a material benefit for this workload; equal-CPU results alone do not
capture it.

The larger allocation strengthens the case for moving Linux compilation to
BuildBuddy. The complete Bazel job, including Docker sandbox tests, now passes on the
32-GB full-job configuration. Matched full-PR latency, artifact handoff and costs
remain unmeasured. Cross-workflow JVM reuse also
remains unproven. The scaling percentage must not be applied directly to the
older GitHub end-to-end timings, which include different startup/cache state.

After the first Bazel cutover, evaluate further Linux migration:

1. Refresh GitHub's disk snapshot after source changes, using a bounded cache
   rotation policy, and compare against that improved baseline.
2. Resolve BuildBuddy's JVM restart/recycling behavior and the Rust rebuild on
   the UI-only change; verify retained analysis and output reuse from counters
   and action diagnostics rather than assuming them.
3. In a subsequent evaluation, use the selected 16-CPU candidate for representative
   Rust and UI changes, then run Docker/Testcontainers,
   runtime and browser suites. Measure complete PR feedback time, failure
   artifacts, artifact handoff and cost. A fresh-cache comparison remains
   separate work if cold-run performance matters to the decision.

GitHub baselines remain manual on main; no new API key was created. macOS
releases, publishing, deployments and existing required checks stay on GitHub
Actions. Remote execution remains disabled so its performance can be evaluated
separately from the orchestrator.
