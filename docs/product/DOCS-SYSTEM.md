# Documentation System

## Purpose

This note defines how documentation should work in `specpunk`.

The goal is to keep one canonical source of truth in the repository while also supporting a clean public docs surface.

## Canonical source of truth

The canonical documentation for the project lives in the repository.

Primary source files:

- `README.md`
- `docs/product/REPO-STATUS.md`
- `docs/product/CURRENT-ROADMAP.md`
- `docs/product/ARCHITECTURE.md`
- `docs/product/CLI.md`
- `docs/product/VISION.md`

When product truth changes, update these canonical docs first.

## Public docs layer

The public docs site is a curated layer over the repository.

Current public goals:

- explain what SpecPunk is
- explain how to use the current CLI
- explain the current operator path
- publish the roadmap
- keep active v0 surface separate from inactive or planned-only surfaces

The public docs site should stay short, product-facing, and easy to scan.

## What is intentionally not public

These should remain repository-internal for now:

- `docs/research/*`
- detailed review memos
- internal execution plans and working notes that are not needed for public operator understanding

## Status vocabulary

Use the same vocabulary everywhere:

- **active v0 surface**
- **in-tree but inactive**
- **planned only**

Do not describe planned-only or inactive surfaces as part of today's default operator path.

## Documentation rules

1. Repo truth comes first.
2. Public docs may summarize canonical docs, but must not contradict them.
3. Internal research notes are not part of the first public docs pass.
4. Examples are deferred until the project has real examples worth publishing.
5. Roadmap belongs on the public site, but the canonical current roadmap still lives in `docs/product/CURRENT-ROADMAP.md`.
