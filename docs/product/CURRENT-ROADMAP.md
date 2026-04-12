# Specpunk Current Roadmap

Last updated: 2026-04-11
Owner: Vitaly
Status: active

## Purpose

This is the **current operational roadmap** for `specpunk`.

Use this file when you need:

- the active product direction right now
- a short list of tracks that are actually forward
- a simpler alternative to the historical planning documents

If this file conflicts with older roadmap or cycle docs, follow this file plus:

- `docs/product/ADR-provider-alignment.md`
- `docs/product/ARCHITECTURE.md`
- `docs/product/NORTH-ROADMAP.md`

## Product rule

`specpunk` should move forward as a:

- **bounded correctness substrate**
- **durable stewardship layer**
- **simple operator shell**

It should **not** drift back into a large parallel agent platform.

## Active tracks

### 1. Execution reliability and rollback

Goal:
- make bounded execution fail safely and predictably

Includes:
- rollback correctness
- patch/apply safety
- non-destructive blocked/failure paths
- isolated workspace integrity

Done means:
- failed runs do not corrupt repo state
- recovery is explicit, deterministic, and test-covered

### 2. Contract quality and structured targeting

Goal:
- reduce free-text contract drift and improve source-first targeting

Includes:
- repo anchors
- candidate ranking
- source-vs-generated pruning
- mixed-surface targeting quality

Done means:
- contracts select the right source surfaces with minimal manual correction
- generated/runtime paths do not pollute normal source work

### 3. Repo fixture matrix and dogfood regression coverage

Goal:
- convert repeated dogfood failures into named fixture coverage

Includes:
- repo classes
- regression notes
- fixture-backed acceptance for reliability fixes

Done means:
- known repo classes have repeatable regression coverage
- “fixed” means fixed outside the current repo too

### 4. Proof, gate, and typed evidence strengthening

Goal:
- make decisions and proof artifacts more inspectable without adding a new truth layer

Includes:
- typed evidence manifests
- stronger gate invariants
- proofpack clarity
- evidence linked to target/integrity checks

Done means:
- operator can see why a run passed or failed without reading shell chatter

### 5. One-face operator shell

Goal:
- keep `go`, `start`, `gate`, and `status` as the obvious happy path

Includes:
- simpler summaries
- better blocked/recovery UX
- less expert-surface leakage into default flows

Done means:
- a normal operator can stay on the shell happy path most of the time

### 6. Provider wrapping, not provider duplication

Goal:
- adopt provider-native runtimes, tools, tracing, and session primitives through adapters

Includes:
- adapter capability mapping
- provider feature wrapping
- simplification when external primitives become mature

Done means:
- `specpunk` gets simpler as provider capabilities mature
- kernel complexity does not grow just because providers add features

### 7. Bounded autonomous loop

Goal:
- improve goal-to-result flow without turning autonomy into a giant subsystem

Includes:
- bounded follow-up cycles
- safer auto-chaining
- stronger terminal blocker handling

Done means:
- autonomy improves operator throughput without weakening boundedness or inspectability

## Not current-forward

Do not treat these as active product-default tracks unless separately justified:

- daemon-first rebuild
- custom universal agent runtime
- large internal memory platform
- broad always-on multi-model divergence
- provider-zoo UX
- research or council growth without a clear bounded reliability payoff

## Selection rule for new work

Pick a bounded slice from this roadmap only if it clearly improves one of:

- boundedness
- reliability
- inspectability
- operator simplicity

Otherwise:
- downgrade it
- defer it
- or cut it
