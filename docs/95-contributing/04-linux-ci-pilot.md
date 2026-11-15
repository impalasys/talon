# Linux CI pilot

The repository defines an additional **BuildBuddy Linux pilot** check in
`buildbuddy.yaml` and a **GitHub Linux pilot** baseline in
`.github/workflows/buildbuddy-linux-pilot.yml`. Both trigger on pushes to `main`
and `codex/bb-pilot-*` branches. The GitHub baseline also supports manual
dispatch. Keep existing required checks, macOS releases, image publishing,
registry uploads and deployments on GitHub during this experiment.

## Enable and run

1. In the [BuildBuddy Workflows UI](https://app.buildbuddy.io/workflows/), select
   the Talon organization and link `impalasys/talon` using the GitHub app. Use the
   checked-in `buildbuddy.yaml`, rather than enabling the default `//...` config.
   The config is inert until the repository is linked and the config is pushed.
2. The existing GitHub `BUILDBUDDY_API_KEY` secret enables main/manual baseline
   runs. For automatic pilot branch pushes, add
   `BUILDBUDDY_READ_ONLY_API_KEY` with **server-enforced read-only cache access**
   to the same BuildBuddy organization/cache. Do not reuse the writer key.
   A missing key fails the baseline with an explicit setup message instead of
   silently comparing different remote cache configurations.
3. Push a `codex/bb-pilot-*` branch to run both providers against the identical
   pushed commit, without synthetic PR merges. Opening a PR on that branch is
   fine, but PR events do not dispatch extra pilot jobs. Fork pushes do not
   trigger this repository's pilot. Seed `main` before evaluating branch snapshot
   reuse, then use a Rust edit and a UI-only edit on pilot branches.
4. Repeat each revision at least five times: rerun the GitHub pilot workflow and
   the BuildBuddy action from their UIs. Verify the resolved commit on each run.
   For a manual GitHub run, select the same pilot branch/ref. Do not dispatch
   `ci.yml`, whose manual entry point is the cache probe.

## Workload

Both providers run `scripts/ci/linux_pilot.py` on Ubuntu 24.04 x86-64, with Bazel
from `.bazelversion`, LLVM 18, four CPU slots and a 12 GiB local action-memory
budget on a 16 GiB runner. BuildBuddy requests four CPUs and 16 GiB explicitly,
matching the public repository's standard GitHub runner. It requests 40 GiB disk
for native compilation; disk capacity and CPU models can differ. The shared
setup script only installs missing native prerequisites. Exact compiler and
native package versions are included in the comparison fingerprint.

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

The pilot covers optimized runtime/UI compilation and UI unit tests.
Docker/Testcontainers, browser E2E and release-artifact correctness remain
covered by existing GitHub CI and need separate compatibility trials before
moving those jobs.

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
No provider speedup has been established by adding this configuration.

References: [Workflows setup](https://www.buildbuddy.io/docs/workflows-setup/),
[runner resources and images](https://www.buildbuddy.io/docs/workflows-config/),
[artifact collection](https://www.buildbuddy.io/docs/remote-runner-features/).
