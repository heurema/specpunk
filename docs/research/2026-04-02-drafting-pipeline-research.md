# Drafting pipeline research: why scope keeps drifting and what to change next

Date: 2026-04-02
Project: specpunk
Mode: read-only research + local code inspection
External references: read-only only; nothing under `/Users/vi/contrib/*` was executed

## Goal

Understand why `punk plot contract` still produces poor live scope for bounded file-level slices even after multiple local drafting heuristics, and identify the next architectural fix that is more reliable than adding more scoring tweaks.

## Problem statement

The target example is a bounded nested product slice:

> Add a skill eval summary line to nested punk status output. Extend status snapshot additively to include recent skill eval window data derived from stored skill eval summaries, and print a concise human-readable line in punk-run status similar to existing eval or benchmark window reporting. Reuse existing eval summary helpers. Do not add new storage, do not change CLI arguments or schemas, and do not broadly rewrite status output.

Desired scope should be narrow and file-level, roughly around:

- `punk/punk-run/src/main.rs`
- `punk/punk-orch/src/eval.rs`
- maybe one additional concrete implementation file if strictly needed

Observed live drafts repeatedly went wrong:

1. mixed root and nested workspaces
2. picked thin facades like `punk/punk-orch/src/lib.rs`
3. over-weighted `skill.rs` because of path token overlap
4. escalated to directory scope like `punk/punk-run/src` and `punk/punk-orch/src`
5. after trying to force directory invalidity, fallback expanded to many unrelated files and checks

This means the current drafting path is unstable on realistic bounded file-level tasks.

## Current pipeline

### Scan stage (`crates/punk-core/src/lib.rs`)

`scan_repo(...)` builds:

- `candidate_entry_points`
- `candidate_scope_paths`
- `candidate_target_checks`
- `candidate_integrity_checks`

Important detail: `collect_scope_candidates(...)` calls `walk_repo(...)`, and `walk_repo(...)` adds:

- matching files
- **their parent directories** with slightly reduced score

So `candidate_scope_paths` is not actually “file scope paths”; it is a mixed bag of:

- files
- directories

That mixed representation is the first structural smell.

### Draft stage (`crates/punk-adapters/src/lib.rs`)

`build_draft_prompt(...)` explicitly tells the model:

- prefer `allowed_scope` entries from `candidate_scope_paths`
- prefer `entry_points` from `candidate_entry_points`

Because `candidate_scope_paths` mixes files and directories, the model sees directories as first-class valid `allowed_scope` candidates.

### Canonicalize + validate + fallback (`crates/punk-core/src/lib.rs`)

The controller then runs:

1. `canonicalize_draft_proposal(...)`
2. `validate_draft_proposal(...)`
3. `build_bounded_fallback_proposal(...)` if the proposal is structurally invalid

The problem is that many bad drafts are **not structurally invalid enough** to trigger the fallback in the right way:

- directory `allowed_scope` can still cover file entry points
- broad file lists are still formally valid
- explicit prompt details are preserved
- fallback, when forced, can become too expansive because it reuses broad candidate sets and broad check sets

## Evidence from recent live drafts

### Baseline bad scope

`ct_20260402051649339_v1`

Scope contained:

- `punk/punk-orch/src/lib.rs`
- `punk/punk-run/src/main.rs`

This was bad because nested `punk/punk-orch/src/lib.rs` is only a thin facade (`pub mod ...`) and does not expose the real implementation surface.

### After thin-facade and workspace-coherence fixes

`ct_20260402065633971_v1`

Scope became workspace-coherent, but still included:

- `punk/punk-orch/src/skill.rs`
- `punk/punk-run/src/main.rs`

This was better than mixing `crates/*` with `punk/*`, but still wrong for helper reuse.

### After helper-aware scoring

`ct_20260402071220220_v1`

Scope improved further:

- `punk/punk-orch/src/skill.rs`
- `punk/punk-orch/src/eval.rs`
- `punk/punk-run/src/main.rs`

This was materially better, but still over-included `skill.rs`.

### After trying to treat directory scope as structural invalidity

`ct_20260402073828391_v1`

Draft regressed badly:

- `punk/punk-run/src/main.rs`
- `punk/punk-orch/src/skill.rs`
- `punk/punk-orch/src/research.rs`
- `punk/punk-orch/src/eval.rs`
- `punk/punk-orch/src/ratchet.rs`
- `punk/punk-orch/src/daemon.rs`
- `punk/punk-orch/src/session.rs`
- `punk/punk-orch/src/lib.rs`

and target checks blew up to a near-workspace-wide set.

This shows the fallback path is too permissive once it activates.

## Root causes

### 1. Mixed candidate representation

`candidate_scope_paths` currently conflates two different things:

- candidate file scope
- candidate directory scope

That leaks ambiguity into the model prompt and then into canonicalization.

### 2. Model is asked to choose scope classes the controller should own

The prompt asks the drafter to pick `allowed_scope` from mixed candidates. This is too much responsibility for the model.

The controller should own at least these decisions for bounded low-risk slices:

