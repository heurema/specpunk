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

## Proposed one-face shell contract

For an initialized repo, the default operator contract should be:

```text
user gives a plain goal
-> shell chooses the default path
-> shell returns one concise progress or blocker summary
-> shell offers one obvious next step
```

The current best approximation is:

```bash
punk go --fallback-staged "<goal>"
```

But the shell contract matters more than the exact command name.

## Default happy path

The happy path should feel like one continuous top-level flow:

1. accept plain goal
2. normalize to bounded work
3. execute autonomy by default
4. summarize result in one operator-facing surface
5. only expose lower-level mechanics when needed

The operator should not need to mentally orchestrate:

- `plot`
- `cut`
- `gate`
- `proof`

to use the normal path.

## Required shell summary contract

The shell summary should answer, in one place:

1. what goal is being worked on?
2. what is the current outcome?
3. what artifact is authoritative right now?
4. what should happen next?

That means the shell summary should converge around fields like:

- `goal`
- `project`
- `outcome`
- `basis`
- `authoritative_ref`
- `next_action`

## Required blocked/recovery contract

When autonomy blocks, the shell should still feel like one face.

The operator should get:

1. one concise blocker summary
2. one obvious recovery path
3. one authoritative artifact or ref to inspect

The operator should **not** need to infer recovery by reading raw pipeline internals.

The current best approximation is:

- non-zero exit
- `Outcome: blocked|escalated`
- `Basis: ...`
- `Proof: ...`
- `Recovery: punk start "<goal>"`

## Expert escape hatch rule

`plot`, `cut`, and `gate` remain necessary, but they should be treated as:

- expert surfaces
- debugging surfaces
- explicit control surfaces

not as the first thing a normal operator must learn.

## Contributor implication

When proposing shell work, contributors should be able to say:

- what the one-face happy path becomes
- what the one-face blocked path becomes
- which lower-level surfaces remain expert-only

If a proposed shell change increases reading burden, it is probably moving in the wrong direction.

## Anti-goals

- do not import Gas Town role mythology
- do not hide safety-critical details behind fake certainty
- do not collapse `gate` truth into vague shell chatter
- do not make the happy path require users to understand mode-level machinery

## Recommended next slices

1. unify `go` result packaging further around one concise operator summary
2. add durable blocked/recovery objects to the ledger, not just command hints
3. make repo-root `AGENTS.md` the canonical shell convention for external agents
4. ensure `status` and future `inspect work` surfaces expose the same one-face summary fields

## Acceptance evidence

This track is done when:
- plain goal in an initialized repo has one obvious execution path
- blocked autonomy has one obvious recovery path
- operators do not need to understand `plot/cut/gate` to use the happy path
- shell-facing docs all describe the same happy-path and blocked-path contract
