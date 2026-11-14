# Building with Bazel

Talon is a standalone Bazel workspace. Run commands from this repository's root.
Install Bazelisk and a native C/C++ toolchain. Bazel pins and downloads Rust,
Node, Python, Go, Java, protobuf, and their package dependencies. Docker is
required for container builds and the Rust library/infrastructure tests.

On Ubuntu, install `pkg-config libssl-dev clang libclang-dev cmake`. On macOS,
install the Xcode command line tools. RocksDB requires libclang; configure
`LIBCLANG_PATH` through `build --action_env=LIBCLANG_PATH=/path/to/lib` in
`.bazelrc.local` if it is installed outside standard locations.

## Build and test

```bash
bazel build //:binaries
bazel run //:talon-cli -- --help
bazel test //...
bazel build --config=aws --config=rocksdb //:binaries
bazel build --config=cpu-profile //:binaries
bazel build --config=heap-profile //:binaries
```

`bazel test //...` builds the declared targets and runs unit tests, Rust
integration tests and doctests, formatting checks, SDK generation freshness,
package checks, and UI coverage. Keep Docker running for the Rust library tests.
The production build and E2E binary targets enable AWS and RocksDB automatically;
ordinary development binaries keep Cargo's default features.
RocksDB's native C++ build uses four workers; other Cargo build scripts retain
the Rust rules' default concurrency.

Infrastructure, browser, and stress tests are explicit targets because they
need Docker, browser installations, external services, or substantially more time:

```bash
bazel test //tests:e2e_tests
bazel test //tests:stress
bazel run //ui:playwright -- install --with-deps chromium
bazel test //ui:e2e --test_env=CI=true
bazel run //packages/talon-chat:playwright -- install --with-deps chromium
bazel test //packages/talon-chat:screenshots
```

Playwright uses the pinned Bazel package; browser downloads live in Playwright's
normal machine cache. CI installs browsers before running those tests. Browser
and Python E2E launch the Bazel-built production binaries without invoking a
second build system. Tests write screenshots and reports into their
`bazel-testlogs/.../test.outputs/` directories. Tests that launch local servers
run locally and are marked exclusive where they share ports.

## Build coverage

| Area | Bazel targets |
| --- | --- |
| Runtime | `//:binaries`, root library/binary tests, Rust integration tests and doctests |
| Optional runtime features | `aws`, `rocksdb`, `cpu-profile`, `heap-profile` configurations |
| Rust SDKs, generator, examples, hook installer | `//sdk/rust/...`, `//sdk/examples/rust/...`, `//tools/install-hooks/...` |
| Go SDKs and example | `//sdk/go/...`, `//sdk/examples/go/...` |
| Java SDKs and example | `//sdk/java/...`, `//sdk/examples/java/...` |
| Python SDKs and example | `//sdk/python/...`, `//sdk/examples/python/...` |
| JS SDKs and example | `//sdk/js/...`, `//sdk/examples/js:build` |
| Talon chat | `//packages/talon-chat:build`, `unit_tests`, `package_test`, `storybook`, `screenshots` |
| Sightline | `//ui:dist`, `//ui:typecheck`, `//ui:unit_tests`, `//ui:e2e` |
| Python integration and stress suites | `//tests:e2e_tests`, `//tests:stress` |
| SDK generation and consistency | `//sdk:generated`, `//sdk:generated_test`, `//scripts/sdk/...` |
| Documentation | `//docs-site:dist`, `//docs-site:check`, `//docs-site:reference`, `//docs-site:reference_test` |
| Benchmarks | `//dockerfiles:benchmark_image`, `//bench:benchmark` |
| Release and Docker packaging | `//:release-dist`, `//dockerfiles:*_image` |
| Language distributions | JS/chat `:archive`, Python SDK `:dist`, Java SDK `:talon_client` / `:talon_server` |

The CI workflows use Bazel for compilation, tests, and distribution packaging.
Registry upload clients remain responsible for publishing: npm, Cargo
(`publish --no-verify`, after Bazel tests), PyPI's publishing action, and the Java
Bazel publication targets. Deployment tools consume prebuilt static output.

Python SDK `:dist` targets use the pinned upstream Hatchling backend through
`rules_python`, using each package's `pyproject.toml` for wheels and source
distributions. A small input-materialization shim remains: Hatchling otherwise
archives the Linux sandbox's absolute symlinks into sdists. Separate copying
actions do not fix this, because Bazel presents their outputs as symlinks to the
next action too. The shim copies and builds in one action; it does not implement
package formats or metadata. The native `rules_python` `py_wheel` rule covers
wheels; replacing this full backend would also require sdist support and a
solution for sharing `pyproject.toml` metadata.
`bazel test //sdk/python:packages_test` verifies the actual artifacts and checks
that each source distribution rebuilds a byte-identical wheel. Explicit Hatch
file selection keeps checkout/build metadata out of published source packages.
Python test targets also invoke upstream pytest directly with `rules_python`
`main_module = "pytest"`. Pytest expands the cache directory's environment
variable at test runtime, keeping its cache in Bazel test outputs without a
custom launcher.

