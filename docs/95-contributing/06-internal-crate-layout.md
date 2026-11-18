---
title: Internal Rust crate layout
---

# Internal Rust crate layout

Internal packages use short component names. The repository provides their
namespace; all packages under `crates/` remain unpublished. The root `talon`
package, public SDK packages, executable names, images, and release artifacts
keep their existing names.

Tracing's implicit targets follow Rust crate names. Target-specific filters such
as `talon_worker=debug` become `worker=debug`; a broad `talon=info` filter no
longer selects every internal component. The default `RUST_LOG=info` continues
to cover all components. Explicit tracing targets and wire identifiers stay put.

## Migration stages

1. **Names:** remove `talon-` from internal package and directory names, and
   `talon_` from their Rust identifiers and Bazel library targets. For example,
   `talon-config` becomes `config`. Update workspace paths, feature forwarding,
   generated dependency metadata, imports, tests, and documentation together.
2. **Namespaces:** use `talon::config`, `talon::protocol`, and
   `talon::storage::postgres` for references between internal components.
   Each implementation crate and executable owns a private namespace module
   that re-exports only its direct dependencies. Nested modules import it with
   `use crate::talon;`. These modules add no dependency edges or compiled crates.
   Third-party imports and local `crate::`, `self::`, and `super::` paths remain
   ordinary Rust. Generated Rust keeps its generator's paths. The root `talon`
   facade exposes canonical paths and retains existing compatibility paths.
3. **Directories:** move `crates/storage-postgres` to `crates/storage/postgres`,
   and apply the same rule to every storage package. Cargo package names remain
   `storage-postgres`, Rust identifiers remain `storage_postgres`, and imports
   remain `talon::storage::postgres`. The storage directory is organizational;
   it is not an umbrella crate. Other component directories stay flat.

Keep each stage independently buildable and reviewable. Historical benchmark
records continue to describe their original revisions.

## Dependency boundaries

A namespace is a source-level view of an existing dependency graph, not a way
to import every component. Implementation crates must not depend on the root
`talon` facade: it depends on those implementations. Explicit per-consumer
re-exports preserve direct dependency boundaries and the crate split's cache
isolation. Optional and test-only dependencies retain their feature/test gates.

The root facade remains available for compatibility consumers. Production
executables continue depending directly on the components they need. Public SDK
packages remain independent of unpublished internal packages.

## Acceptance checks

- Audit the migration for unchanged business logic, external dependency versions,
  feature selection, protocol output, runtime identifiers, and release behavior.
  Review automatic logging targets when Rust crate identifiers change.
- Check Cargo workspace resolution and default, AWS, RocksDB, CPU-profiling,
  and heap-profiling feature configurations. Keep Bazel feature metadata in sync.
- Run full BuildBuddy Linux CI, including E2E and Docker tests, plus macOS release
  and publishing checks before landing each stage.
- Compare normalized dependency graphs; only names and physical paths may change.
- Compile canonical namespace paths and existing root-facade compatibility paths.
- After the final stage, repeat CLI and worker source-edit cache checks. Unrelated
  binaries must retain their hashes and avoid recompilation.

## Progress

- Pass 1: internal names shortened.
- Pass 2: canonical namespaces implemented.
- Pass 3: storage directories nested; package names and namespaces unchanged.
