# Specpunk North Roadmap

Last updated: 2026-04-11
Owner: Vitaly
Status: active

## Purpose

This document is the durable strategic backlog for `specpunk`.

Use it when:
- a session starts cold
- roadmap work must survive compaction or context loss
- contributors need to know which high-level tracks matter next
- contributors need a pointer to the short active roadmap for current-forward work

`ROADMAP-v2.md` remains the implementation roadmap.
This file is the strategic map that explains **what** must be strengthened and **why**.
`CURRENT-ROADMAP.md` is the short operational roadmap for active work right now.

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
- When `punk` blocks or escalates for deterministic runtime-style reasons while operating inside another repo, the shell should emit one explicit incident path: capture locally first (`punk incident capture <proof-id>`), then optionally promote into the upstream `specpunk` repo with `punk incident promote <incident-id> --repo <path>`. If the operator explicitly opts in with `--auto-run`, that upstream promotion may continue through approve/execute/gate/proof inside the target repo and write those refs back onto `prom_<id>`, but only when the effective promote target matches deterministic local `specpunk` markers; otherwise the lane should stay draft-only. Failed internal attempts should also leave durable attempt/failure metadata on `prom_<id>` so retry can use `punk incident rerun <promotion-id> --auto-run` without relying on shell history. Repeated upstream targets should be configurable with explicit precedence: CLI flag first, then repo-local incident defaults, then operator-wide global defaults.
- External GitHub submission now exists as an explicit opt-in lane: `punk incident submit <incident-id> --github owner/repo` prepares a sanitized bundle first, and `--publish` is the only step that actually talks to GitHub.

## Cross-cutting harness note

- Harness engineering should be implemented as a derived harness/evidence plane across existing tracks, not as a new primitive layer.
- Near-term focus: inspectable project harness packets, typed evidence paths for `gate` / `proof`, and stronger harness-linked recovery continuity in the work ledger.
- Reference note: `docs/research/2026-04-08-specpunk-harness-engineering.md`.

## Cross-cutting architecture-steering note

- The first `plot -> cut -> gate` slice now also carries deterministic architecture steering without adding a fourth runtime mode.
- `plot` should always persist `architecture-signals.json`, may persist `architecture-brief.md`, and should freeze any enforceable architecture commitments inside the contract document.
- `gate` should write `architecture-assessment.json`, escalate when critical architecture review was required but missing from the approved contract, block on breached enforceable architecture commitments (including cheap deterministic forbidden dependency edges), and carry the assessment ref/hash into proof.
- operators should be able to inspect the same architecture refs through existing surfaces (`punk inspect work`, contract JSON inspect, proof JSON inspect) instead of relying on transient shell output.
- Keep this mechanism deterministic: repo scan, contract inspection, receipt/check output verification only. No council, hosted memory, or LLM-based runtime judging.

## Cross-cutting autonomy note

- The intended operator model is: user at the first step, system inside the loop, user again at the last step.
- If the loop needs a second technical opinion, the default escalation target should be another model/provider or a bounded council protocol, not the user.
- Sensitive decisions still require single-model handling when multi-provider escalation would broaden data exposure beyond policy.

## Cross-cutting provider-alignment note

- `specpunk` should stay a **local-first correctness and stewardship layer** over provider-native agent runtimes.
- Build locally only what protects boundedness, verification, rollback, and proof.
- Wrap provider-native runtimes, tools, tracing, and session/memory primitives instead of rebuilding them in the kernel.
- Prefer simplification over abstraction growth when a provider ships a stable primitive that replaces custom `specpunk` logic.
- Reference note: `docs/research/2026-04-11-provider-alignment-build-wrap-avoid.md`.
- Accepted ADR: `docs/product/ADR-provider-alignment.md`.

## Provider-aligned pruning table

Use this table before adding roadmap work that increases architecture depth.

| Track / idea | Decision | Rule |
|---|---|---|
| Identity and layering | **keep** | strengthens the kernel/shell split and reduces drift |
| Work ledger | **keep** | durable local truth is a core differentiator |
| Primitives and derived mechanisms | **keep** | prevents accidental kernel growth |
| Repo fixture matrix | **keep** | reliability and regressions are core |
| One-face operator shell | **keep** | simpler operator UX is a product advantage |
| Harness / typed evidence | **keep** | strengthens proof without adding a new primitive layer |
| Autonomous loop | **downgrade** | keep as a bounded goal-to-result loop over existing primitives, not as a giant autonomous platform |
| Project intelligence | **downgrade** | prefer structured overlays and repo anchors, not a large memory/intelligence subsystem |
| Council | **downgrade** | selective advisory mechanism only, never a default tax on all work |
| Research subsystem | **downgrade** | bounded support mechanism, not a product core that keeps expanding |
| Multi-model divergence everywhere | **cut / avoid** | use only when correctness materially improves |
| Provider-zoo UX | **cut / avoid** | adapters matter, provider-dashboard behavior does not |
| Custom universal agent runtime | **cut / avoid** | duplicate of provider direction |
| Large internal memory platform | **cut / avoid** | duplicate of provider session/state direction |
| Free-text-heavy orchestration logic | **cut / avoid** | repeatedly harms reliability and inspectability |

Default decision:

> if a roadmap item does not clearly improve boundedness, reliability, inspectability, or operator simplicity, it should be downgraded or cut

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
