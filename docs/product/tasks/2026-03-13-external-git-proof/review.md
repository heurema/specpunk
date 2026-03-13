# Review

Last updated: 2026-03-13
Reviewer posture: inspect and approve
Task status: completed

## Decision

Approve.

## Why

- the proof uses the existing CLI instead of a special-case adapter
- the external repo is small but real, with its own tests and file layout
- the bounded and drift ranges are both understandable from the resulting artifacts
- the proof increases confidence in the wedge more than another internal demo would
- the proof exposed a real cross-module execution detail without requiring CLI redesign

## What Improved

- Specpunk now has an external git-backed proof, not only an internal sandbox proof
- the first wedge has crossed from self-designed fixture to cloned OSS repo
- `--changed-git` now has a stored bounded/inspect pair outside temporary tests

## Remaining Risk

- the external repo is still very small and friendly
- the task is still chosen by us, not by an external maintainer or issue
- this validates scope review shape, not broader product-market fit
- external execution currently prefers a built binary over `go run` when crossing module boundaries

## Next Reviewable Change

Run the same flow on a less curated external repo task where the boundary is less obvious and the review decision is harder.
