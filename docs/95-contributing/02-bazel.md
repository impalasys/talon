# Building with Bazel

Talon has its own Bazel workspace. Run these commands from the repository root;
all labels refer to this repository.

## Prerequisites

Install Bazelisk, a native C/C++ toolchain, and Docker for the Rust library tests.
Bazelisk reads `.bazelversion`; Bazel downloads the Rust and Node toolchains and
a prebuilt protobuf compiler. Rust dependencies come from Cargo manifests and
JavaScript dependencies come from `pnpm-lock.yaml`.

On Ubuntu, install `pkg-config libssl-dev clang libclang-dev cmake`. On macOS,
install the Xcode command line tools. RocksDB additionally needs libclang; set
`LIBCLANG_PATH` through a local Bazel configuration if your installation cannot
be discovered automatically.

## Build and test

```bash
bazel build //:binaries
bazel run //:talon-cli -- --help
bazel test //...
bazel build --config=release //:binaries
bazel build --config=aws --config=rocksdb //:binaries
```

The four runtime binaries appear directly in `bazel-bin/`. Rust targets include
library tests, binary unit tests, the Rust client SDK tests, and the integration
tests in `tests/*.rs`. The library suite launches PostgreSQL containers and
runs locally, even when remote execution is configured. Docker must be running.
The Python E2E helpers also discover binaries in this repository's `bazel-bin/`.

Bazel runs the same `build.rs` as Cargo for protobuf generation. Generated Rust
types and serde behavior are shared between both build systems.

Frontend targets include:

```bash
bazel build //ui:dist
bazel test //ui:typecheck
bazel build //packages/talon-chat:build
bazel build //sdk/js/talon-client:build //sdk/js/talon-server:build
```

Coverage is incremental. `bazel test //...` runs the declared Bazel tests; the
table below lists the current build and test coverage.

| Area | Bazel coverage |
| --- | --- |
| Core Rust runtime and four production binaries | Build, library/binary unit tests, and root Rust integration tests |
| AWS and RocksDB runtime features | Build configurations |
| Rust client SDK | Build and unit tests |
| Sightline UI | Production build and TypeScript checking |
| Talon chat and JS client/server SDKs | Package builds |
| JavaScript unit tests and browser E2E | Existing pnpm/Playwright commands |
| Rust server SDK, Rust SDK codegen, examples, install-hooks tool | Existing Cargo commands |
| Go, Java, and Python SDKs and Python E2E | Existing language tooling |
| Rust doctests and optional CPU/heap profiling features | Existing Cargo commands |
| Documentation, benchmarks, container/release packaging, and publishing | Existing tooling and workflows |

The Cargo CI job remains alongside Bazel while build parity is established.

## Dependency updates

After updating Cargo dependencies and `Cargo.lock`, run:

```bash
python3 scripts/sync_bazel_dependencies.py
bazel build //:binaries
python3 scripts/sync_bazel_dependencies.py --check
```

Commit `tools/bazel/Cargo.toml`, `tools/bazel/Cargo.lock`, and `MODULE.bazel.lock`
with the source dependency changes. The metadata-only Cargo package enables
resolution of AWS and RocksDB dependencies without changing ordinary Cargo
builds. Its separate lockfile also includes SDK development dependencies used
by Bazel; CI verifies that external package versions match `Cargo.lock`.

## BuildBuddy cache

The `buildbuddy` configuration is opt-in. Put authentication in the ignored
`.bazelrc.local` file, using the API key from your BuildBuddy organization:

```text
build:buildbuddy --remote_header=x-buildbuddy-api-key=YOUR_KEY
build:buildbuddy --bes_header=x-buildbuddy-api-key=YOUR_KEY
```

Then run:

```bash
bazel build --config=buildbuddy //:binaries
bazel test --config=buildbuddy //...
```

For CI, create a BuildBuddy organization and an API key with cache write access,
then add it as the repository Actions secret `BUILDBUDDY_API_KEY`. The Bazel job
automatically enables BuildBuddy on pushes to `main`.
The repository settings page is:

<https://github.com/impalasys/talon/settings/secrets/actions>

Pull-request builds keep using the local CI cache; the BuildBuddy writer key is
only passed to trusted main-branch builds. With no secret, CI continues to work
using its existing disk cache. A BuildBuddy GitHub App installation is optional
for this GitHub Actions setup; BuildBuddy Workflows are a separate integration.

Use separate read-only keys for developers or other cache readers when
appropriate, and add `build:buildbuddy --remote_upload_local_results=false` to
their `.bazelrc.local` files. Do not commit keys. BuildBuddy receives build events,
action logs, and cached outputs when this config is used. See the
[authentication guide](https://www.buildbuddy.io/docs/guide-auth/) for API keys
and the [quickstart](https://www.buildbuddy.io/docs/quickstart/) for account setup.

Remote execution is a separate rollout: native compilation needs a Linux
execution image and matching C/C++ toolchains, and Docker-based tests need a
suitable execution environment. This initial configuration enables caching.
Configure native execution platforms before enabling remote execution.
