# Comparison

Last updated: 2026-03-13
Task status: completed

## Raw Diff Versus Review Object

Raw diff for merged revision `9ee7366` tells the reviewer:

- `2 files changed`
- `uuid.go`
- `uuid_test.go`
- about `82` lines added

That is clean, but the reviewer still has to infer whether the change shape matches the original issue.

Specpunk review object adds:

- the issue-derived task statement
- the explicit allowed boundary
- confirmation that no unrelated files were touched
- evidence that tests pass on the merged revision
- a compact reviewer posture instead of making the reviewer reconstruct intent from code alone

## Why This Proof Matters

This is the first proof where:

- the source of truth is an issue, not a PR
- the boundary is derived without maintainer review comments
- the output stays compact while still giving more review meaning than the raw diff
