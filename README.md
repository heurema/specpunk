<p align="center">
  <img src="site/assets/punk-mascot.svg" alt="SpecPunk mascot" width="240" />
</p>

# SpecPunk

Local-first, stewarded multi-agent engineering runtime.

`punk` is the public surface of this repo: one CLI, one vocabulary, one runtime.

## What SpecPunk is

SpecPunk is a repository-first runtime for AI-driven engineering work across codebases.

It is built around a small set of product laws:

- **one CLI**: `punk`
- **one vocabulary**: `plot / cut / gate`
- **contract first**
- **`gate` writes the final decision**
- **proof before acceptance**
- **local-first, VCS-aware operation**

The goal is not to be a generic provider-zoo shell or just another chat wrapper around coding models.
The goal is to make bounded engineering work more reliable, inspectable, and proof-bearing.

## Current status

This repo is in an active rebuild phase.

Use this vocabulary everywhere:

- **active v0 surface**
- **in-tree but inactive**
- **planned only**

Current reality:

- current operator path: `init -> start/go -> plot -> cut -> gate -> proof`
- `punk-council` is **in-tree but inactive**
- `punk-shell`, `punk-skills`, `punk-eval`, and `punk-research` are **planned only**
- legacy code under `punk/` is source material, not the public operator surface

The exact repo truth lives in [`docs/product/REPO-STATUS.md`](docs/product/REPO-STATUS.md).

## Quickstart

Run the CLI from the source tree:

```bash
cargo run -p punk-cli -- --help
```

Initialize a repository:

```bash
cargo run -p punk-cli -- init --enable-jj --verify
```

If `punk` is already on your `PATH`, you can use:

```bash
punk init --enable-jj --verify
```

Default happy path:

```bash
punk go --fallback-staged "<goal>"
```

Staged path:

```bash
punk start "<goal>"
punk plot approve <contract-id>
punk cut run <contract-id>
punk gate run <run-id>
punk gate proof <run-id|decision-id>
```

`jj` is preferred when available. `git` is the fallback.

## Read this first

If you are orienting in the repo, read in this order:

1. [`docs/product/REPO-STATUS.md`](docs/product/REPO-STATUS.md)
2. [`docs/product/CURRENT-ROADMAP.md`](docs/product/CURRENT-ROADMAP.md)
3. [`docs/product/CLI.md`](docs/product/CLI.md)
4. [`docs/product/ARCHITECTURE.md`](docs/product/ARCHITECTURE.md)
5. [`docs/product/ADR-provider-alignment.md`](docs/product/ADR-provider-alignment.md)
6. [`docs/product/VISION.md`](docs/product/VISION.md)
7. [`docs/product/ACTION-PLAN.md`](docs/product/ACTION-PLAN.md)
8. [`docs/product/NORTH-ROADMAP.md`](docs/product/NORTH-ROADMAP.md)

## Canonical docs map

Core product docs:

- [Repo status](docs/product/REPO-STATUS.md)
- [Current roadmap](docs/product/CURRENT-ROADMAP.md)
- [CLI](docs/product/CLI.md)
- [Architecture](docs/product/ARCHITECTURE.md)
- [Vision](docs/product/VISION.md)
- [Action plan](docs/product/ACTION-PLAN.md)
- [North roadmap](docs/product/NORTH-ROADMAP.md)
- [Documentation system](docs/product/DOCS-SYSTEM.md)

Public docs layer:

- Mintlify config: [`docs.json`](docs.json)
- Public overview source: [`index.mdx`](index.mdx)
- Public quickstart source: [`quickstart.mdx`](quickstart.mdx)
- Public roadmap source: [`roadmap.mdx`](roadmap.mdx)

The public docs site is a curated layer over the repo.
Canonical product truth still lives in the repository.

## What is in scope today

The current public slice is intentionally narrow:

- repository bootstrap
- goal-first intake through `punk go` and `punk start`
- contract drafting and approval
- bounded `cut` execution
- `gate` decision and proof artifacts
- repo-local artifacts and inspectable status surfaces

Not current-forward:

- interactive shell as the default surface
- always-on council
- broad internal memory platform work
- provider-zoo UX
- public research notes
- examples gallery before real examples exist

## License

MIT
