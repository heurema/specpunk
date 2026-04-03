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

## Evidence from reference systems

- **Beads** treats work graph and continuity as a primary operational plane.
- **Gas City** says work is the primitive, not orchestration.
- Our own dogfood bugs show that when runtime state is split across multiple surfaces, reliability work becomes reactive and expensive.

## Non-goals

- do not choose `Dolt` yet
- do not redesign every existing event schema in one shot
- do not replace append-only events before the object model is explicit

## Recommended next slices

1. Define a `WorkLedgerView` projection over existing objects
2. Add `punk inspect work <id>` style query surface
3. Backfill latest contract/run/decision/proof linkage into one materialized view

## Acceptance evidence

This track is done when:
- one canonical work view exists
- status and inspect can answer current/blocked/next questions from it
- contributor agents no longer need to infer work continuity from multiple file types
