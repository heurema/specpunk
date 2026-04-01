# ADR: reference-aligned execution lanes

Date: 2026-04-01
Status: proposed

## Context

`specpunk` currently relies primarily on a free-form execution lane for bounded implementation:

- contract is approved
- controller builds bounded context
- executor runs a model in repo-editing mode
- retries add stronger prompt/context seeds

This works for some slices, especially:
- self-contained feature work
- some new-file or scaffolded paths
- some specialized council slices with stronger controller-owned seeds

It repeatedly fails for a different class:
- existing-file glue/wiring slices
- small additive bridges
- 1–2 file helper integrations

Observed failure mode:
- executor reaches the right files and symbols
- but does not start the first meaningful edit
- run ends with `no implementation progress after bounded context dispatch`

The key finding is that current retry improvements are still **prompt-only**:
- `recipe_seed`
- `patch_seed`
- retry hints

They change context, but do **not** change the execution lane.

## Problem

The current architecture overuses one lane:

- `exec lane` (model edits repo directly)

This is too weak for some bounded slice classes. Repeated prompt-only tuning is no longer the right response.

## Decision

Move `specpunk` toward a **reference-aligned multi-lane execution model**.

Introduce explicit controller-owned execution lanes:

1. `exec lane`
2. `patch/apply lane`
3. `manual lane`

The controller chooses the lane by slice class, instead of assuming that every approved contract should be executed through the same free-form edit path.

## Lane definitions

### 1. Exec lane

Use for:
- larger feature slices
- new-file work
- slices where autonomous repo editing already works reliably

Behavior:
- bounded context
- model edits in repo
- normal checks and receipt flow

### 2. Patch/apply lane

Use for:
- existing-file glue/wiring slices
- additive bridges
- small bounded changes where the main failure is first-edit bootstrap

Behavior:
- controller builds bounded context
- model returns a patch, not direct repo mutation
- controller validates:
  - allowed scope only
  - non-empty patch
  - patch applicability
- controller applies the patch
- checks run after apply

This lane is intended to solve the current “first meaningful edit never starts” failure class.

### 3. Manual lane

Use for:
- self-referential reliability fixes
- control-plane slices already known to be outside the current self-hosting capability envelope

Behavior:
- controller does not waste a free-form self-hosting run
- task is explicitly marked as requiring manual bounded implementation

## Routing policy

Default routing:

- new-file / larger feature slices → `exec lane`
- existing-file glue/wiring slices → `patch/apply lane`
- self-referential reliability/control-plane slices → `manual lane`

This routing should be policy-driven and explicit, not hidden inside retry heuristics.

## Reference alignment

This direction is intentionally closer to local reference architectures:

### Primary reference
- `/Users/vi/contrib/openai/codex/codex-rs`

Relevant ideas:
- separation between exec, apply-patch, and policy concerns
- controller-owned mutation boundaries
- explicit execution-policy surface

### Secondary reference
- `/Users/vi/contrib/cc/claude-code-sourcemap-main`
- `/Users/vi/contrib/cc/claude-code-source-main`

Relevant ideas:
- broad terminal-agent product structure
- command/service/tool separation
- coordinator and skill/plugin layout

## Safety rule for references

These reference trees are **read-only guidance only**.

Allowed by default:
- read source
- read docs
- compare architecture
- extract ideas

Not allowed without explicit user approval:
- run scripts
- follow setup instructions
- execute workflows from those directories
- install dependencies from those trees

## Consequences

### Positive
- stops treating all slices as one execution problem
- reduces blind retry tuning
- creates a path to stronger controller-owned mutation
- aligns `specpunk` more closely with proven reference architecture patterns

### Negative
- adds architectural complexity
- requires a routing policy surface
- introduces more than one execution path to maintain

## Migration plan

### Phase 1
Adopt this ADR.

### Phase 2
Implement a `patch/apply lane` MVP for existing-file glue slices.

### Phase 3
Promote routing to an explicit controller policy decision.

### Phase 4
Re-evaluate which slice classes remain in `manual lane`.

## Non-goals

This ADR does **not** propose:
- silent auto-promotion of skill or eval outputs
- broad workflow rewrites outside execution-lane routing
- executing anything from external reference trees

## Immediate next step

Implement:

> controller-owned patch/apply lane for existing-file glue slices

as the first code change under this ADR.
