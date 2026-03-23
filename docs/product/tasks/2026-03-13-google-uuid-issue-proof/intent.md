# Intent

Last updated: 2026-03-13
Task status: completed

## Change Intent

Prove that Specpunk can derive a useful review boundary directly from real issue text and still produce a coherent review object against the merged diff.

## Must Preserve

- use issue text as the task source of truth
- keep the proof reproducible from public third-party material
- avoid repo-specific special casing

## Must Not Introduce

- PR-review-derived boundary rules
- manual changed-file lists
- synthetic commits or edited history

## Success Condition

The issue-derived boundary yields a clean `approve` on the merged revision, and the resulting review object is easier to reason about than the raw diff alone.
