# Scope Review CLI Task

Last updated: 2026-03-12
Owner: Vitaly
Status: completed

## Task

Build the smallest repo-local tool that compares declared scope with actual changed files and emits a markdown review artifact.

## Why This Task

This is the first code-level proof of the wedge.

The earlier dogfood tasks proved the product shape on the public surface.
This task proves a minimal product behavior:

- take structured task input
- compare declared boundary with actual file changes
- produce a review artifact instead of relying on chat memory

## Runtime Scope

Allowed repo scope:

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
- `docs/product/tasks/2026-03-12-scope-review-cli/input-git-drift.json`
- `docs/product/tasks/2026-03-12-scope-review-cli/changed-manifest.txt`
- `docs/product/tasks/2026-03-12-scope-review-cli/changed-manifest-drift.json`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-manifest.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-manifest-drift.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/changed-diff.patch`
- `docs/product/tasks/2026-03-12-scope-review-cli/changed-diff-drift.patch`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff-drift.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff-stdin.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-git.md`
- `docs/product/tasks/2026-03-12-scope-review-cli/generated-review-git-drift.md`

Blocked:

- `site/**`
- `docs/research/**`
- Cloudflare configuration
- any new dependency or framework

## Outcome

- a stdlib `Go` CLI now exists under `cmd/` and `internal/`
- the CLI can render a markdown review artifact from structured JSON input
- the CLI can also accept changed files from an external manifest
- the CLI can also derive changed files from a unified diff or git patch file
- the CLI can also derive changed files from diff content streamed through stdin
- the CLI can also derive changed files from an explicit git revspec
- stored git-backed approve/inspect artifacts now exist from a stable sandbox repo
- bounded and drift examples are both stored in the repo
- the product now has a first code-wedge proof, not only UI proofs

Current note:
- the top-level workspace is still not a git checkout, but `--changed-git` is now dogfooded through the checked-in sandbox repo at `sandbox/git-proof-repo/`

## Artifact Links

- [intent.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/intent.md)
- [scope.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/scope.md)
- [evidence.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/evidence.md)
- [review.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/review.md)
- [input.json](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/input.json)
- [generated-review.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/generated-review.md)
- [input-drift.json](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/input-drift.json)
- [generated-review-drift.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/generated-review-drift.md)
- [input-git.json](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/input-git.json)
- [generated-review-git.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/generated-review-git.md)
- [input-git-drift.json](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/input-git-drift.json)
- [generated-review-git-drift.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/generated-review-git-drift.md)
- [changed-manifest.txt](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/changed-manifest.txt)
- [generated-review-manifest.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/generated-review-manifest.md)
- [changed-manifest-drift.json](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/changed-manifest-drift.json)
- [generated-review-manifest-drift.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/generated-review-manifest-drift.md)
- [changed-diff.patch](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/changed-diff.patch)
- [generated-review-diff.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff.md)
- [generated-review-diff-stdin.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff-stdin.md)
- [changed-diff-drift.patch](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/changed-diff-drift.patch)
- [generated-review-diff-drift.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff-drift.md)
- [sandbox git proof repo](/Users/vi/personal/specpunk/sandbox/git-proof-repo)
