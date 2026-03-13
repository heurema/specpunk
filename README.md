# Specpunk

Specpunk is a repo-native review boundary for AI-assisted brownfield change.

Current thesis:

- AI can generate code faster than teams can safely review it.
- The bottleneck is no longer generation, but review clarity and trust.
- Specpunk turns a noisy change into a bounded review object.

Public surface:

- `https://specpunk.com`

## Status

Experimental.

What already exists in this repo:

- a Go CLI wedge
- repo-local task scaffolding
- scope checking against manifest, diff, stdin, or git range
- stored proof artifacts on internal and external repos
- product operating docs under `docs/product/`

## Current Wedge

The first wedge is:

`scope enforcement + minimal review artifact`

That means:

- declare what files a task should touch
- compare the declared boundary with the actual change
- emit a compact review artifact that says whether the change stayed bounded

## Quick Start

Run tests:

```bash
go test ./...
```

Create a minimal task directory:

```bash
go run ./cmd/specpunk task init \
  --task-dir tmp/demo-task \
  --task "Review a bounded site change." \
  --allow "site/index.html" \
  --allow "site/style.css" \
  --block "docs/research/**" \
  --evidence "manual browser check"
```

Important:

- quote glob patterns like `"docs/research/**"` so the shell does not expand them before Specpunk sees them

Add a changed-file manifest:

```bash
printf 'site/index.html\nsite/style.css\n' > tmp/demo-task/changed.txt
```

Generate a review artifact:

```bash
go run ./cmd/specpunk check \
  --task-dir tmp/demo-task \
  --changed-manifest tmp/demo-task/changed.txt
```

That writes:

- `tmp/demo-task/generated-review.md`

## Repo Map

- `cmd/specpunk/`
  CLI entrypoint
- `internal/check/`
  scope classification and review artifact rendering
- `internal/taskdir/`
  task-directory scaffold workflow
- `site/`
  current public surface deployed on `specpunk.com`
- `docs/product/`
  product source of truth, roadmap, cycle, reviews, and dogfood packets
- `docs/research/`
  supporting research and synthesis docs
- `tools/specpunk_review.py`
  earlier spike, kept for reference, not the current product path

## Product Docs

Start here:

- [Product Brief](docs/product/brief.md)
- [Roadmap](docs/product/roadmap.md)
- [Current Cycle](docs/product/current-cycle.md)

## Non-Goals

Specpunk is not:

- a new IDE
- a replacement for Claude Code, Codex, or Cursor
- a natural-language compiler
- a full spec-as-source system
