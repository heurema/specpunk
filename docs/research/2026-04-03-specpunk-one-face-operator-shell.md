# Specpunk One-Face Operator Shell

Date: 2026-04-03
Status: active research track
Priority: P1

## Research question

How should `specpunk` reduce operator reading burden so that initialized repos have one obvious top-level path from plain goal to action?

## Why this matters

The strongest lesson from the `Mayor` idea is not roleplay. It is operator ergonomics:

- one obvious entrypoint
- one summary surface
- one blocked/recovery surface
- less worker noise

`specpunk` improved here with `init`, `start`, `go`, `AGENTS.md`, and staged fallback, but it is still more tool-facing than operator-facing.

## Current state

Good:
- plain goal intake exists
- `go --fallback-staged` is a useful default
- generated repo-local instructions exist

Weak:
- multiple mode-level commands are still very visible
- blocked autonomy still requires understanding underlying pipeline stages
- there is not yet a single "face" or command contract that feels final

## Working hypothesis

The right shell should make these true:
- user speaks in goals
- shell chooses autonomous or staged path
- shell summarizes progress in one surface
- shell explains blockers in one surface
- lower layers remain available, but optional

## Anti-goals

- do not import Gas Town role mythology
- do not hide safety-critical details behind fake certainty
- do not collapse `gate` truth into vague shell chatter

## Recommended next slices

1. unify `go` result packaging further around one concise operator summary
2. add durable blocked/recovery objects to the ledger, not just command hints
3. make repo-root `AGENTS.md` the canonical shell convention for external agents

## Acceptance evidence

This track is done when:
- plain goal in an initialized repo has one obvious execution path
- blocked autonomy has one obvious recovery path
- operators do not need to understand `plot/cut/gate` to use the happy path
