# Project-Scoped Autonomous Development with `punk`

**Date:** 2026-03-31  
**Source:** user ideation during `specpunk` session on self-hosting and project autonomy

## Idea

`punk` can evolve from a contract-first executor into a system that advances a concrete project in small bounded steps using the same loop currently used to develop `punk` itself:

`goal -> contract -> cut -> gate -> learn -> next step`

But for this to work on arbitrary projects, the `punk` instance working on that project must stay deeply inside the project's context.

## Core thesis

The hard problem in autonomous development is not only execution.

The real problem is that an agent usually:

- does not hold long-lived project context reliably
- does not know local rules and conventions
- forgets previous failures and process lessons
- does not distinguish local priorities from generic coding work
- drifts or keeps repairing the wrong layer

So the next stage for `punk` is not just "do tasks automatically", but:

> give `punk` durable project-scoped agency

## What “project-scoped” means

A `punk` instance working on a project should know:

- what the project is and what it is for
- current goals and milestones
- local engineering rules
- architectural boundaries and conventions
- recent successful and failed runs
- accumulated lessons and process fixes
- project-specific skills and workflows
- active blockers and recent regressions

This means it should behave not like a generic coding agent, but like a bounded operator of one concrete system.

## Desired operating loop

The intended loop is:

1. `punk` understands current project state
2. it selects the next small safe step
3. it turns that into a bounded contract
4. it executes
5. it validates through gate
6. it records lessons
7. it updates project context
8. it selects the next step

That is more than a task runner. It is a continuous project-development loop.

## Required capabilities

### 1. Project context pack

`punk` needs a stable project context pack that includes:

- architecture
- active goals
- local rules
- recent receipts and decisions
- relevant lessons
- current health

### 2. Project memory

`punk` needs project-scoped memory for:

- durable facts
- architecture decisions
- bugfix lessons
- process failures
- known constraints

### 3. Project-local skills and rules

Projects may need their own:

- workflows
- coding rules
- deployment rules
- verification patterns
- trust boundaries

### 4. Self-repair process

If `punk` repeatedly fails the same way, it should:

- detect the pattern
- propose a bounded process fix
- apply it
- continue

instead of repeating the same failure loop.

### 5. Autonomy boundaries

`punk` needs explicit rules for:

- what it may do autonomously
- what requires human approval
- what counts as trust-sensitive

## Main design challenge

The hardest part is not automatically writing code.

The hardest part is maintaining a reliable model of project reality:

- what is already done
- what is currently in flight
- what is broken
- which rules are mandatory
- where fast iteration is safe
- where human checkpoints are required

Without that, autonomy turns into drift.

## Strategic direction

### Phase 1

Make `punk` reliable at self-hosting:

- contracts
- gating
- bounded execution
- self-repair loop

### Phase 2

Add project-scoped context:

- context pack
- project memory
- local skills and rules
- run triage
- active state awareness

### Phase 3

Add the autonomous project loop:

- milestone decomposition
- next-step selection
- recurring-failure learning
- bounded continuous delivery

## Guiding principle

`punk` should not "magically do the project".

It should:

- understand the project
- compress the next move into a bounded contract
- verify the result
- update project knowledge
- continue

## One-sentence version

> `punk` should become a project-scoped autonomous development system: a system that evolves a concrete project through small verifiable steps while staying inside that project's memory, rules, context, and trust boundaries.
