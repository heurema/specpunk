# Product Brief

Last updated: 2026-03-12
Owner: Vitaly
Status: **archived** (superseded 2026-03-27)

> **This document describes the original review-boundary product direction.**
> The product has pivoted to an agent orchestration platform.
> Current SSoT: [VISION.md](VISION.md), [ARCHITECTURE.md](ARCHITECTURE.md), [ROADMAP-v2.md](ROADMAP-v2.md)

## SSoT Rule

This file is the product source of truth.

If `current-cycle.md` conflicts with this file, `brief.md` wins.
Changes to this file require a corresponding entry in `decisions.md`.

## One Sentence

Specpunk is a repo-native review boundary for AI-assisted brownfield change.

## What We Are Building

We are building a compact layer that makes AI-generated code changes easier to understand, constrain, and review.

The product does not compete with Claude Code, Codex, Cursor, or other coding agents.
It sits around their output and turns a noisy change into a bounded review object.

## The Problem

AI can generate code faster than teams can safely understand and review it.

The real problem is not "how to get more code written."
The real problem is:

- the original intent disappears between prompt, session, and diff
- agents touch more files than they should
- reviewers see changed code, but not a clear boundary
- tests may pass without proving that the intended behavior stayed intact

## Who This Is For

Primary user:
- engineers and reviewers working in existing codebases with AI-assisted changes

Primary team context:
- brownfield repositories
- teams already using AI coding tools
- teams where review trust is worse than generation speed

Likely buyer later:
- engineering manager
- platform / DevEx
- VP Engineering

## Current Wedge

The first wedge is:

`scope enforcement + minimal review artifact`

That means:

- declare what files a task should touch
- compare the declared boundary with the actual change
- produce a minimal review note that explains what happened

The first useful output is not a full platform.
The first useful output is a change that is easier to reason about than a raw diff.

## Product Shape

The durable product shape is a compact artifact set around a change:

- `intent`
- `scope`
- `glossary`
- `invariants`
- `evidence`
- `review posture`

These artifacts must stay shorter and denser than the code they help review.
If they become long, decorative, or generic, the product is failing.

## Principles

- Keep the portable core repo-native.
- Prefer explicit boundaries over clever inference.
- Prefer review clarity over generation speed.
- Add automation only when it reduces manual overhead without hiding meaning.
- Do not create markdown for its own sake.
- Dogfood the product on this repo before expanding the scope.

## In Scope

- review boundary for AI-assisted changes
- artifact-backed review
- scope enforcement
- contradiction and terminology checks
- behavior-oriented evidence
- brownfield workflows

## Out of Scope

- building a new IDE
- replacing coding agents
- "natural language programming" as the main thesis
- full compiler or spec-as-source system
- formal verification as a v1 requirement

## Success Signal

Specpunk succeeds when one real repository task becomes measurably easier to review with Specpunk artifacts than with a raw diff alone.

## Failure Signal

Specpunk fails if:

- artifact prep takes longer than the task itself
- the artifact set is longer than the code it explains
- reviewers still cannot explain what changed and what stayed bounded
- the product depends on one vendor transcript format to work at all
