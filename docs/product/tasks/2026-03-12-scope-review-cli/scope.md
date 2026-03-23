# Scope

Last updated: 2026-03-12
Task status: completed

## Declared Runtime Scope

Allowed:

- `go.mod`
- `cmd/specpunk/main.go`
- `cmd/specpunk/main_test.go`
- `internal/check/check.go`
- `internal/check/check_test.go`
- `sandbox/git-proof-repo/README.md`
- `sandbox/git-proof-repo/site/index.html`
- `sandbox/git-proof-repo/site/style.css`
- `sandbox/git-proof-repo/docs/research/notes.md`
- `sandbox/git-proof-repo/wrangler.toml`
- `docs/product/tasks/2026-03-12-scope-review-cli/input.json`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/input-drift.json`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-drift.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/input-git.json`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-git.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/input-git-drift.json`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-git-drift.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/changed-manifest.txt`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-manifest.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/changed-manifest-drift.json`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-manifest-drift.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/changed-diff.patch`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/changed-diff-drift.patch`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff-drift.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff-stdin.md`

Blocked:

- `site/**`
- `docs/research/**`
- `wrangler.toml`
- `site/_headers`

## Actual Runtime Scope

Touched:

- `go.mod`
- `cmd/specpunk/main.go`
- `cmd/specpunk/main_test.go`
- `internal/check/check.go`
- `internal/check/check_test.go`
- `sandbox/git-proof-repo/README.md`
- `sandbox/git-proof-repo/site/index.html`
- `sandbox/git-proof-repo/site/style.css`
- `sandbox/git-proof-repo/docs/research/notes.md`
- `sandbox/git-proof-repo/wrangler.toml`
- `docs/product/tasks/2026-03-12-scope-review-cli/input.json`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/input-drift.json`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-drift.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/input-git.json`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-git.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/input-git-drift.json`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-git-drift.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/changed-manifest.txt`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-manifest.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/changed-manifest-drift.json`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-manifest-drift.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/changed-diff.patch`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/changed-diff-drift.patch`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff-drift.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff-stdin.md`

## Scope Result

Status: respected

The runtime change stayed exactly inside the declared file boundary.

The surrounding task packet and cycle-review updates are meta-work and are not counted as runtime scope drift.
