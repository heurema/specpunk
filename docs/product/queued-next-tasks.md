# Queued Next Tasks

Last updated: 2026-04-11
Owner: Vitaly
Status: historical queue with current shortlist

> **Important:** this file contains an older paused queue from the pre-alignment phase.
> Use it only as historical context.
>
> Current strategic truth lives in:
>
> - `docs/product/NORTH-ROADMAP.md`
> - `docs/product/ADR-provider-alignment.md`
> - `docs/product/ARCHITECTURE.md`
> - `docs/product/CURRENT-ROADMAP.md`

## Current shortlist (2026-04-11)

### Ship now

These are current-forward and should be preferred when choosing bounded slices:

1. reliability fixes for bounded execution, rollback, and proof integrity
2. repo fixture matrix expansion for known dogfood repo classes
3. one-face shell improvements that simplify `go/start/gate/status`
4. typed evidence / harness improvements that strengthen `Receipt` and `Proofpack`
5. structured repo anchors and source-first targeting that reduce free-text contract drift
6. provider adapter and wrapping improvements that simplify, rather than expand, the kernel

### Later

These remain valid only in bounded, derived form:

1. autonomous loop improvements that stay inside existing primitives
2. project overlays / project intelligence that remain small and structured
3. selective council improvements for high-risk work only
4. bounded research tooling that improves decisions without becoming product core

### Cut or avoid

Do not treat these as default-forward:

1. daemon-first rebuilds
2. broad multi-model divergence as routine behavior
3. large internal memory or self-improvement subsystems
4. provider-zoo or control-plane UX
5. free-text-heavy orchestration layers that do not improve boundedness or reliability

## Queue Rule

This is the ordered return queue, not an active sprint.
Do not start lower items before resolving the higher gating items.

## Resume Order

1. Re-read the product SSoT and the pause handoff.
2. Decide whether research changes the thesis enough to require a `brief.md` update.
3. Resolve the highest-priority research question.
4. Only then pick the first bounded implementation task.

## Research Tasks Before More Implementation

### R-001

Task:
- sharpen the exact product problem statement in plain language

Why it matters:
- the wedge works mechanically, but the idea still needs a tighter human-level statement

Exit signal:
- one short paragraph explains what Specpunk is, what pain it removes, and why a raw diff is not enough

### R-002

Task:
- identify the strongest first user and first buying context

Why it matters:
- the current thesis names likely users, but still needs a sharper entry point

Exit signal:
- one primary user and one primary team context are written down without fallback phrasing

### R-003

Task:
- decide what task-truth source should matter most in v1:
  manual task directory, issue text, PR text, or session-derived context

Why it matters:
- too much support surface too early will blur the wedge

Exit signal:
- one source is named the default runtime path and the others are demoted to validation or later work

### R-004

Task:
- define the minimum evidence artifact beyond file boundary checks

Why it matters:
- `scope` alone is useful but not yet the full reviewer aha moment

Exit signal:
- one concrete next evidence artifact is chosen or explicitly deferred

### R-005

Task:
- validate whether the public surface communicates the idea clearly to the right people

Why it matters:
- the site is live, but the product promise still needs reality checks

Exit signal:
- 3 focused reactions or conversations are logged with the interview template

### R-006

Task:
- decide what to do with PR `#1` during the pause:
  merge it now, leave it open, or refresh it later

Why it matters:
- return friction is lower if repo state is explicit

Exit signal:
- one explicit branch decision is recorded in the next dated review

## First Implementation Tasks After Research

### I-001

Task:
- run `specpunk task init` and `specpunk check --task-dir` on one real repo-local code change

Why it matters:
- this is the missing Milestone 1 proof

Exit signal:
- one real code change in this repo produces a useful `generated-review.md`

### I-002

Task:
- reduce task authoring friction without hiding meaning

Why it matters:
- current task input still depends on manual structure

Exit signal:
- the next task can be created with less ceremony while staying readable

### I-003

Task:
- add the next smallest evidence field only if R-004 justifies it

Why it matters:
- the artifact pack should grow only when the added signal is clear

Exit signal:
- one additional evidence field exists and survives dogfood use

### I-004

Task:
- convert the next useful repo task into a repeatable internal demo

Why it matters:
- the product should keep proving itself on its own repo before broadening scope

Exit signal:
- at least two repo-local tasks share the same compact artifact shape

## Do Not Do First

- do not add new tool integrations before the research pass
- do not widen the CLI surface without a clearer product reason
- do not restart broad benchmark work before the first repo-local Milestone 1 proof is complete
