# CI execution: GitHub Actions and BuildBuddy

Assessment updated 2026-11-15: the Linux pilot now has repeated, matched
measurements. The controlled workload finishes sooner on BuildBuddy, primarily
because it avoids GitHub cache restoration and native prerequisite setup.
macOS releases and publishing remain on GitHub Actions. See the controlled
results below before generalizing to Docker-backed or full-pipeline CI.

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

### Limits of this experiment

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
  release jobs, and paid-plan costs are outside this measured workload. The
  current full PR critical path is not proven to improve by the same percentage.

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

**Keep the side-by-side pilot; do not move all Linux CI yet.** The unchanged
revision improves median turnaround by 68%, but the two source-invalidation
probes improve by only 16–23%, below the runbook's proposed 30% threshold. The
actual build/test work is similar or slower, cross-workflow JVM reuse remains
unproven, and Docker-backed suites and the full PR critical path are unmeasured.
This establishes a preparation-latency benefit for the measured workload, not a
blanket migration case or a cost saving.

Before deciding to move Linux orchestration:

1. Refresh GitHub's disk snapshot after source changes, using a bounded cache
   rotation policy, and compare against that improved baseline.
2. Resolve BuildBuddy's JVM restart/recycling behavior and the Rust rebuild on
   the UI-only change; verify retained analysis and output reuse from counters
   and action diagnostics rather than assuming them.
3. Repeat representative Rust and UI changes, then run Docker/Testcontainers,
   runtime and browser suites. Measure complete PR feedback time, failure
   artifacts, artifact handoff and cost. A fresh-cache comparison remains
   separate work if cold-run performance matters to the decision.

Pilot-branch GitHub baselines remain manual; no new API key was created. macOS
releases, publishing, deployments and existing required checks stay on GitHub
Actions. Remote execution remains disabled so its performance can be evaluated
separately from the orchestrator.
