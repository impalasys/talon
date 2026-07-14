---
title: Docs System
sidebar:
  order: 1
---

Talon’s docs are canonical in the monorepo.

## Source of truth

- Hand-written docs live under `talon/docs`
- Draft or wiki-style notes live under `talon/docs/99-drafts`
- Generated reference is emitted into `talon/docs/05-reference/generated`

## Repository model

- The Markdown in `talon/docs` is the canonical documentation source in this repository.
- `talon/docs-site` currently exists to hold docs tooling, including the reference-generation script.
- Generated pages are checked into the repo so contract changes are reviewable in pull requests.

## Generated reference

Generated pages come from:

- `proto/talon/v1/*.proto`
- `proto/config.proto`
- `proto/resources/*.proto`
- `proto/data/*.proto`

Review generated diffs in PRs rather than treating them as opaque build output.

## Editing workflow

1. Edit or add markdown under `talon/docs`
2. If needed, update source proto definitions
3. Regenerate the reference pages via `pnpm --filter @impalasys/talon-docs generate:reference`
4. Review links, commands, and terminology against the actual repository layout
5. Include generated diffs in the same PR when proto changes affect the reference pages

## What to avoid

- Do not point readers at private directories or unpublished build pipelines that are not present in this repository.
- Do not hand-edit files under `docs/05-reference/generated/` unless you are also updating the generator.
