# Intent

Last updated: 2026-03-13
Task status: completed

## Change Intent

Prove that Specpunk can review a real third-party historical commit range without relying on changes we authored.

## Must Preserve

- use existing `specpunk` behavior with no special-casing for the target repo
- derive boundaries from the commit/task framing, not from files we edited ourselves
- keep the proof concrete, short, and reproducible

## Must Not Introduce

- synthetic commits in the target repo
- manual changed-file lists
- repo-specific parsing logic

## Success Condition

One historical commit range produces `approve` and another produces `inspect`, both via `--changed-git`, with the resulting reasoning still feeling coherent to a human reviewer.
