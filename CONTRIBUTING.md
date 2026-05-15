# Contributing

## Workflow

- Open pull requests against `main`.
- Keep changes scoped to Talon runtime, UI, tests, or supporting docs.
- Do not add private infrastructure, secrets, or deployment configuration.
- By contributing, you agree to the Talon Contributor License Agreement in
  `CLA.md`. The CLA workflow will ask you to sign it once per identity.

## Validation

Before opening a pull request, run:

```bash
cargo metadata --locked
cargo build --locked --bins
cargo test --locked
```

If your change touches the UI, also run:

```bash
cd ui
pnpm install --frozen-lockfile
pnpm build
```

## License headers

Rust source files in this repository carry the following header:

```rust
// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only
```

To apply headers across the tracked Rust files:

```bash
./scripts/apply_rust_license_headers.sh
```

To check that headers are present:

```bash
./scripts/check_rust_license_headers.sh
```

The scripts use `google/addlicense` pinned at `v1.2.0`.

## Scope notes

- `site/` is maintained privately and is not part of this repository.
- Production publishing is handled outside this repository.
