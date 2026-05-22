# Contributing

## Workflow

- Open pull requests against `main`.
- Keep changes scoped to Talon runtime, UI, tests, or supporting docs.
- Do not add private infrastructure, secrets, or deployment configuration.

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

If your change touches generated reference docs or their proto sources, also run:

```bash
pnpm --filter @impalasys/talon-docs generate:reference
```

## Docs workflow

- Canonical docs live under `docs/`.
- Generated reference lives under `docs/reference/generated/`.
- Reference pages are derived from `proto/gateway.proto`, `proto/config.proto`, and `proto/manifests.proto`.
- Draft notes can live under `docs/wiki/`, but published-facing documentation should be kept in the main docs tree.

## Scope notes

- This repository does not include Impala's private marketing site or internal deployment plumbing.
- Public-facing documentation source lives directly in this repository under `docs/`.