- file-level vs directory-level scope class
- workspace family coherence
- whether broad scope is acceptable at all

### 3. Validation is too syntactic

Current validity largely checks:

- paths are repo-relative
- entry points are covered by allowed scope
- checks are well-formed

But it does **not** robustly encode:

- bounded file-level tasks should prefer file scope when concrete file candidates exist
- broad directory scope is suspicious when the scan already found concrete files
- giant check sets are suspicious for additive glue slices

### 4. Fallback is not narrow-by-construction

Once fallback kicks in, it still builds from broad scan candidates and broad target checks.

That means the system can convert one kind of bad draft into another kind of bad draft.

### 5. Heuristic scoring is now doing too much work

Thin-facade penalties, workspace-family penalties, helper-aware symbol scoring — these are useful, but they are trying to compensate for the absence of a stronger drafting policy model.

## Reference-aligned interpretation

Read-only inspection of `contrib/openai/codex/codex-rs` reinforces one design direction:

- separate policy from execution
- keep controller-owned validation explicit
- avoid overloading one fuzzy stage with multiple responsibilities

For our drafting pipeline, the analogous principle is:

- scan should gather candidates
- policy should choose what classes of scope/checks are legal
- model should draft within that narrowed envelope
- fallback should be deterministic and narrow, not a second fuzzy search

This is closer to the reference architecture than piling on more token scoring.

## Recommended architecture

### Recommendation: split file candidates from directory candidates

The strongest next step is **not another scoring tweak**.

It is to make scan output more structured:

- `candidate_file_scope_paths`
- `candidate_directory_scope_paths`
- keep `candidate_entry_points` separate

Then drafting policy can say:

- for low-risk file-level slices, only file scope is legal unless the user explicitly asked for directory scope
- directory scope is allowed only for explicitly broad tasks

This is the most important architectural change.

### Recommendation: introduce a bounded-slice drafting policy gate

Before the model drafts the proposal, classify the request into a small set of drafting modes, for example:

1. `file_glue`
2. `new_file_scaffold`
3. `directory_refactor`
4. `artifact_protocol`

For `file_glue`:

- only file paths in `allowed_scope`
- max 1–3 scope files by default
- target checks narrowed to matching packages only
- fallback cannot emit directories

This is much stronger than hoping prompt wording nudges the model correctly.

### Recommendation: make fallback deterministic, not expansive

If a `file_glue` draft is invalid, fallback should build from a very narrow controller-owned source:

- top file candidates only
- exact matching entry points first
- helper reuse files next
- no directory expansion
- no broad “supporting source paths” unless explicitly justified by the task class

In other words, fallback should be a deterministic narrowing pass, not a second broad synthesis pass.

### Recommendation: narrow checks by task class

Current fallback can explode target checks. For bounded file-level additive slices, checks should be constrained by policy:

- prefer package-matching checks
- avoid unrelated package tests when the scope does not touch them
- keep integrity as workspace-wide if needed, but target checks should remain narrow

### Recommendation: keep helper-aware scoring, but stop expecting it to solve policy problems

Useful heuristics to keep:

- thin facade penalty
- workspace coherence
- helper/symbol-aware scoring

But these should become **supporting ranking signals**, not the primary safety mechanism.

## Recommended phased plan

### Phase 1 — design correction

Do not commit the current `crates/punk-core/src/lib.rs` diff.

Rollback current drafting experiments and implement a cleaner design based on:

- split file vs directory candidate lists
- bounded drafting mode classification
- deterministic narrow fallback for `file_glue`

### Phase 2 — minimal implementation

A bounded implementation MVP could be:

1. extend `RepoScanSummary` with separate file vs directory scope candidates
2. update draft prompts to prefer file candidates for file-level work
3. update structural invalidity so a `file_glue` draft with directory scope is invalid
4. replace expansive fallback with a top-N concrete file fallback for that class

### Phase 3 — validation

Re-run the same problematic prompt and expect something close to:

- `punk/punk-run/src/main.rs`
- `punk/punk-orch/src/eval.rs`

and at most one more tightly justified file

### Phase 4 — only then resume self-hosted validation

Once the drafting layer stops producing bad scope, patch/apply lane validation becomes meaningful again.

## Confidence

### High confidence

- The current problem is no longer primarily in patch lane execution.
- The drafting pipeline mixes candidate classes too early.
- Broad directory scope should not be left as a model choice for bounded file-level slices.

### Medium confidence

- Splitting candidate files vs candidate directories is the best next implementation step.
- A small drafting-mode classifier (`file_glue`, `directory_refactor`, etc.) is likely worth the added complexity.

### Low confidence

- Any further local scoring tweak alone will produce stable live behavior.

## Recommendation

Do **not** commit the current local `crates/punk-core/src/lib.rs` drafting diff.

Instead:

1. rollback the current drafting experiment
2. implement a cleaner policy-oriented fix:
   - separate file and directory candidate outputs
   - narrow drafting mode for bounded file-level slices
   - deterministic concrete-file fallback

That is the most reliable next step and the one most aligned with the reference idea of controller-owned policy rather than prompt-driven heuristics.
