# Intent

Last updated: 2026-03-13
Task status: completed

## Change Intent

Prove that Specpunk can derive a useful review boundary from real PR discussion text, not only from commit messages.

## Must Preserve

- use the current `specpunk` behavior with no repo-specific logic
- derive the boundary from the PR body and maintainer feedback
- keep the proof reproducible from public third-party material

## Must Not Introduce

- synthetic commits
- manual changed-file lists
- retrospective boundary tuning from the final diff alone

## Success Condition

The same PR context produces `inspect` on the initial too-wide commit and `approve` on the revised commit, with a clear comparison between `raw diff` and `review object`.
