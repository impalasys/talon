# Bazel-backed Rust in Zed (opt-in pilot)

This pilot gives Zed the Rust graph, feature cfgs, generated sources, proc-macro
libraries, rust-analyzer, macro server, and formatter from the repository's pinned
Bazel toolchains. Opening, editing, and saving Rust files does not invoke Cargo or
start a compiler build. Rust-analyzer still indexes and analyzes source code.

Compiler diagnostics, including complete borrow checking and Clippy, are explicit
Bazel commands during this pilot. Rust-analyzer's native diagnostics, completion,
navigation, and macro expansion stay enabled. This is a deliberate pilot tradeoff;
it does not provide the same automatic compiler diagnostics as `cargo check`.

## Setup and everyday use

From this worktree's root, with Python 3, Bazelisk, and Cargo installed:

```sh
python3 scripts/dev/rust_ide.py setup
```

Setup writes ignored project-local Zed settings and state under
`.rules_rust_analyzer/`; global Zed preferences are untouched. Zed normally
restarts the Rust language server after setup and every explicit refresh.
This is necessary because automatic graph reloading is disabled. If an old
server remains active, run Zed's **editor: restart language server** action.
The latest server log is `.rules_rust_analyzer/zed-rust-analyzer.log` (a
symlink into `.rules_rust_analyzer/logs/`, which retains previous server logs).

The active model is `.rules_rust_analyzer/rust-project.json`. Setup uses the
upstream `rules_rust` generator and postprocesses its root `rust-project.json`
before atomically publishing the active model. A running editor never consumes
an intermediate model without the standard-library graph.

No model refresh is needed for ordinary edits to existing Rust files. Refresh
explicitly after changing BUILD files, dependencies, features, generated-code
inputs, toolchain versions, or switching branches with different build graphs:

```sh
python3 scripts/dev/rust_ide.py refresh
bazel build //:binaries
bazel test //crates/fanout:unit_tests
```

Use your actual changed Bazel targets for targeted checks. Run the repository's
usual full test suite before pushing. Build, test, and model generation use the
normal Bazel output base, so they share proc-macro and generated-source outputs.

`--workspace /path/to/talon` configures a different checkout. Each checkout gets
its own absolute source paths and state. A copy of the helper is also installed
locally, so an active pilot can refresh before the helper's branch is merged:

```sh
python3 .rules_rust_analyzer/rust_ide.py refresh
```

The default model matches native, default-feature Bazel development builds. To
analyze an optional-feature configuration, pass one named Bazel configuration:

```sh
python3 scripts/dev/rust_ide.py refresh --config=aws
```

Refresh remembers that selection. Run `setup` without `--config` to return to the
default configuration. For combined features, define one combined named config
in `.bazelrc.local`. Changing configurations can legitimately compile additional
macros/build scripts. Production/E2E feature selections and cross-compilation
are not represented by the default native model.

The helper discovers Rust targets in Git-visible Bazel packages, including new
unignored BUILD files. It avoids scanning ignored nested editor worktrees.
Existing project settings must be plain JSON; unsupported JSONC fails before any
build/model generation and is left untouched. Unrelated JSON preferences are
preserved, and the original settings/model are backed up locally.

## Why the extra integration exists

Upstream `rules_rust` setup does not provide Zed settings directly. The pilot
translates the tool paths into Zed's `lsp.rust-analyzer` configuration and disables
Cargo autoreload, Cargo build scripts, and on-save compiler checks. It removes
the generated Flycheck runnable, whose separate `_rra` output base and JSON
compiler-diagnostics flags would otherwise create another compilation setup.
The upstream setup tool may initialize that output base; the pilot does not use
it for compilation.

The Bazel aspect materializes proc-macro dylibs and build-script outputs,
including protobuf `OUT_DIR` sources. It does not compile every application
binary. The helper also overlays Bazel's `rustc_env_files`, which the upstream
model omits, so version macros match real builds. Keep the repository's existing
macOS `-Cstrip=none` exec-Rust flag: stripped proc-macro dylibs fail to load.

Even an explicit JSON project normally makes rust-analyzer invoke Cargo metadata
for the standard library. To prevent this automatic call, manual setup reads
**only the pinned standard library's metadata**, once per sysroot, without running
Cargo build/check or build scripts. It caches a `sysroot_project` source graph
and includes it in the IDE model. A toolchain change can require another explicit
metadata query; Cargo may download missing standard-library dependency sources.
No Cargo compilation cache is created for Talon.

## Validation and measurements

Measured on the local Apple Silicon development machine on 2026-11-18, with
existing Bazel dependency outputs. These are warm-cache results, not a cold-build
speedup claim.

| Operation | Wall time | Executed Rust compiler actions |
| --- | --- | --- |
| Warm `bazel build //:binaries`, three runs | 0.47 s median | 0 each |
| Explicit IDE model refresh, three runs | 2.45 s median | 0 each |
| Build `//crates/fanout:fanout` after a temporary source edit | 0.51 s | 1 |
| IDE model refresh after that build | 2.41 s | 0 |

Bazel execution logs counted actions, rather than inferring cache reuse from
elapsed time. The edit was restored after the probe. No normal Rust dependency
was compiled again for IDE model generation. Initial setup can still compile
missing proc-macro tools and generators; source indexing is additional IDE work.

A standalone LSP probe using the installed Zed initialization settings verified
Tokio test and Serde derive expansion, `tonic::include_proto!`, navigation into
Bazel-generated protobuf code, and the correct `CARGO_PKG_VERSION` expansion.
The opened source files reported no diagnostics. Opening, changing, and saving
files, including a live model refresh, caused zero guarded Cargo/Bazel/compiler
command attempts. The refresh itself was run explicitly outside the LSP guard. The same probe
passed against the actual Zed checkout, where Zed's running server and macro
server paths were confirmed to come from Bazel. Editor keystrokes could not be
automated because the computer-use service could not access Zed's window.

## Rollback

```sh
python3 scripts/dev/rust_ide.py restore
# Before this branch is merged, the locally installed helper also works:
python3 .rules_rust_analyzer/rust_ide.py restore
```

Run one command. Restore reinstates the original project settings and original
root model (or removes the generated model if none existed), allowing normal
Cargo discovery again. It refuses to overwrite project settings edited since
the last setup/refresh; in that case merge the local backup manually. Bazel's
cache and the machine-local state directory remain available for a later pilot.
