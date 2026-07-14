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

## Scope notes

- `site/` is maintained privately and is not part of this repository.
- Production publishing is handled outside this repository.
