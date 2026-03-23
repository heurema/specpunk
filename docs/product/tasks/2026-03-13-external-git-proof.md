# External Git Proof Task

Last updated: 2026-03-13
Owner: Vitaly
Status: completed

## Task

Run the Specpunk review flow on a small external git-backed repo instead of the internal sandbox fixture.

## Why This Task

The earlier git-backed proof used a controlled repo designed specifically for Specpunk.
This task checks the same wedge on a real cloned OSS repo with its own file layout, tests, and history.

## External Repo

- repo: `https://github.com/matryer/is`
- local path: `sandbox/external-matryer-is`
- baseline remote head: `0d9f7ec`
- bounded proof commit: `dc0c39e`
- drift proof commit: `be9247a`

## Runtime Scope

Allowed repo scope:

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

- `site/**`
- `docs/research/**`
- changes to Specpunk CLI logic itself

## Outcome

- Specpunk now has a stored git-backed proof on a cloned OSS repo, not only on the internal fixture
- the same `--changed-git` adapter produces both `approve` and `inspect` on that external repo
- the proof also captures a real execution constraint: using a built binary is the practical bridge across Go module boundaries
- the first wedge is now validated on three layers:
  - internal structured input
  - internal sandbox git repo
  - external git-backed repo

## Artifact Links

- [intent.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof/intent.md)
- [scope.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof/scope.md)
- [evidence.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof/evidence.md)
- [review.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof/review.md)
- [input-bounded.json](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof/input-bounded.json)
- [generated-review-bounded.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof/generated-review-bounded.md)
- [input-drift.json](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof/input-drift.json)
- [generated-review-drift.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof/generated-review-drift.md)
- [external repo](/Users/vi/personal/specpunk/sandbox/external-matryer-is)
