# Google UUID Issue Proof Task

Last updated: 2026-03-13
Owner: Vitaly
Status: completed

## Task

Run the Specpunk review flow from a real external issue text, derive the boundary from that issue alone, and compare the resulting review object against the final merged diff.

## Why This Task

The PR-context proof still used maintainer review language to tighten the boundary.
This task removes that crutch and uses only the issue statement as the task source of truth.

## External Repo

- repo: `https://github.com/google/uuid`
- local path: `sandbox/external-google-uuid`
- issue: `#137`
- issue URL: `https://github.com/google/uuid/issues/137`
- merged PR: `#141`
- merged revision: `9ee7366`

## Runtime Scope

Allowed repo scope:

- `sandbox/bin/specpunk`
- `sandbox/external-google-uuid/**`
- `docs/product/tasks/2026-03-13-google-uuid-issue-proof.md`
- `docs/product/tasks/2026-03-13-google-uuid-issue-proof/context.md`
- `docs/product/tasks/2026-03-13-google-uuid-issue-proof/comparison.md`
- `docs/product/tasks/2026-03-13-google-uuid-issue-proof/intent.md`
- `docs/product/tasks/2026-03-13-google-uuid-issue-proof/scope.md`
- `docs/product/tasks/2026-03-13-google-uuid-issue-proof/evidence.md`
- `docs/product/tasks/2026-03-13-google-uuid-issue-proof/review.md`
- `docs/product/tasks/2026-03-13-google-uuid-issue-proof/input-issue.json`
- `docs/product/tasks/2026-03-13-google-uuid-issue-proof/raw-diff.patch`
- `docs/product/tasks/2026-03-13-google-uuid-issue-proof/generated-review.md`

Blocked:

- `cmd/specpunk/**`
- `internal/check/**`
- `site/**`
- `docs/research/**`

## Outcome

- Specpunk now has a proof where the task boundary comes from issue text rather than from PR discussion
- the issue-derived boundary yields a coherent `approve` on the merged change
- the wedge now covers fixture, authored external changes, historical commits, PR discussion, and issue text

## Artifact Links

- [context.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-issue-proof/context.md)
- [comparison.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-issue-proof/comparison.md)
- [intent.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-issue-proof/intent.md)
- [scope.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-issue-proof/scope.md)
- [evidence.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-issue-proof/evidence.md)
- [review.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-issue-proof/review.md)
- [input-issue.json](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-issue-proof/input-issue.json)
- [raw-diff.patch](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-issue-proof/raw-diff.patch)
- [generated-review.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-issue-proof/generated-review.md)
- [external repo](/Users/vi/personal/specpunk/sandbox/external-google-uuid)
