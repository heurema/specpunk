# Google UUID PR Context Proof Task

Last updated: 2026-03-13
Owner: Vitaly
Status: completed

## Task

Run the Specpunk review flow on a real PR discussion from `google/uuid`, derive the boundary from the PR text and review comments, and compare the resulting review object against the raw diff.

## Why This Task

The historical-commit proof still framed the task from commit messages.
This task uses maintainer review text, contributor replies, and the PR body to shape the boundary.

## External Repo

- repo: `https://github.com/google/uuid`
- local path: `sandbox/external-google-uuid`
- PR: `#166`
- PR URL: `https://github.com/google/uuid/pull/166`
- base revision: `d55c313`
- initial PR commit: `a0eddd2`
- revised PR commit: `54f9572`
- merged revision: `0e97ed3`

## Runtime Scope

Allowed repo scope:

- `sandbox/bin/specpunk`
- `sandbox/external-google-uuid/**`
- `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof.md`
- `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/context.md`
- `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/comparison.md`
- `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/intent.md`
- `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/scope.md`
- `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/evidence.md`
- `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/review.md`
- `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/input-pr-context.json`
- `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/raw-diff-initial.patch`
- `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/raw-diff-revised.patch`
- `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/generated-review-initial.md`
- `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/generated-review-revised.md`

Blocked:

- `cmd/specpunk/**`
- `internal/check/**`
- `site/**`
- `docs/research/**`

## Outcome

- Specpunk now has a proof driven by real reviewer language, not only by commit messages
- the same PR yields `inspect` before review tightening and `approve` after the contributor removes the extra scope
- the wedge now has a direct `raw diff` versus `review object` comparison on third-party material

## Artifact Links

- [context.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/context.md)
- [comparison.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/comparison.md)
- [intent.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/intent.md)
- [scope.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/scope.md)
- [evidence.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/evidence.md)
- [review.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/review.md)
- [input-pr-context.json](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/input-pr-context.json)
- [raw-diff-initial.patch](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/raw-diff-initial.patch)
- [generated-review-initial.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/generated-review-initial.md)
- [raw-diff-revised.patch](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/raw-diff-revised.patch)
- [generated-review-revised.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/generated-review-revised.md)
- [external repo](/Users/vi/personal/specpunk/sandbox/external-google-uuid)
