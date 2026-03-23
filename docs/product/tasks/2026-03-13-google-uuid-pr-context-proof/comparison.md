# Comparison

Last updated: 2026-03-13
Task status: completed

## Raw Diff Versus Review Object

### Initial PR Commit `a0eddd2`

Raw diff tells the reviewer:

- `3 files changed`
- `json_test.go`
- `uuid.go`
- `uuid_test.go`

That is not enough to tell whether `json_test.go` is intentional, accidental, or a backward-compatibility leak.

Specpunk review object adds:

- explicit allowed boundary from PR text and maintainer review
- immediate `inspect` decision
- one visible out-of-scope file: `json_test.go`
- reason that the change exceeded the bounded functional scope

### Revised PR Commit `54f9572`

Raw diff tells the reviewer:

- `2 files changed`
- `uuid.go`
- `uuid_test.go`

That is better, but the reviewer still has to infer why this smaller shape is the acceptable one.

Specpunk review object adds:

- the same declared boundary as before
- `approve` once the diff falls back inside that boundary
- attached evidence that tests pass on the revised commit

## Why This Proof Matters

This is the first proof where:

- the task framing comes from real maintainer language
- the same PR moves from too-wide to bounded
- the value is not only classification, but compression of review reasoning
