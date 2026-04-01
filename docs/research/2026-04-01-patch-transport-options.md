# Research: patch transport options for reference-aligned execution lanes

Date: 2026-04-01
Status: exploratory

## Scope

Question:

> What is the strongest next step after introducing `exec / patch-apply / manual` execution lanes in `specpunk`?

Constraint:

- local reference repos are **read-only guidance only**
- nothing from `contrib/openai/codex` or `contrib/cc/*` may be executed without explicit user approval

## Current evidence from `specpunk`

### Confirmed local facts

1. Existing-file glue slices repeatedly fail in the current self-hosting loop.
2. Prompt-only retry seeds did not solve the problem.
3. A distinct `patch/apply lane` is architecturally correct, but the current transport is still weak.

### Concrete observed runs

- `run_20260401154641218`
  - scope: `punk/punk-orch/src/ratchet.rs`, `punk/punk-run/src/main.rs`
  - result: `no implementation progress after bounded context dispatch`
  - meaning: free-form repo-editing lane reached the right symbols but did not start the first diff

- `run_20260401162204780`
  - first `patch/apply lane` validation
  - result: 30s stall on prompt/instruction tail
  - meaning: lane separation alone was not enough; the prompt envelope was too heavy

- `run_20260401163410956`
  - compact patch context + lane-specific timeout
  - result: `codex command timed out after 90s`
  - stdout/stderr stayed empty
  - meaning: the remaining blocker is not only prompt size; the patch-generation transport itself is unreliable

## Current `specpunk` architecture fact

Today in `crates/punk-adapters`:

- `exec lane` uses `codex exec --full-auto --ephemeral`
- `manual lane` short-circuits known blocked reliability/control-plane slices
- `patch/apply lane` now exists, but still asks `codex exec` to produce a patch via:
  - `read-only`
  - `output-schema`
  - controller-side patch validation + `git apply`

So the main remaining gap is:

> patch generation still depends on the same `codex exec` transport family that already behaves poorly for this slice class

## Reference findings

### `contrib/openai/codex/codex-rs`

Most relevant signals:

- `exec/` is a separate runtime path
- `apply-patch/` is a separate parser + apply surface
- `execpolicy/` is a separate routing/policy layer

This strongly supports:

- separate execution lanes
- controller-owned mutation boundaries
- explicit routing instead of retry heuristics

`apply-patch` also uses a **plain patch text format** with local parsing/application, not a “model edits repo directly” path.

### `contrib/cc/*`

Useful mainly for:

- broader command/service/tool separation
- coordinator/product structure

Less useful than Codex refs for this specific transport problem.

## Option comparison

### Option A — JSON patch via `codex exec --output-schema`

Shape:

- model runs in read-only mode
- returns JSON like `{ summary, patch, blocked_reason }`
- controller parses and applies patch

Pros:

- structured output
- explicit blocked path
- controller owns mutation

Cons:

- still depends on `codex exec` transport for generation
- `output-schema` adds another layer of model conformance pressure
- observed failures already show stall/timeout in this exact envelope

Assessment:

- architecture fit: medium
- implementation cost: already paid
- reliability outlook: low-to-medium

Confidence:

- ~0.80 that this is **not** the best final transport

### Option B — plain patch text lane

Shape:

- model returns **only patch text**
- no JSON schema
- controller validates patch syntax, scope, and applicability
- controller applies patch
- blocked path is a sentinel line or empty patch + explicit failure rule

Pros:

- much closer to `codex-rs/apply-patch`
- lower output-shape friction than JSON
- cleaner separation between generation and application
- simpler local validation story

Cons:

- weaker machine structure than JSON
- requires patch parser or strict local validation
- blocked/failure signaling needs clear conventions

Assessment:

- architecture fit: high
- implementation cost: medium
- reliability outlook: medium-to-high

Confidence:

- ~0.65 that this is the best next MVP

### Option C — controller first-hunk engine

Shape:

- controller computes and applies one tiny mechanical first diff
- model continues from a non-blank changed state

Pros:

- strongest way to break the “first edit never starts” pattern
- highly deterministic once insertion logic is reliable

Cons:

- controller effectively becomes a mini editor/transformation engine
- more local complexity
- much easier to overfit to one slice class

Assessment:

- architecture fit: medium
- implementation cost: high
- reliability outlook: potentially high long-term, but not the best next bounded step

Confidence:

- ~0.75 that this is better as a later step, not the next MVP

### Option D — alternate model transport

Shape:

- keep patch/apply lane concept
- replace `codex exec` generation with another model/API transport

Pros:

- may bypass current CLI ceiling quickly

Cons:

- weakens dogfooding
- risks adding a product-specific sidecar path
- moves away from reference-aligned core architecture

Assessment:

- architecture fit: low
- implementation cost: medium
- reliability outlook: unknown

Confidence:

- ~0.85 that this should remain fallback only

## Recommendation

### Best next step

Implement:

> **plain patch text lane**

instead of continuing with:

> JSON patch via `codex exec --output-schema`

### Why

This is the best balance of:

- reference alignment
- bounded implementation size
- controller-owned mutation
- lower output-shape friction
- continued dogfooding

### Recommended design

For eligible existing-file glue slices:

1. controller routes to `patch/apply lane`
2. model is asked to return **patch text only**
3. controller validates:
   - patch is non-empty
   - only allowed scope paths are touched
   - patch updates existing files only
   - patch applies cleanly
4. controller applies patch
5. controller runs checks

### Blocked path

If model cannot produce a valid patch:

- emit explicit blocked sentinel
- or fail with `patch/apply lane returned no valid patch`

but do not silently fall back to free-form repo editing for the same slice

## Not recommended next

Do **not** spend the next slice on:

- another retry seed
- another timeout tweak
- another classifier
- another JSON patch prompt rewrite

Those are now inferior to a transport change.

## Decision summary

### What is highly likely correct

- execution lanes are the right architecture
- controller-owned mutation is the right direction
- retry/prompt tuning is no longer the main lever

### What still needs proof

- whether plain patch text lane is sufficient on its own
- or whether later we still need a first-hunk controller bootstrap

## Proposed next implementation slice

Bounded target:

- `crates/punk-adapters/src/lib.rs`
- maybe `crates/punk-adapters/src/context_pack.rs`

Goal:

> replace JSON patch generation in `patch/apply lane` with plain patch text generation and local patch validation/apply
