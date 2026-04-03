# Specpunk Project Intelligence

Date: 2026-04-03
Status: active research track
Priority: P2

## Research question

How should `specpunk` evolve from bootstrap files and project-scoped skills into a coherent project-intelligence layer that survives across sessions and repo classes?

## Why this matters

Current progress is real:
- project bootstrap
- project-aware status
- project-scoped skill triggers
- repo-local `AGENTS.md`
- repo-local `.punk/bootstrap/*`

But these pieces still behave more like adjacent utilities than one coherent project-intelligence system.

## Current components

- inferred project id
- project resolver and path mapping
- bootstrap file
- generated agent instructions
- project-scoped skills
- status scoping

## Gaps

- no explicit project overlay packet
- no canonical project capability summary
- no durable project memory packet inside runtime state
- no clear line between repo-local instructions and runtime project policy

## Working hypothesis

A future `ProjectOverlay` or equivalent should bundle:
- project identity
- repo capabilities
- bootstrap rules
- project-scoped skills
- local constraints
- safe default checks

This should remain repo-local and inspectable.

## Recommended next slices

1. define a `ProjectOverlay` packet shape
2. add `punk inspect project` or equivalent
3. unify bootstrap file, generated instructions, and project-scoped skill metadata under one inspectable model

## Acceptance evidence

This track is done when:
- project bootstrap no longer feels like a collection of special cases
- agents can inspect one project packet instead of piecing together conventions from multiple files
- project-specific behavior is explicit and testable
