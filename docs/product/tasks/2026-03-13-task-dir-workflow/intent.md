# Intent

Last updated: 2026-03-13
Task status: completed

## Change Intent

Turn the minimal task artifact into a real product path instead of a manual JSON convention.

## Must Preserve

- keep the workflow small and repo-native
- avoid changing the core classification logic
- reduce path juggling without adding hidden inference

## Must Not Introduce

- markdown parsing as task truth
- repo-specific logic
- a second competing task format

## Success Condition

A user can scaffold a task directory with one command and run `specpunk check` from that directory without manually wiring `input.json` and `generated-review.md` paths.
