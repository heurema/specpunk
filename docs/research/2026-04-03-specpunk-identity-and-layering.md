# Specpunk Identity and Layering

Date: 2026-04-03
Status: active research track
Priority: P0

## Research question

Is `specpunk` primarily a correctness substrate, a product shell, or both? If both, what must live in `core` versus `shell`?

## Why this matters

Recent work added `punk init`, `punk start`, and `punk go --fallback-staged`, which is product-shell behavior. At the same time the strongest differentiated value still comes from kernel concerns: `Contract`, `Scope`, `gate`, `proof`, and explicit failure semantics.

Without an explicit layer split, fixes keep landing opportunistically and the shell risks polluting the kernel.

## Current hypothesis

`specpunk` should be defined as:

- **Layer A: correctness substrate**
- **Layer B: operator shell**

### Layer A — correctness substrate
Owns:
- `Goal` intake normalization
- `Contract`
- `Scope`
- isolated `Workspace`
- `Run`
- `Decision`
- `Proof`
- durable `Ledger`
- project identity
- safety invariants

### Layer B — operator shell
Owns:
- `punk init`
- `punk go --fallback-staged`
- `punk start`
- summaries
- blocked/recovery UX
- repo-local guidance files
- shell-facing status output

## Evidence from reference systems

- **Gas Town** proves a product shell can radically lower operator cognitive load.
- **Gas City** proves that shell concerns and platform primitives should not be fused.
- **Beads** suggests the substrate must expose durable work state independently of shell flavor.

## Risks if unresolved

- shell behavior leaks into substrate code
- substrate invariants become prompt-dependent
- roadmap drifts toward feature accumulation instead of architecture
- contributor agents cannot tell whether a change belongs in `core` or `shell`

## Recommended next slice

Write an explicit architecture note that states:
- the two-layer split
- allowed responsibilities per layer
- forbidden cross-layer shortcuts

## Acceptance evidence

This track is done when:
- `docs/product/ARCHITECTURE.md` has an explicit `core vs shell` split
- new commands and docs can be classified unambiguously as `core` or `shell`
- contributor instructions reference this split
