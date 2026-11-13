# CI execution: GitHub Actions and BuildBuddy

Recommendation (2026-11-12): keep GitHub Actions as the CI orchestrator and
verify BuildBuddy remote caching first. Pilot BuildBuddy Workflows for Linux
builds before deciding on a broader migration. There is no measured
BuildBuddy-versus-GitHub speedup yet.

## What we measured

The [successful baseline run](https://github.com/impalasys/talon/actions/runs/43135197791)
at commit `b08bf05e`, before the packaging cleanup, recorded these step durations:

| Step | Duration |
| --- | --- |
| Build and test all Bazel targets | 45m 56s |
| Python E2E, including building its binaries | 36m 55s |
| Live UI E2E, including building its backend | 30m 00s |
| Linux release archives | 29m 39s |
| macOS ARM64 release archives | 41m 15s |

These are individual step wall times, not additive pipeline times. The release
matrix waits for the Bazel, runtime-image and UI jobs. The PR run did not use
BuildBuddy: its writer credential is restricted to main-branch pushes. These
numbers therefore cannot establish the performance of GitHub Actions *with*
BuildBuddy caching. Repository downloads were cached; native compilation still
accounted for much of the work.

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

## Decision and measurement plan

1. Run the maintainer-triggered cache verification in `ci.yml`. It proves an
   authenticated upload and an independent remote-cache read; it is not a build
   performance benchmark. The second build uses a fresh output base and no
   GitHub disk cache.
2. After merging, populate the main-branch cache and measure subsequent runs.
   PRs currently use GitHub caches only. Giving trusted PRs BuildBuddy cache
   access requires a separately scoped read-only key and workflow wiring;
   passing the writer key with uploads disabled is not equivalent protection.
3. Pilot `bazel build //:production_binaries` on a BuildBuddy Linux Workflow
   and on GitHub Actions with the same revision, Bazel/toolchain versions,
   feature flags, cache endpoint and CPU/RAM budget. Use Ubuntu 24.04 and the
   checked-in native prerequisites. Then test Docker-backed suites separately.
4. Compare fresh-runner cold builds, repeated warm builds, a Rust source edit,
   and a UI-only edit. Separate compilation from analysis, fetching, downloads,
   tests and cache/snapshot overhead. Record end-to-end latency, cache hits,
   resource usage, cost and artifact correctness; repeat enough to distinguish
   persistent improvements from runner noise.
5. Move Linux build/test orchestration only if that pilot improves real PR
   latency at acceptable cost. Retain GitHub release/publishing jobs until
   macOS execution and artifact handoff are proven. If cache misses remain
   compute-bound, evaluate BuildBuddy RBE independently of the orchestrator.

A full CI migration or a claim that Workflows is faster would be premature
without that comparison. The current repository does not enable BuildBuddy
Workflows or remote execution.