Cargo manifests and package-manager scripts remain useful for IDEs and downstream
consumers; CI no longer compiles the same code through Cargo, Gradle, and pnpm.
Bazel runs the existing Rust `build.rs`, sharing protobuf types and serde behavior
with Cargo. No paths or compatibility aliases from the former monorepo are used.

### Remaining Bazel support code

Most of `tools/bazel` is generated dependency metadata and tests. Its small
adapters compose upstream rules rather than implementing compilers or packagers:

| Files | Purpose and remaining upstream opportunities |
| --- | --- |
| Root `BUILD.bazel`, `checksums.py` | Release layout is declared directly with upstream `rules_pkg` and `select_file` rules. Talon defines archive roots, platform spellings, modes, runtime data and checksum manifests; there is no release wrapper macro. The checksum helper retains the per-member and node-archive manifest policy. |
| `go_tools.bzl`, `protoc.bzl` | Expose executables from selected upstream toolchains. Keep toolchain resolution, including prebuilt protoc; hardcoded SDK paths or the source-built `@protobuf//:protoc` are not equivalent. |
| `production.bzl` | [with_cfg.bzl](https://github.com/fmeum/with_cfg.bzl) applies AWS/RocksDB and optimized compilation to production artifacts. Talon only defines the feature policy. |
| `sdk/proto_rules.bzl`, `sdk/normalize_generated.py` | A thin binding to `rules_proto_grpc` compiles Go/Java/JS with pinned plugins; direct-source mode preserves published comments. Python invokes the pinned `grpc_tools.protoc` module for protobuf/runtime compatibility. Talon retains whitespace/import/package policy and custom clientset facades. |
| `sdk/source_updates.bzl` | Declares destination ownership for upstream `copy_to_directory`, `directory_path`, `write_source_files` and `diff_test`. Complete update inputs include BUILD files and other metadata. No custom copy/delete/check implementation remains. |
| `python_packager.py` | Materializes sandbox inputs before invoking Hatchling's standard wheel/sdist backend functions. It contains no archive writer or metadata implementation; native `py_wheel` is not a drop-in replacement for both artifacts. |

## Docker and releases

```bash
# Build and load runtime + UI images using Docker on macOS or Linux.
scripts/build-images.sh

# Run on Linux so native binaries match the container OS.
bazel run //dockerfiles:runtime_image
bazel run //dockerfiles:benchmark_image
# Static UI image assembly also works on macOS.
bazel run //dockerfiles:ui_image
# Build/load the ACP developer sandbox through the Linux helper.
scripts/build-images.sh acp

bazel build //:release-dist
```

`rules_oci` assembles runtime, benchmark, UI, and ACP images from Bazel outputs.
Base images are pinned by digest. Debian packages come from a fixed snapshot;
Python/Monty wheels and the Docker client are checksum-pinned. Stable dependency,
configuration, and application layers are separate so code changes reuse the
other layers. Image assembly needs no Docker daemon or Docker Buildx.
`oci_load` loads an image locally; `oci_push` publishes directly to the registry.

The runtime retains the Python E2E tools, Monty, Docker client, curl, and the
model catalog. The ACP developer-tool sandbox packages pinned native coding
agents and the locked Claude ACP JavaScript adapter using maintained rules. It also includes the
Debian build toolchain, a generated CA bundle, and an unprivileged `codex` user
with writable home/config/workspace directories. No application image uses a
Dockerfile or Docker Buildx. `//dockerfiles:acp_push` publishes the ACP image;
CI retains all four existing GHCR repository names through its `--repository` flag.
Run `bazel test //dockerfiles/acp:smoke_test` on Linux with Docker to check all
three ACP initialization paths, native CLIs, compilers, certificates, and the
unprivileged filesystem layout. This explicit test is excluded from `//...`
because it loads and runs a Docker image.

Runtime images require a Linux builder matching the desired CPU architecture;
CI builds Linux x86-64. macOS supports native binary releases and UI image
assembly. ACP layers can also be cross-assembled on Linux using
`--platforms=//dockerfiles/acp:linux_x86_64` or
`--platforms=//dockerfiles/acp:linux_arm64`. Run the smoke test with a matching
Docker platform. For Compose, load the runtime and UI images, then run `docker compose up`.
Compose uses local `:dev` tags; `TALON_RUNTIME_IMAGE` and `TALON_UI_IMAGE` can
select other tags. `scripts/build-images.sh` provides a native Linux builder
inside Docker for macOS and Linux users. It builds the same OCI targets without
Dockerfiles or a nested Docker daemon, exports their archives, and loads them
into the host Docker daemon. Its `talon-bazel-linux-<arch>` volume preserves the
Bazel caches between runs. Pass `runtime`, `ui`, or `benchmark` to select images.

`bazel-bin/release-dist/` contains deterministic runtime and `talon-node` SDK
archives, `SHA256SUMS` for the runtime contents, and `node.sha256` for the SDK
archive. The runtime archive contains all four optimized binaries, configuration
files, and the full model catalog. `rules_pkg` builds both archives; the small
checksum tool only generates their manifests. Run
`bazel test //tools/bazel:release_archives_test` to verify the actual archive
layout, checksums, modes and packaged CLI. CI builds Linux x86-64
and macOS ARM64 bundles. Platform npm archives require the corresponding
Bazel-built release executable to be staged before packaging; missing binaries
fail the archive action.

Gateway configuration is a tracked build input:

```bash
bazel build --define=GATEWAY_URL=https://gateway.example.com //ui:dist
```

For a CLI release with embedded Google OAuth defaults, pass
`--action_env=TALON_GOOGLE_CLIENT_ID --action_env=TALON_GOOGLE_CLIENT_SECRET`.
The release workflow supplies these variables from its existing environment
secrets. Runtime OAuth environment configuration remains available.

## Generated sources and dependencies

```bash
# Generate with pinned tools and update the owned SDK destinations.
scripts/sdk/generate.sh
bazel test //sdk:generated_test

# Regenerate documentation reference pages.
bazel build //docs-site:reference
cp -R bazel-bin/docs-site/generated-reference/. docs/05-reference/generated/

# After updating Cargo.toml and Cargo.lock:
python3 scripts/sync_bazel_dependencies.py
bazel build //:binaries
python3 scripts/sync_bazel_dependencies.py --check
```

SDK updates replace only the five generated directories declared in
`sdk/source_updates.bzl` and the individual clientset/facade files. Handwritten
SDK helpers outside those directories are preserved. Complete directory inputs
combine generated files with checked-in non-generated metadata, including Go
`BUILD.bazel` files and Rust's `.gitkeep`. Add new handwritten helpers outside
owned generated directories; maintain metadata through each SDK package's
`generated_metadata` filegroup. Freshness uses upstream `diff_test` against a
tree assembled from declared checked-in files, so stale file additions/removals
invalidate the check. It never relies on a source-directory label as an input.
`//sdk:update_generated_test` runs the real upstream updater twice against a
temporary checkout, checking content, stale removal and helper/metadata preservation.

Commit the generated metadata manifest, root `Cargo.lock`, `MODULE.bazel.lock`,
and relevant language locks with dependency changes. Cargo and Bazel share the
root lock; there is no second Rust dependency lock to synchronize. The root
Cargo workspace includes the metadata-only feature package and hook installer,
with `default-members = ["."]` keeping ordinary Cargo builds scoped to Talon.
The metadata package resolves optional dependencies for Bazel without enabling
those features in ordinary builds. Maven dependencies are pinned in
`tools/bazel/maven_install.json`; repin with `REPIN=1 bazel run @maven//:pin`.
Python dependencies use `tools/bazel/requirements.lock`, generated with
`uv pip compile --universal --python-version 3.13 --generate-hashes tools/bazel/requirements.in -o tools/bazel/requirements.lock`.

`tools/bazel` contains dependency pins and the small integration points that need
Talon-specific behavior: production feature selection, release layout/checksums,
SDK generation/synchronization, and test entry points. Archive construction,
package assembly, and generic build actions come from `rules_pkg`, `rules_js`,
`rules_distroless`, and `bazel_lib`. The Go gRPC generator is pinned directly in
`MODULE.bazel`; it does not need a synthetic Go module.

## BuildBuddy setup

Create a BuildBuddy organization and an API key with cache write access. Add the
key to this repository's Actions secrets as `BUILDBUDDY_API_KEY`. Every build job
uses the shared Bazel setup action, which enables remote caching and build-event
uploads for trusted pushes to `main`. PRs use the CI disk cache without receiving
the writer key. Builds work without the secret.

Repository writers can verify the secret before merging with the explicit
manual cache-probe job:

```bash
gh workflow run ci.yml --ref YOUR_TRUSTED_BRANCH
```

Manual dispatch runs only `BuildBuddy Cache Verification`, not the complete CI
matrix. This is the sole exception to main-only writer access. It uploads one
small action and builds it again with an independent output base and disk
caching disabled; the second log must report a remote cache hit. Each run uses
a unique probe input so it checks a new upload as well as a read.
Logs are attached to the run; the API key is never printed.

See [the CI execution assessment](./03-ci-execution.md) for the measured baseline,
Workflows tradeoffs, and the comparison needed before migrating CI.

For a developer machine, put authentication in the ignored `.bazelrc.local`:

```text
build:buildbuddy --remote_header=x-buildbuddy-api-key=YOUR_KEY
build:buildbuddy --bes_header=x-buildbuddy-api-key=YOUR_KEY
```

Then run `bazel test --config=buildbuddy //...`. Read-only keys can additionally
set `build:buildbuddy --remote_upload_local_results=false`. Do not commit keys.
BuildBuddy receives build events, action logs, and cached outputs when enabled.
See [BuildBuddy authentication](https://www.buildbuddy.io/docs/guide-auth/) and
[the quickstart](https://www.buildbuddy.io/docs/quickstart/).

Remote execution is a separate rollout requiring a matching Linux execution
image and native toolchains. This configuration enables remote caching. Measure
cold and warm CI times and action-cache hit rates before promising a speedup;
linking, Docker layers, and infrastructure tests still take time.
