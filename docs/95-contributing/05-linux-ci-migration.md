# Linux CI migration path

Status: 2026-11-15. This is the implementation sequence for moving Linux
compilation to BuildBuddy while keeping macOS releases and publishing on GitHub
Actions. The [assessment](./03-ci-execution.md) contains measured results; the
[pilot runbook](./04-linux-ci-pilot.md) describes the existing experiment.
The first cutover routes same-repository PR Bazel compilation/tests through
BuildBuddy. Main and fork PRs keep GitHub execution; branch rules are unchanged.
Image/E2E jobs, native archives, macOS releases and publishing retain their GitHub
producers. Later phases below describe further work, not completed migrations.

## First cutover configuration

The full action requests 16 CPUs, **32 GB RAM** and 80 GB disk, with a 4 GiB JVM
heap and 20 GiB action-memory budget. The earlier smaller-workload CPU sweep used
16 GB RAM; its timing is not the full-job timing. The runner starts/verifies
Docker, enables the opt-in sandbox tests, disables cached test results, runs all
Bazel tests, then builds the complete existing production/SDK/UI/docs/chat target
set. Each phase emits profiles, logs and sanitized results.

The GitHub `bazel` job keeps its ID/name and dependent release jobs. It uses a
read-only GitHub token to validate the BuildBuddy status, exact head/merge trees,
and current PR revision. No BuildBuddy credential is transferred to GitHub.
Set repository variable `BUILDBUDDY_LINUX_PR_BUILDS=false` and rerun CI to restore
GitHub execution for trusted PRs; the unset default enables BuildBuddy. The
[operations runbook](./04-linux-ci-pilot.md#operate-the-pr-gate) has the command.

Validation evidence is added alongside the assessment after the complete job
passes. Two compatibility fixes keep actions independent of writable user homes:
Bazel's pnpm now reads its version from `package.json`, and the documentation
build explicitly disables Astro telemetry.

## Runner choice and the 32-vCPU retry

Use **16 requested CPUs / 16 GB RAM** as the current candidate. The Rust source
edit sweep measured 267.12, 183.82 and 145.10 seconds at CPU caps of 4, 8 and 16,
respectively. These are command times on one VM, not full PR latency.

The requested 32-vCPU retry at commit `379b81c8` was rejected before allocation:

> Runner requests 32 vCPUs, exceeding your organization's per-runner CPU limit of 16 vCPUs.

The [retry status](https://api.github.com/repos/impalasys/talon/commits/379b81c80d7cffb60eb32b65dbca2f34890f5206/status)
and [recorded evidence](../../bench/linux-ci-cpu-sweep-2026-11-15.json) establish
that the account limit has not changed. There are no new 32-CPU timings.
BuildBuddy must raise Impala's per-runner limit to at least 32 before that
comparison can run; no plan upgrade or support message was submitted.

Once allowed, use the prepared `codex/bb-cpu-sweep-32` experiment branch to compare
caps of 16 and 32 on the same 32-CPU VM. Keep RAM at 16 GB, JVM heap at 4 GiB,
action memory at 12 GiB, local execution, and the same workload. Exclude the seed,
require two successful samples per cap, and reject skipped CPU sizes. Prefer 16
unless 32 improves the median by more than 10%. If memory limits cause failures,
measure a separate RAM configuration rather than combining unlike samples.
The migration can proceed with 16 CPUs independently of that quota change.

## What moves first

| Current producer | First migration step | Later work |
| --- | --- | --- |
| `ci.yml` / `bazel` (`Bazel` check) | Full BuildBuddy shadow, then first cutover | Keep its GitHub job ID as a status bridge and fork fallback |
| `runtime_image`, `acp_harness_image`, `ui` | Keep on GitHub | Move build/verification only after an OCI artifact handoff works; publishing stays on GitHub |
| `python_e2e` | Keep on GitHub | Qualify Docker networking and Testcontainers on BuildBuddy, then move separately |
| `sightline_e2e_screenshots` | Keep on GitHub | Preserve screenshots, previews and PR reporting before changing its producer |
| `binary_artifacts` Linux/macOS matrix | Keep on GitHub | Consider Linux compilation offload only after verified artifact transfer; retain macOS |
| `publish.yaml`, Cloudflare, Chromatic, CLA, PR reports | Keep on GitHub | No publishing or deployment credential migration |
| `bench-1vcpu.yml` | Keep its current environment | Its runtime performance comparison is separate from CI runner sizing |

Two current dependencies determine the order:

- `binary_artifacts.needs` contains `bazel`, `runtime_image` and `ui`. A GitHub
  `needs` edge does not wait for an external BuildBuddy status.
- `pr-report.yml` listens for `Talon OSS CI` completion and downloads
  `sightline-e2e-screenshots` and `sightline-e2e-dark-preview` from that GitHub run.
  BuildBuddy artifacts alone do not satisfy that consumer.

## PR 1: qualify the complete Bazel job

Merge the additive pilot first. Add a distinct `BuildBuddy Linux build/test`
shadow action at 16 CPUs, using the same pinned Linux prerequisites and feature
settings. Reproduce the current Bazel job's full coverage:

```sh
bazel test --build_tests_only //...
bazel build //:binaries //:production_binaries //sdk/... \
  //tools/install-hooks/... //ui:dist //docs-site:dist //packages/talon-chat/...
```

Apply the explicit CPU, memory, compiler and cache options from the pilot to both
commands. Keep RBE disabled during orchestration qualification. The smaller
pilot's production binaries and UI unit tests do not replace this target set.

**Docker is required even for this first cutover:** `//:talon_test` is tagged
`requires-docker` and `no-remote-exec`. Prove daemon access, container lifecycle,
networking, cleanup and failure logs before declaring full Bazel parity. A
non-Docker subset may be useful for diagnosis, but cannot pass the full check.
Measure peak memory too; the 16-GB result only establishes capacity for the
smaller workload.

Use pushes in the linked base repository for initial shadow runs. This tests
exact branch commits without automatically running fork PRs on BuildBuddy.
Keep GitHub's full job active and pilot-branch baselines manual. Retain one
canonical GitHub disk-cache writer; fix bounded snapshot rotation separately
from provider selection so source-only edits can refresh the immutable cache
keys. No new GitHub
BuildBuddy key is needed for this stage.

Qualify representative Rust, UI, SDK, schema and configuration changes. Collect
at least ten paired full-job samples, queue-to-completion time, cache hit rates,
peak memory and snapshot transfer usage. Exercise intentional test failure,
missing/cancelled status and an updated PR head. Promote on full coverage and a
repeatable latency improvement; the existing proposed threshold is 30% median
Linux build/test feedback-time reduction with no material tail regression. Also measure overall
PR completion, because image jobs and macOS can remain the critical path.

Investigate snapshot reuse as part of qualification. BuildBuddy documents a
`first-non-default-ref` default save policy: later pushes can reuse the first
branch snapshot rather than the preceding run. This is a plausible explanation
for the Rust rebuild after the UI-only edit, not a verified diagnosis of that
run. Inspect effective properties and saved/resumed snapshot IDs first. Compare
an explicit snapshot policy using the same change sequence and its transfer
usage; changing properties itself invalidates the snapshot key. See
[runner snapshot policies](https://www.buildbuddy.io/docs/remote-runner-introduction/#snapshot-save-policy).

## PR 2: replace compilation behind the existing GitHub gate

After qualification, preserve the GitHub `bazel` job ID and `Bazel` check name.
For same-repository PRs, replace its compilation steps with a bounded wait for
the distinct BuildBuddy build/test status. For **main and fork PRs**, retain the
existing GitHub compilation/test steps initially. This keeps the release
matrix's `needs: bazel` relationship and provides a single check for both paths.

Main stays on GitHub at this stage because `bazel` is the sole writer of the
`talon-build` disk-cache bucket. Remaining image and E2E jobs restore that bucket
without saving. Removing its writer could make those consumers slower and erase
the PR improvement. BuildBuddy also runs on main to seed its own snapshots.
Remove the duplicate main producer only after remaining consumers have moved or
a compatible replacement cache path is demonstrated; avoid adding competing
writers or putting large image-assembly outputs back into the shared snapshot.

Use the following revision contract for the bridge implementation:

1. BuildBuddy's initial production action uses base-repository **push** events,
   so the built revision is the branch head, not an independently generated PR
   merge commit. If PR triggers are introduced later, explicitly review their
   `merge_with_base` behavior.
2. For a PR, wait on `pull_request.head.sha`. A later main cutover would use
   `github.sha`, after replacing the GitHub cache producer. Validate the status
   context and BuildBuddy publisher. Missing, pending past timeout, error,
   cancelled or superseded results must not become success.
3. Before accepting a PR result, require the tested head tree to equal the
   GitHub merge tree used by retained jobs. An outdated branch must be updated
   and rebuilt rather than passing on an old head status. Recheck that the PR
   still points to that head before completing the bridge.
4. Require up-to-date branches and the unified `Bazel` check when enabling merge
   enforcement. At inspection time, GitHub reports no classic protection on
   main and no repository rulesets; current green checks are not equivalent to
   configured enforcement. Do not globally require the BuildBuddy-only context,
   which would block the fork fallback.

The bridge reads commit statuses with GitHub's read-only token permissions; it
does not require a BuildBuddy credential. Verify this contract with a changed
base branch, stale success, failed build, fork PR and new head before removing
GitHub compilation on the trusted path. BuildBuddy's
[trigger and merge documentation](https://www.buildbuddy.io/docs/workflows-config/#merge-with-base)
describes why default PR behavior needs deliberate handling.

Warm the production action on main as well as branches. Keep the existing
GitHub cache writer credential restricted to main. Keep publishing tokens and
release environments on GitHub. Audit BuildBuddy's trusted-run secret behavior
before broadening its triggers; see [secrets](https://www.buildbuddy.io/docs/secrets/).

Rollback is one workflow change: restore the original compilation/test steps
inside `bazel`, retain its ID/name, and return the BuildBuddy action to shadow
status. Keep that path available until cutover is established.

## Later PRs: containers and artifact handoff

Move Python E2E only after its exact command passes on BuildBuddy with the same
Testcontainers environment and failure diagnostics. Move browser E2E last among
the tests: first preserve its screenshot artifact names, retention, missing-file
failures and the existing PR preview/reporting path.

Keep image publishing, native archive production and `publish.yaml` on GitHub
initially. Removing their compilation later requires a handoff keyed by commit,
platform, feature/toolchain configuration and checksums. Accept artifacts only
from a successful matching trusted invocation; never use a mutable "latest"
artifact or rebuild a different revision silently. Verify executable modes,
archive contents and existing release tests on the consuming side.

BuildBuddy supports programmatic artifact retrieval through `GetInvocation`
with artifacts included, then `GetFile`; a future bridge would need suitable
read access or a separately designed storage transfer. GitHub's artifact
actions do not directly download BuildBuddy artifacts. See
[artifact retrieval](https://www.buildbuddy.io/docs/remote-runner-features/#fetching-artifacts-programmatically).
Prepare and validate that integration before removing the GitHub producers.
macOS builds, release assembly, registry upload, signing and deployment remain
GitHub responsibilities throughout this sequence.

## Later build optimization: modularization

Treat this as a separate measured effort after the CI environment is stable.
The current root `rust_library(name = "talon")` collects almost all runtime Rust
sources, and all four production binaries depend on it. A Bazel reverse-dependency
query confirms that fan-out. Existing Rust `mod` boundaries do not create
independent compilation/cache actions.

There is also real coupling to unwind. A lexical scan finds mutual references
between `control`, `gateway`, `harness` and `worker`, including tests and cfg
branches. It is not a resolved dependency graph, so it identifies investigation
areas rather than proving a valid crate decomposition. Generated protobuf modules
live in `gateway/rpc/mod.rs`, while `build.rs` emits serde paths into
`crate::control::manifest`; moving the generated files alone would preserve a
cycle or fail to compile.

Start with these bounded investigations:

1. Profile representative edits, separating Rust library compilation, binary
   code generation/linking, analysis, fetching and snapshot movement. Audit broad
   `compile_data` inputs too: root runtime YAML/model files include inputs used
   by configuration tests. Keep required embedded files, but test whether
   test-only inputs can be attached to test targets instead of production
   compilation. This may reduce invalidation before any crate split.
2. Extract stable protocol types and their serialization helpers into a leaf
   crate, preserving wire formats, YAML/JSON behavior and temporary public
   re-exports. Give it matching Cargo and Bazel targets. Use a one-boundary spike
   before splitting the whole runtime.
3. Define shared contracts below the runtime layers, then consider storage/cloud
   adapters and harness, worker, gateway and CLI crates. Narrow each target's
   direct dependencies instead of retaining `_RUNTIME_DEPS` everywhere. Keep
   test support separate and make dependencies acyclic.
4. Repeat identical edit scenarios after each extraction. Keep it only if
   unrelated crates stay cached and median developer/CI feedback improves,
   without regressing production optimization, binary size, tests or packaging.
   Dependent binaries may still need to relink; a crate split does not eliminate
   legitimate downstream work.

RocksDB's fixed `NUM_JOBS=4` is another independent tuning opportunity for cold
builds. Evaluate that separately from crate extraction and the orchestrator so
its memory and speed effects are attributable. RBE can be considered after these
boundaries exist; more executors cannot split one monolithic Rust action into
independent crate actions.
