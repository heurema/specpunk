# Specpunk Primitives and Derived Mechanisms

Date: 2026-04-03
Status: active research track
Priority: P1

## Research question

Which concepts in `specpunk` are true primitives, and which are only derived UX or orchestration mechanisms?

## Why this matters

`gascity` is strongest where it states primitives and invariants clearly. `specpunk` currently has many real parts, but the primitive model is still mostly implicit.

## Candidate primitives

1. `Project`
2. `Goal`
3. `Contract`
4. `Scope`
5. `Workspace`
6. `Run`
7. `Decision`
8. `Proof`
9. `Ledger`

## Candidate derived mechanisms

- `init`
- `start`
- `go`
- staged fallback
- `refine`
- `status`
- `inspect`
- project overlays / project bootstrap instructions
- research / council / eval systems

## Proposed v1 primitive taxonomy

The current best working split is:

| Concept | Class | Why |
|---|---|---|
| `Project` | primitive | identity and repo boundary |
| `Goal` | primitive | normalized user intent anchor |
| `Contract` | primitive | executable bounded spec |
| `Scope` | primitive | safety boundary for execution |
| `Workspace` | primitive | isolated mutation context |
| `Run` | primitive | one execution attempt |
| `DecisionObject` | primitive | final verification truth |
| `Proofpack` | primitive | immutable verification artifact |
| `Ledger` | primitive | durable evented truth and projections |
| `Feature` | durable grouping, non-final | important long-lived grouping, but still allowed to evolve as ledger design sharpens |
| `Task` | derived orchestration unit | queue/execution wrapper around approved work |
| `Receipt` | canonical artifact, non-primitive | execution truth artifact attached to a run |

## Proposed v1 derived mechanism map

| Mechanism | Class | Built from |
|---|---|---|
| `init` | shell bootstrap mechanism | `Project`, `Ledger`, bootstrap guidance |
| `start` | staged shell intake | `Goal` + `Contract` draft path |
| `go` | autonomous shell intake | `Goal` + `Contract` + `Run` + `DecisionObject` + `Proofpack` |
| staged fallback | shell recovery mechanism | `Goal` + `Contract` + `Ledger`-projected recovery linkage |
| `plot` | substrate permission boundary | `Contract` / `Scope` preparation |
| `cut` | substrate permission boundary | `Workspace` + `Run` + `Receipt` |
| `gate` | substrate permission boundary | `DecisionObject` + `Proofpack` |
| `status` | shell/projected read surface | `Ledger` projections |
| `inspect` | shell/projected read surface | canonical artifacts + `Ledger` projections |
| project overlays | derived project-intelligence mechanism | `Project` + repo-local constraints + skills |
| `council` | advisory derived subsystem | packets + artifacts + optional synthesis |
| `eval` | derived ratchet subsystem | artifacts + baselines + promotion evidence |
| `research` | derived bounded inquiry subsystem | packets + artifacts + synthesis output |

## Design rule

If something can be recomposed from primitives without changing truth, it is not a primitive.

## Evidence from current code

- `plot / cut / gate` already act more like permission boundaries than user-facing primitives.
- `start` and `go` are shell mechanisms assembled from deeper lifecycle operations.
- `proof` is a true primitive because it anchors verification truth.
- `allowed_scope` is part of primitive safety semantics, not a UX nicety.
- the new v0 architecture steering slice keeps `architecture-signals.json`, `architecture-brief.md`, and `architecture-assessment.json` as derived artifacts under `.punk/` instead of minting a new primitive alongside `Contract` or `Proofpack`

## Architecture steering v0 implementation note

- artifact paths:
  - `.punk/contracts/<feature-id>/architecture-signals.json`
  - `.punk/contracts/<feature-id>/architecture-brief.md`
  - `.punk/runs/<run-id>/architecture-assessment.json`
- default thresholds:
  - `warn_file_loc >= 600`
  - `critical_file_loc >= 1200`
  - `critical_scope_roots > 1`
  - `warn_expected_interfaces > 2`
  - `warn_import_paths > 5`
- trigger logic:
  - `plot` always refreshes `architecture-signals.json`
  - `plot` writes `architecture-brief.md` when signals are `critical`, `--architecture on` is used, or contract-side architecture integrity already exists
  - enforceable commitments stay inside the persisted contract document under `architecture_integrity`
- gate decision logic:
  - `Escalate` if persisted architecture signals are `critical` and the approved contract has no `architecture_integrity`
  - `Block` if `touched_roots_max`, `file_loc_budgets[]`, or deterministically verifiable `forbidden_path_dependencies[]` are breached
  - keep the assessment in the proof chain through `check_refs` / hashed proof inputs
- enforced now vs deferred:
  - enforced now: touched-root budgets, file LOC budgets, direct local Rust dependency edges, direct local JS/TS relative imports
  - deferred: broader language coverage and whole-repo dependency graph analysis

## Risks if left implicit

- new commands create hidden domain concepts
- contributor agents add logic to the wrong layer
- docs drift faster than code
- reliability bugs become harder to localize because responsibilities are fuzzy

## Recommended next slices

1. Add an explicit primitive table to `ARCHITECTURE.md`
2. Add a matching derived-mechanisms table
3. Add CLI-facing mapping that shows which commands are compositions versus permission boundaries
4. Require roadmap entries and contributor notes to name the primitive they touch

## Acceptance evidence

This track is done when:
- new roadmap items can be mapped to primitives unambiguously
- contributor docs can say "this change touches primitive X" instead of relying on intuition
- shell commands are described as compositions, not as ontology
- `CLI.md` and `ARCHITECTURE.md` describe the same primitive/derived split
