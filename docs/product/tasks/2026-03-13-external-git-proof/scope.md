# Scope

Last updated: 2026-03-13
Task status: completed

## Declared Runtime Scope

Allowed:

- `sandbox/bin/specpunk`
- `sandbox/external-matryer-is/is.go`
- `sandbox/external-matryer-is/is_test.go`
- `sandbox/external-matryer-is/testdata/example_comment_test.go`
- `sandbox/external-matryer-is/README.md`
- `docs/product/tasks/2026-03-13-external-git-proof.md`
- `docs/product/tasks/2026-03-13-external-git-proof/intent.md`
- `docs/product/tasks/2026-03-13-external-git-proof/scope.md`
- `docs/product/tasks/2026-03-13-external-git-proof/evidence.md`
- `docs/product/tasks/2026-03-13-external-git-proof/review.md`
- `docs/product/tasks/2026-03-13-external-git-proof/input-bounded.json`
- `docs/product/tasks/2026-03-13-external-git-proof/input-drift.json`
- `docs/product/tasks/2026-03-13-external-git-proof/generated-review-bounded.md`
- `docs/product/tasks/2026-03-13-external-git-proof/generated-review-drift.md`

Blocked:

- `cmd/specpunk/**`
- `internal/check/**`
- `site/**`
- `docs/research/**`

## Actual Runtime Scope

Touched:

- `sandbox/bin/specpunk`
- `sandbox/external-matryer-is/is.go`
- `sandbox/external-matryer-is/is_test.go`
- `sandbox/external-matryer-is/testdata/example_comment_test.go`
- `sandbox/external-matryer-is/README.md`
- `docs/product/tasks/2026-03-13-external-git-proof.md`
- `docs/product/tasks/2026-03-13-external-git-proof/intent.md`
- `docs/product/tasks/2026-03-13-external-git-proof/scope.md`
- `docs/product/tasks/2026-03-13-external-git-proof/evidence.md`
- `docs/product/tasks/2026-03-13-external-git-proof/review.md`
- `docs/product/tasks/2026-03-13-external-git-proof/input-bounded.json`
- `docs/product/tasks/2026-03-13-external-git-proof/input-drift.json`
- `docs/product/tasks/2026-03-13-external-git-proof/generated-review-bounded.md`
- `docs/product/tasks/2026-03-13-external-git-proof/generated-review-drift.md`

## Scope Result

Status: respected

The proof stayed inside the declared file boundary.
The external repo change itself intentionally includes one drift commit, but that drift is part of the proof scenario, not runtime scope drift for this Specpunk task.
