# Specpunk Repo Fixture Matrix

Date: 2026-04-03
Status: active research track
Priority: P1

## Research question

Which repository classes must be covered by automated smoke/regression fixtures so that dogfood-discovered failures become repeatable tests instead of recurring surprises?

## Why this matters

Recent critical bugs were discovered only via external dogfood:
- stale global events
- project identity collisions
- `.build/` scope pollution
- exact refine scope drift
- fresh intake hangs

That means internal happy paths are not enough.

## Required repo classes

1. fresh repo with no history
2. repo with legacy global events
3. repo with corrupted global events
4. SwiftPM repo with populated `.build/`
5. JS/TS repo with `node_modules/` and build outputs
6. Python repo with `.venv/`, `dist/`, `.pytest_cache/`
7. repo with ambiguous basename collision across different roots
8. repo with generated agent instructions already present

## Required command matrix

For each relevant repo class, at minimum exercise:
- `punk init --enable-jj --verify`
- `punk start "<goal>"`
- `punk go --fallback-staged "<goal>"`
- `punk status`
- refine path where exact `allowed_scope` matters

## Working hypothesis

A small number of high-value fixture repos and shell-level smoke tests will catch more real regressions than deeper unit tests alone.

## Recommended next slices

1. create a `fixtures/` directory or generated fixture builder helpers
2. add smoke tests per repo class
3. require every reliability bugfix to add one fixture regression if possible

## Acceptance evidence

This track is done when:
- the recent external dogfood failures are reproducible locally in tests
- adding a new repo class becomes a standard maintenance move
- contributors can point to fixture coverage before claiming a flow is reliable
