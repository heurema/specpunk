# Google UUID History Proof Task

Last updated: 2026-03-13
Owner: Vitaly
Status: completed

## Task

Run the Specpunk review flow on real historical commits from `google/uuid` instead of on changes we authored ourselves.

## Why This Task

The sandbox proof and the first external proof still depended on changes we created.
This task checks whether the wedge still reads cleanly when the input is a third-party repo's actual history and commit messages.

## External Repo

- repo: `https://github.com/google/uuid`
- local path: `sandbox/external-google-uuid`
- bounded historical commit: `0e97ed3`
- drift historical commit: `d55c313`

## Runtime Scope

Allowed repo scope:

- `sandbox/bin/specpunk`
- `sandbox/external-google-uuid/**`
- `docs/product/tasks/2026-03-13-google-uuid-history-proof.md`
- `docs/product/tasks/2026-03-13-google-uuid-history-proof/intent.md`
- `docs/product/tasks/2026-03-13-google-uuid-history-proof/scope.md`
- `docs/product/tasks/2026-03-13-google-uuid-history-proof/evidence.md`
- `docs/product/tasks/2026-03-13-google-uuid-history-proof/review.md`
- `docs/product/tasks/2026-03-13-google-uuid-history-proof/input-bounded.json`
- `docs/product/tasks/2026-03-13-google-uuid-history-proof/input-drift.json`
- `docs/product/tasks/2026-03-13-google-uuid-history-proof/generated-review-bounded.md`
- `docs/product/tasks/2026-03-13-google-uuid-history-proof/generated-review-drift.md`

Blocked:

- `cmd/specpunk/**`
- `internal/check/**`
- `site/**`
- `docs/research/**`

## Outcome

- Specpunk now has a stored proof based on third-party historical commits, not only authored fixtures
- `--changed-git` produces `approve` for a bounded code/test commit and `inspect` for a docs-labeled commit that also touched logic files
- the wedge now has evidence across four layers:
  - internal structured input
  - internal sandbox git repo
  - external repo with our authored commits
  - external repo historical commits

## Artifact Links

- [intent.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof/intent.md)
- [scope.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof/scope.md)
- [evidence.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof/evidence.md)
- [review.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof/review.md)
- [input-bounded.json](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof/input-bounded.json)
- [generated-review-bounded.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof/generated-review-bounded.md)
- [input-drift.json](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof/input-drift.json)
- [generated-review-drift.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof/generated-review-drift.md)
- [external repo](/Users/vi/personal/specpunk/sandbox/external-google-uuid)
