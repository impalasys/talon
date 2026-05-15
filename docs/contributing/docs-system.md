---
title: Docs System
sidebar_position: 1
---

Talon’s docs are canonical in the monorepo.

## Source of truth

- Hand-written docs live under `talon/docs`
- Draft or wiki-style notes live under `talon/docs/wiki`
- Generated reference is emitted into `talon/docs/reference/generated`

## Build model

- Astro builds both the landing page and the published `/docs` routes
- the canonical docs source still lives in `talon/docs`
- a Docusaurus scaffold still lives in `talon/docs-site`, but production publishing currently reads the markdown source directly from Astro

## Generated reference

Generated pages come from:

- `proto/gateway.proto`
- `proto/config.proto`
- `proto/manifests.proto`

Review generated diffs in PRs rather than treating them as opaque build output.

## Editing workflow

1. Edit or add markdown under `talon/docs`
2. If needed, update source proto definitions
3. Regenerate the reference pages via `pnpm --filter @impalasys/talon-docs generate:reference`
4. Build the site locally and verify `/docs`
