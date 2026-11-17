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
also participate in normal selection, including the local process-restart
durability stress suite. Only native npm staging aliases remain manual.

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

GitHub release/publishing jobs retain BuildBuddy caching and build-event logs,
but the shared setup action sets
`DISABLE_COMMIT_STATUS_REPORTING=true`. Their GitHub job checks report success
or failure; only the native **Talon CI** Workflow posts a BuildBuddy commit
status. This follows the
[BuildBuddy metadata guide](https://www.buildbuddy.io/docs/guide-metadata/).

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

## Rust crate decomposition plan

The identifiers milestone below is implemented; the remaining extractions are
planned. Keep the four binary
names, public Rust paths, protobuf wire formats, manifest behavior and release
outputs stable while introducing independently compiled crates. Each new crate
gets its own `Cargo.toml`, `BUILD.bazel`, `src/lib.rs` and tests under `crates/`.
Adding BUILD files alone does not create compilation boundaries: each boundary
needs a separate `rust_library` with explicit dependencies. See the
[rules_rust library reference](https://bazelbuild.github.io/rules_rust/rust_library.html)
and [Cargo workspace reference](https://doc.rust-lang.org/cargo/reference/workspaces.html).

### Baseline and expected benefit

The [first consolidated main run](https://impalasys.buildbuddy.io/invocation/175f2d46-dc74-4a3a-8c33-420280a6e568)
at commit `26f8c106` built 390 targets and passed all 58 test targets on 16 CPUs.
Its exported profile and manifest give this baseline:

| Measurement | Time |
| --- | ---: |
| Complete build phase | 804.7 s |
| RocksDB build script, on the build critical path | 356.0 s |
| Root Talon library, on the build critical path | 89.4 s |
| Worker binary compilation/linking, on the build critical path | 68.6 s |
| Complete test phase | 808.0 s |

Action times are parts of the build and can overlap; do not add them to the
phase totals. RocksDB ran locally in this sample. A fresh or incomplete snapshot
is a possible explanation, not a verified snapshot diagnosis. The runner also
inherits `--remote_upload_local_results=false` from the comparison pilot, so
locally built outputs do not warm the shared action cache. VM snapshots remain
an independent source of cached outputs. Establish warm-cache behavior before
attributing an improvement to crate extraction.

The root `rust_library(name = "talon")` compiled 199 source files in that profile.
A reverse-dependency query confirms that all four production binaries depend on
it. Existing `mod` declarations share that compilation boundary. Smaller crates
should preserve cached compilation of unrelated components and allow independent
work to run concurrently; downstream crates and binaries may still rebuild or
relink. A crate split does not remove cold RocksDB C++ work or the integration
test stage. No speedup factor is established yet.

### Identifiers milestone

`crates/talon-identifiers` now owns resource keys and namespace helpers, with its
own Cargo manifest, Bazel library and unit-test target. Its only direct external
dependencies are `anyhow`, `sha2` and `urlencoding`; it has no runtime, RPC or
storage dependency. The existing `talon::control::keys` and
`talon::control::ns` paths re-export the same modules and types.

The eight existing helper tests moved with the implementation. A separate
integration test verifies type identity through the old/new imports and exact
storage-key encoding and parsing. Cargo workspace/all-target checks and the
root package's `aws,rocksdb` all-target check exercise both manifest paths.

A native Bazel cache-isolation probe first built `//:talon`, made a temporary
comment-only edit in `src/worker/runtime.rs`, and built it again. The second
profile recorded Talon's Rustc action (197 source files), no identifiers Rustc
action, and unchanged identifiers artifact hashes. The worker edit was restored.
This proves that a worker input change does not recompile the new leaf; it is
not a measured end-to-end speedup or a claim that downstream binaries stay cached.

### Dependency constraints found in the code

These are concrete extraction constraints, rather than a claim that the current
folders already form a valid crate graph:

- `control/resource_model.rs` imports `gateway::rpc::resources_proto`, while
  gateway code uses control-plane storage and resource interfaces.
- `gateway/rpc/mod.rs` owns generated messages, while `control/events.rs`
  includes another generated module and re-exports gateway data messages.
- `build.rs` emits serde hooks into `crate::control::manifest`. Moving generated
  code without moving those pure serialization helpers would create a reverse
  dependency from protocol types to the runtime.
- `control/tool_output.rs` uses harness LLM types. Shared DTOs must move below
  both consumers instead of making the control crate depend on the harness.
- `harness/acp.rs` uses the concrete worker `SandboxLeaseService`; worker code
  also uses harness execution types. Extract a narrow sandbox capability
  interface and keep its implementation in the worker.
- Resource keys import namespace helpers. Move both together for the first
  small, independent extraction.

A lexical scan also finds other cross-folder references, including tests and
cfg branches. Classify production, test-only and feature-specific edges during
each extraction; a text scan is not a resolved Rust dependency graph.

### Intended crate boundaries

Crate names below are proposed implementation targets. Split adapters further
only when their dependency isolation or measured build cost justifies it.

| Crate | Responsibility | Dependency restriction |
| --- | --- | --- |
| `talon-identifiers` | Resource keys, paths and namespace helpers | Small leaf; no runtime, storage or generated RPC dependency |
| `talon-protocol` | Internal generated messages, event types and their serde hooks | No control, gateway, harness or worker implementation dependency |
| `talon-contracts` | Storage/pubsub interfaces, shared execution DTOs and capability interfaces | Depends on identifiers/protocol, not concrete backends or services |
| `talon-storage-*` | SQLite/Postgres/RocksDB and cloud adapters | Depends on contracts; each backend owns only its required native/cloud dependencies |
| `talon-harness` | Execution, providers, tools and knowledge integration | Depends on contracts/protocol; no concrete worker dependency |
| `talon-gateway` | RPC/HTTP serving and handlers | Uses contracts and injected capabilities instead of worker controllers |
| `talon-worker` | Controllers, scheduling and runtime orchestration | Uses harness/contracts/protocol; generated clients come from protocol, not gateway implementation |
| `talon-cli` | CLI commands and lightweight manifest helpers | Avoids backend/service dependencies where commands do not need them |
| `talon-test-support` | Mocks and reusable test fixtures | Dev/test dependency; production crates do not import it |

The root `talon` crate initially remains a compatibility facade. Final binary
composition must use the actual subset of crates needed by each entry point:
keeping every binary dependent on a facade that imports everything would retain
much of today's rebuild fan-out. Audit the combined `talon-node` separately;
its dependency on both gateway and worker is intentional.

### Rollout as reviewable changes

1. **Record a warm baseline.** Keep CPU, memory, toolchain, flags and test
   coverage fixed. Compare an unchanged repeat with small CLI, worker, storage
   and protocol edits. Record build/test wall time separately, critical path,
   executed Rustc actions, cache hits/misses, affected targets, peak memory and
   binary/archive sizes. Consider trusted-main remote cache writes in a separate
   change, retaining read-only PR access. Do not mix that experiment with crate
   extraction. Likewise, evaluate RocksDB's current `NUM_JOBS=4` separately.
2. **Prove the mechanism with identifiers.** Move `control/keys.rs` and
   `control/ns.rs` into `talon-identifiers`. Preserve old module paths through
   re-exports. Update Cargo workspace/path dependencies, Bazel dependencies,
   crate-universe metadata and dependency-sync checks together. Demonstrate that
   a worker-only edit leaves this leaf's compilation cached before expanding
   the refactor. This small cut proves the tooling; it is not expected to remove
   most of the 89-second Talon action by itself.
3. **Extract protocol and serialization together.** Move internal generation
   and generated module ownership to `talon-protocol`. Move pure enum/capability
   serde hooks below the runtime and update their generated paths. Audit type-
   owned trait and inherent impls; keep them with their owning types or use
   explicit extension traits where appropriate. Preserve
   `talon::gateway::rpc::*` and control event imports through temporary
   re-exports. Keep `proto/` as the source of truth and avoid changing the public
   SDK client's packaging or making it depend on an unpublished internal crate.
4. **Extract contracts and invert service edges.** Move `KeyValueStore`,
   `MessagePublisher`, resource key/list types and the shared execution/capability
   DTOs to their appropriate leaf/contract crates. Separate factory/wiring code
   from interfaces. Replace the harness-to-worker sandbox dependency with an
   injected interface. Move test-only edges to test support. The exit condition
   is an acyclic graph with no leaf depending on the root facade.
5. **Extract adapters, then service crates one at a time.** Start with a backend
   whose inputs and tests are self-contained, then harness, gateway and worker.
   Give every crate a narrow explicit dependency list; do not copy
   `_RUNTIME_DEPS` into each library. Keep AWS/RocksDB/profile features optional
   and preserve the production transition in `tools/bazel/production.bzl`.
   Choose the next extraction from the measured remaining critical path.
6. **Narrow binary and test dependencies.** Move CLI code and reusable fixtures,
   replace production dependencies on the broad facade, and keep binary names
   and release labels stable. Give tests only the binaries/data they actually
   exercise where possible. Retain genuine integration dependencies and the
   full monorepo build/test on main.

Every extraction is independently revertible. Keep compatibility re-exports
until consumers migrate, and remove them only in a separate deliberate cleanup.
Do not do a simultaneous rewrite of runtime behavior, schemas or SDK APIs.

### Validation and acceptance gates

- Run the required pre-push checks and the full Linux BuildBuddy suite, including
  Docker, E2E, browser, durability and release/archive verification. Preserve
  native macOS release verification and publishing on GitHub.
- Check Cargo workspace/all-target builds and the root package's production
  `aws,rocksdb` feature combination as each manifest is migrated. Keep Cargo and
  Bazel dependency/feature definitions consistent; do not enable all profiling
  features merely as a substitute for the production feature check.
- Add focused serialization/wire compatibility coverage when generation or
  serde ownership moves. Preserve YAML/JSON defaults and enum spellings, existing
  manifest round trips, generated SDK checks and binary/archive contracts.
- Verify unrelated crate actions remain cached after a source-only component
  edit. A private storage edit must not require recompiling unrelated CLI/harness
  crates just because they share a BUILD file or a dependency umbrella.
- Repeat the same representative edits against the baseline, collecting at
  least three warm samples per retained comparison and reporting medians. Keep
  cold-cache results separate. Investigate memory, release size and runtime
  regressions before proceeding to the next extraction.
- Remember that BUILD, Cargo and module/lockfile changes conservatively select
  the full CI universe. An extraction PR's full run is not evidence that steady-
  state affected selection failed; evaluate source-only edits after the new
  boundaries exist. Shared protocol changes may legitimately affect everything.

The first implementation milestone is the identifiers extraction plus its cache
evidence. The next architectural milestone is protocol/serde ownership with no
runtime back-edge. Broader service splitting proceeds only after those gates.
