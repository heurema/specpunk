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

## Design rule

If something can be recomposed from primitives without changing truth, it is not a primitive.

## Evidence from current code

- `plot / cut / gate` already act more like permission boundaries than user-facing primitives.
- `start` and `go` are shell mechanisms assembled from deeper lifecycle operations.
- `proof` is a true primitive because it anchors verification truth.
- `allowed_scope` is part of primitive safety semantics, not a UX nicety.

## Risks if left implicit

- new commands create hidden domain concepts
- contributor agents add logic to the wrong layer
- docs drift faster than code
- reliability bugs become harder to localize because responsibilities are fuzzy

## Recommended next slices

1. Add an explicit primitive table to `ARCHITECTURE.md`
2. Add a matching derived-mechanisms table
3. Require roadmap entries to name the primitive they touch

## Acceptance evidence

This track is done when:
- new roadmap items can be mapped to primitives unambiguously
- contributor docs can say "this change touches primitive X" instead of relying on intuition
- shell commands are described as compositions, not as ontology
