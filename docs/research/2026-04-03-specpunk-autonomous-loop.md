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

## Anti-goals

- do not hide failed verification behind automatic retries
- do not mark blocked work as success because a staged recovery contract exists
- do not couple publish strategy to proof correctness

## Recommended next slices

1. durable `blocked` / `recovery` record in ledger
2. explicit replan object or `next_contract` linkage
3. optional publish policy after accepted proof

## Acceptance evidence

This track is done when:
- autonomous runs can fail, recover, and continue without shell-only glue
- operators can inspect where autonomy stopped and what the next durable action is
- `go` behaves like a product shell over a reliable state machine
