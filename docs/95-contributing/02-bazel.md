# Building with Bazel

Talon is a standalone Bazel workspace. Run commands from this repository's root.
Install Bazelisk and a native C/C++ toolchain. Bazel pins and downloads Rust,
Node, Python, Go, Java, protobuf, and their package dependencies. Docker is
required for container builds and the Rust library/infrastructure tests.

On Ubuntu, install `pkg-config libssl-dev clang libclang-dev cmake`. On macOS,
install the Xcode command line tools. RocksDB requires libclang; configure
`LIBCLANG_PATH` in your environment if it is installed outside standard locations.

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

Cargo manifests and package-manager scripts remain useful for IDEs and downstream
consumers; CI no longer compiles the same code through Cargo, Gradle, and pnpm.
Bazel runs the existing Rust `build.rs`, sharing protobuf types and serde behavior
with Cargo. No paths or compatibility aliases from the former monorepo are used.

## Docker and releases

```bash
# Run on Linux so native binaries match the container OS.
bazel run //dockerfiles:runtime_image -- --tag talon-runtime:dev
bazel run //dockerfiles:benchmark_image -- --tag talon-bench:dev
# Static UI and ACP images can be packaged from either supported host OS.
bazel run //dockerfiles:ui_image -- --tag talon-ui:dev
bazel run //dockerfiles:acp_image -- --tag talon-acp:dev

bazel build //:release-dist
```

Docker packages declared, precompiled Bazel contexts. It does not compile Rust
or JS again in CI. Direct Docker builds and Compose use a Bazel builder stage
on Linux. The runtime image retains the Python E2E tools and Monty runtime.

`bazel-bin/release-dist/` contains the platform's four optimized runtime binaries,
configuration files, full model catalog, deterministic tar archives and SHA256
checksums. It also contains the `talon-node` SDK archive. CI builds Linux x86-64
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
# Generate with pinned tools and copy only generated SDK files into the checkout.
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

Commit the Bazel metadata manifest/lock, `MODULE.bazel.lock`, and relevant language
locks with dependency changes. The metadata-only Cargo package resolves optional
feature dependencies without enabling them in ordinary builds. The separate lock
also records SDK development dependencies; CI checks registry versions against
the root Cargo lock. Maven dependencies are pinned in
`tools/bazel/maven_install.json`; repin with `REPIN=1 bazel run @maven//:pin`.
Python dependencies use `tools/bazel/requirements.lock`, generated with
`uv pip compile --universal --python-version 3.13 --generate-hashes tools/bazel/requirements.in -o tools/bazel/requirements.lock`.

## BuildBuddy setup

Create a BuildBuddy organization and an API key with cache write access. Add the
key to this repository's Actions secrets as `BUILDBUDDY_API_KEY`. Every build job
uses the shared Bazel setup action, which enables remote caching and build-event
uploads for trusted pushes to `main`. PRs use the CI disk cache without receiving
the writer key. Builds work without the secret.

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
