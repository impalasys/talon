# Contributing

Thanks for helping improve Talon. This guide covers the expected local workflow,
repository layout, validation steps, and pull request checklist.

## Getting started

1. Fork or clone the repository.
2. Install the prerequisites listed in [`README.md`](./README.md#prerequisites).
3. Create a branch from `main`.
4. Keep changes scoped to Talon runtime, UI, tests, docs, or supporting tooling.

Do not add private infrastructure, secrets, or deployment configuration.

## Repository layout

```text
src/                  Rust runtime, gateway, worker, control plane, CLI
src/bin/              talon-server, talon-worker, talon-cli entrypoints
ui/                   Next.js operator UI
packages/talon-chat/ React client package for Talon-backed chat surfaces
sdk/                  Client packages and examples
proto/                Protobuf service and schema definitions
manifests/            Default and example namespace/agent resources
dockerfiles/          Runtime and UI container builds
docs/                 Product and operator documentation
tests/                Python end-to-end tests
```

## Development workflow

- Open pull requests against `main`.
- Prefer small, reviewable changes with focused commits.
- Include tests or docs updates when behavior, APIs, manifests, or user-facing workflows change.
- Keep generated files in the same pull request as the source changes that produced them.

## Validation

To enable the repository pre-push hook in a worktree, run:

```bash
cargo install-hooks
```

This sets `core.hooksPath` to `.githooks`. After that, `git push` runs the Rust
validation checks below before sending commits to the remote.

Before opening a pull request, run:

```bash
cargo metadata --locked
cargo fmt --all --check
cargo build --locked --bins
cargo test --locked
```

If your change touches the UI, also run:

```bash
cd ui
pnpm install --frozen-lockfile
pnpm build
```

If your change touches container builds, also run:

```bash
docker build -f dockerfiles/oss-runtime.Dockerfile .
docker build -f dockerfiles/oss-ui.Dockerfile .
```

## Documentation

Docs are organized numerically so GitHub renders them in reading order. Start
with [`docs/00-introduction.md`](./docs/00-introduction.md), and see
[`docs/95-contributing/01-docs-system.md`](./docs/95-contributing/01-docs-system.md)
for the docs workflow.

If proto changes affect generated reference pages, regenerate them:

```bash
pnpm --filter @impalasys/talon-docs generate:reference
```

Review generated diffs rather than treating them as opaque build output.

## Pull request checklist

- The change is scoped and described clearly.
- Local validation passed, or skipped checks are called out with a reason.
- Docs and examples are updated when user-facing behavior changes.
- Generated reference files are included when proto contracts change.
- No secrets, private infrastructure, or unrelated refactors are included.

## Security

Do not report security vulnerabilities in public issues. See
[`SECURITY.md`](./SECURITY.md) for the project security policy.

## Scope notes

- `site/` is maintained privately and is not part of this repository.
- Production publishing is handled outside this repository.
