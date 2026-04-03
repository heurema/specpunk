# Specpunk Work Ledger

Date: 2026-04-03
Status: active research track
Priority: P1

## Research question

What is the canonical durable work object in `specpunk`, and how should `Goal`, `Feature`, `Contract`, `Run`, `Decision`, and `Proof` compose into one queryable ledger?

## Why this matters

Today work state is spread across multiple objects and files. That is enough for local slices, but not yet enough for long-horizon reliability. The comparison with `beads` makes the gap obvious: durable work continuity is a product feature, not just a storage detail.

## Current problem

We have:
- `Feature`
- `Contract`
- `Run`
- `Receipt`
- `DecisionObject`
- `Proofpack`
- append-only events

But we do not yet have one obvious answer to:

> What should an operator or agent query to understand the current state of work?

## Working hypothesis

A first-class `WorkItem` or equivalent ledger entry should exist above the current chain and link:
- project
- goal text / normalized intent
- current contract ref
- run history
- latest decision
- latest proof
- blocked state
- next recommended action

This does **not** require a new storage engine first. The model should be fixed before storage migration is discussed.

## Proposed shape: `WorkLedgerView`

The next bounded design step should standardize one canonical projection:

```text
WorkLedgerView
  project_id
  work_id
  goal_ref
  feature_ref
  active_contract_ref
  latest_run_ref
  latest_receipt_ref
  latest_decision_ref
  latest_proof_ref
  lifecycle_state
  blocked_reason
  next_action
  next_action_ref
  updated_at
```

### Required semantics

- `work_id` must be stable across contract versions and retries
- `active_contract_ref` must point to the contract currently expected to drive execution
- `latest_*` refs must reflect the newest durable artifacts, not shell guesses
- `lifecycle_state` must answer the operator question:
  - what state is this work item in right now?
- `next_action` must answer:
  - what should happen next if I continue this work item?

## Proposed `lifecycle_state`

The first version does not need a huge state machine. It needs a useful one.

Recommended v1 states:

- `drafting`
- `awaiting_approval`
- `ready_to_run`
- `running`
- `awaiting_gate`
- `accepted`
- `blocked`
- `escalated`
- `superseded`
- `cancelled`

These states are not replacements for lower-level object statuses. They are the operator-facing projection over them.

## Source-of-truth rule

`WorkLedgerView` is a **projection**, not a new mutable truth object.

That means:

- append-only events remain canonical truth
- contracts, runs, decisions, and proofs remain canonical artifacts
- `WorkLedgerView` is materialized from them
- shell output must prefer `WorkLedgerView` over ad hoc inference

## Query surfaces that should use it

Once introduced, these commands or surfaces should read from it first:

- `punk status`
- `punk inspect work <id>`
- blocked/recovery summaries in `punk go`
- future morning/briefing surfaces

## Anti-goals

- do not invent a second write path for work state
- do not put review prose into the ledger view
- do not choose a storage engine before the view shape is accepted
- do not collapse `Feature` and `WorkLedgerView` into one object without proving they are the same concept

## Evidence from reference systems

- **Beads** treats work graph and continuity as a primary operational plane.
- **Gas City** says work is the primitive, not orchestration.
- Our own dogfood bugs show that when runtime state is split across multiple surfaces, reliability work becomes reactive and expensive.

## Non-goals

- do not choose `Dolt` yet
- do not redesign every existing event schema in one shot
- do not replace append-only events before the object model is explicit

## Current implementation status

The first bounded implementation slice now exists:

- `punk inspect work`
- `punk inspect work <id>`
- `punk inspect work <id> --json`

The current v1 derives `WorkLedgerView` from existing repo-local artifacts:

- `Feature`
- `Contract`
- `Run`
- `Receipt`
- `DecisionObject`
- `Proofpack`
- `AutonomyRecord`

without introducing a new persistence layer yet.

`punk status` now prefers this derived view for current work continuity fields (`work_id`, `lifecycle_state`, `next_action`, and latest contract/run/decision refs) instead of reconstructing them ad hoc from raw events.

`punk go` now writes a durable autonomy-linked record so blocked or escalated outcomes with staged recovery prepared become inspectable later through `punk inspect work` / `punk status`.

The next shell-oriented refinement is recovery-aware summaries that expose:

- `autonomy_outcome`
- `recovery_contract_ref`
- one obvious `suggested_command`

## Recommended next slices

1. Materialize one durable view record per active work item from the existing event stream
2. Backfill stronger latest contract/run/decision/proof linkage into one materialized view
3. Add dedicated `inspect work <id>` recovery-oriented summaries for blocked/escalated autonomy
4. Decide whether a materialized on-disk work-ledger record is still needed beyond the current derived view

## Acceptance evidence

This track is done when:
- one canonical work view exists
- status and inspect can answer current/blocked/next questions from it
- contributor agents no longer need to infer work continuity from multiple file types
- blocked/autonomous recovery state is inspectable without reading shell logs
