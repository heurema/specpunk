# Intent

Last updated: 2026-03-13
Task status: completed

## Change Intent

Prove that Specpunk can review a real git-backed OSS change outside the internal fixture and still make boundary drift obvious.

## Must Preserve

- use the existing Specpunk CLI without patching it for this proof
- choose a small repo with fast tests and understandable file boundaries
- keep the external change itself compact and technically defensible
- keep the resulting review artifact short and decision-oriented

## Must Not Introduce

- ad-hoc parsing that only works for this repo
- changes to the main Specpunk wedge just to make the proof pass
- a fake task where every touched file was predesigned around the tool

## Success Condition

An external git-backed repo produces one stored `approve` artifact and one stored `inspect` artifact through `--changed-git`, with tests still passing in that repo.
