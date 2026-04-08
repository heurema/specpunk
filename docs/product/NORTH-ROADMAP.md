# Specpunk North Roadmap

Last updated: 2026-04-08
Owner: Vitaly
Status: active

## Purpose

This document is the durable strategic backlog for `specpunk`.

Use it when:
- a session starts cold
- roadmap work must survive compaction or context loss
- contributors need to know which high-level tracks matter next

`ROADMAP-v2.md` remains the implementation roadmap.
This file is the strategic map that explains **what** must be strengthened and **why**.

## How to use this file in a new session

1. Read this file top-to-bottom
2. Open the linked research note for the chosen track
3. Pick one bounded slice only
4. Update both the code/docs and this roadmap status if the slice changes the strategic picture

## Strategic tracks

| Priority | Track | Why it matters | Research |
|---|---|---|---|
| P0 | Identity and layering | Prevent shell/kernel blur and keep architecture coherent | `docs/research/2026-04-03-specpunk-identity-and-layering.md` |
| P1 | Work ledger | Build one durable work plane instead of scattered state | `docs/research/2026-04-03-specpunk-work-ledger.md` |
| P1 | Primitives and derived mechanisms | Make responsibilities explicit for contributors and roadmap work | `docs/research/2026-04-03-specpunk-primitives-and-derived-mechanisms.md` |
| P1 | One-face operator shell | Reduce reading burden and keep one obvious happy path | `docs/research/2026-04-03-specpunk-one-face-operator-shell.md` |
| P1 | Repo fixture matrix | Turn dogfood failures into repeatable regression coverage | `docs/research/2026-04-03-specpunk-repo-fixture-matrix.md` |
| P2 | Autonomous loop | Turn `go --fallback-staged` into a true durable goal-to-result loop | `docs/research/2026-04-03-specpunk-autonomous-loop.md` |
| P2 | Project intelligence | Turn bootstrap + scoped skills into a coherent overlay system | `docs/research/2026-04-03-specpunk-project-intelligence.md` |

## Root synthesis

Read this first when you need the external comparison context:

- `docs/research/2026-04-03-gastown-beads-gascity-comparison.md`

Short version:
- `Beads` = durable work plane
- `Gas Town` = product shell / one-face UX
- `Gas City` = primitive/config platform
- `specpunk` should keep its stronger correctness kernel while learning from the other two layers

## Current strategic stance

`specpunk` should become:
- a **strong bounded correctness substrate**
- with a **durable work ledger**
- and a **simpler operator shell** on top

It should **not** become a role-heavy clone of Gas Town.

## Default sequencing

Recommended order unless a production bug forces reprioritization:

1. Identity and layering
2. Work ledger
3. Primitives and derived mechanisms
4. Repo fixture matrix
5. One-face operator shell
6. Autonomous loop
7. Project intelligence

## Current operator-shell note

- Goal-intake commands such as `punk start` and `punk go --fallback-staged` should fail early with one explicit recovery path when the workspace is not VCS-backed, instead of surfacing late repo-scan or adapter errors.

## Rules for future contributors

- One bounded slice per session
- Every strategic slice must link back to one track in this roadmap
- If a fix changes operator UX, update `README.md`, `docs/product/CLI.md`, and this file together
- If a fix changes architecture or invariants, update `docs/product/ARCHITECTURE.md` and the linked research note together
- If a fix comes from dogfood, add or update a fixture/regression note under the relevant track

## Exit criteria for this roadmap

This roadmap has done its job when:
- `specpunk` has a clear `core vs shell` split
- work continuity is inspectable through one durable plane
- initialized repos have one obvious path from plain goal to bounded execution
- the major dogfood repo classes are covered by fixture regressions
