# Consolidated Linux CI

Linux CI uses one BuildBuddy action: **Talon CI**, at 16 CPUs / 32 GiB RAM.
PRs build/test affected targets and their dependents after merging main; main
pushes build/test the complete Linux-compatible Bazel monorepo. The
[operations guide](./04-linux-ci-pilot.md) describes selection, coverage,
artifacts, credential boundaries and rollback.

## What moved

The separate GitHub Linux Bazel, runtime/ACP/UI image validation, Python E2E,
Sightline browser/screenshots, chat screenshots and benchmark producers are
consolidated in BuildBuddy. Docker and browser suites are no longer hidden from
ordinary discovery by `manual` tags. Documentation and release-archive tests
also participate in normal selection. Stress remains an opt-in workload.

The GitHub provider switch and old Bazel status bridge are removed. BuildBuddy
runs native PR triggers and publishes its own status. There is one automatic
BuildBuddy action, with a shared runner/cache across its build and test phases.

## What stays on GitHub

| Responsibility | Producer |
| --- | --- |
| macOS native archives and release verification | `macos-releases.yml` |
| Main Linux desktop release packaging with existing OAuth defaults | Main-only release job in `ci.yml` |
| OCI registry publishing, Cloudflare deployment, Chromatic publishing | Artifact consumers in `ci.yml`; no image/UI recompilation |
| SDK tags, releases and registry uploads | `publish.yaml`, gated on exact-commit full CI |
| CLA metadata and PR result reporting | `cla.yml` and `pr-report.yml` |

Publishing credentials stay on GitHub. An authenticated trusted-base artifact
relay transfers only the outputs of the exact successful BuildBuddy revision.
It checks the tested merge tree and hashes, and never executes downloaded PR
code. Native macOS and credential-configured desktop release packaging remain
separate from the Linux CI suite by design.

## Validation contract

Exercise both full and affected modes, including new/deleted targets,
configuration changes, an unrelated source edit, failed selection, a failing
test and stale artifacts. New ordinary targets must reach build/test execution
without registration. Confirm all promoted Docker/browser suites execute, and
that the GitHub relay can read the matching manifest/bundle and preserve
screenshots, benchmarks and publishing inputs.

Historical controlled provider and CPU measurements are preserved in the
[assessment](./03-ci-execution.md). They do not establish a full-pipeline speedup
for this broader workload. Subsequent measurements should compare identical
changes and report selection, queue time, build/test time and artifact transfer.

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
