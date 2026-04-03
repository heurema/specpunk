# Specpunk Autonomous Loop

Date: 2026-04-03
Status: active research track
Priority: P2

## Research question

What remains between the current `go --fallback-staged` flow and a reliable goal-to-result autonomous loop?

## Why this matters

`specpunk` already has the skeleton:
- goal-only intake
- draft
- approve
- cut
- gate
- proof
- outcome metadata
- staged fallback prep

That is a major step. But it is not yet the same as a stable autonomous work loop.

## Missing pieces

1. stronger replan loop after `block` / `escalate`
2. richer final result packaging for user-level consumption
3. durable blocked state in ledger, not only shell output
4. publish/PR strategy as optional post-proof action
5. stronger guarantees around retries and side-effect isolation

## Working hypothesis

Autonomy should be implemented as a sequence of explicit state transitions, not as one giant shell convenience layer.

That suggests:
- keep `go` as shell UX
- strengthen the underlying state machine and recovery objects
- promote recovery from text hint to durable workflow state

## Proposed autonomous loop contract

The autonomous loop should be modeled as:

```text
Goal
-> active Contract
-> Run
-> DecisionObject
-> Proofpack
-> autonomous outcome record
-> next durable action
```

The key change is that blocked or escalated autonomy should not live only in shell output.

## Proposed durable autonomous outcome

The first useful durable outcome shape should include:

```text
AutonomyRecord
  work_id
  goal_ref
  contract_ref
  run_ref
  decision_ref
  proof_ref
  autonomy_outcome
  basis_summary
  recovery_contract_ref
  next_action
  next_action_ref
  recorded_at
```

### `autonomy_outcome`

Recommended v1 values:

- `succeeded`
- `blocked`
- `escalated`

This should map cleanly to shell summaries and exit semantics, but should survive beyond one shell invocation.

## Required durable recovery behavior

When `go --fallback-staged` blocks, the system should durably record:

- what contract was attempted
- which run/decision/proof became authoritative
- whether a recovery contract was prepared
- what the next recommended durable action is

The shell may summarize this, but should not be the only place where it exists.

## Relationship to `WorkLedgerView`

`AutonomyRecord` does not need to be a separate mutable truth plane.

The most likely good design is:

- autonomous outcomes are written as canonical artifacts or event-linked records
- `WorkLedgerView` projects them into:
  - `lifecycle_state`
  - `blocked_reason`
  - `next_action`
  - `next_action_ref`

That keeps shell UX and durable state aligned.

## Proposed v1 state progression

The first useful autonomous progression can stay compact:

1. `drafting`
2. `ready_to_run`
3. `running`
4. `awaiting_gate`
5. `accepted` or `blocked` or `escalated`
6. optional `recovery_prepared`
7. later `superseded` when a newer contract replaces it

`recovery_prepared` is especially important because it distinguishes:

- blocked with no durable next step
- blocked with a staged recovery path already prepared

## Required shell alignment

The shell summary should be a view over durable state, not a parallel truth:

- shell summary fields should come from durable refs where possible
- recovery hints should be traceable to a durable `next_action_ref`
- blocked autonomy should be inspectable later without reading historical shell logs

## Non-goals for v1

- do not add automatic retry loops that hide failure
- do not auto-promote recovery contracts to success
- do not couple publish/PR policy to proof correctness
- do not require a new storage engine before proving the state model

## Anti-goals

- do not hide failed verification behind automatic retries
- do not mark blocked work as success because a staged recovery contract exists
- do not couple publish strategy to proof correctness

## Recommended next slices

1. durable `blocked` / `recovery` record linked to the work ledger
2. explicit `next_contract` or equivalent `next_action_ref` linkage
3. `status` / `inspect work` projection over autonomous outcomes
4. optional publish policy after accepted proof

## Acceptance evidence

This track is done when:
- autonomous runs can fail, recover, and continue without shell-only glue
- operators can inspect where autonomy stopped and what the next durable action is
- `go` behaves like a product shell over a reliable state machine
- blocked vs blocked-with-recovery-prepared are distinguishable in durable state
