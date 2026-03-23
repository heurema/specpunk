---
title: "Specpunk Strategy Pack"
date: 2026-03-12
status: overview
origin: consolidated index over thesis, capability, governance, GTM, and benchmark memos
---

# Specpunk Strategy Pack

## Purpose

This file is the canonical entry point for the current Specpunk research pack.

Use it to answer four questions quickly:

1. What is the product thesis?
2. Where is the product actually portable vs tool-specific?
3. What governance/commercial constraints shape the product?
4. What evidence plan is strong enough to validate the thesis?

## Executive Synthesis

Specpunk is most defensibly framed as a **repo-native intent-control and verification layer** for AI-assisted brownfield development.

It should **not** be framed as:

- a new IDE
- a new coding agent
- a spec-as-source compiler
- a uniformly tool-agnostic reasoning layer

The current pack supports this narrower but stronger position:

- product wedge: `intent pack + scope enforcement + behavior-first review evidence`
- user: `AI-heavy brownfield teams`, not solo/small-team vibe coding
- commercial frame: `review/risk/control infrastructure`, not "more code generation"
- support posture: `portable core` plus `Claude-first` reasoning-aware enhancements
- governance posture: `local-first`, `derived artifacts over raw transcripts`, `human-owned classification`
- validation posture: benchmark for `accuracy + containment + overhead`, not just artifact preference

## Canonical Documents

Read these in order:

1. [Product Thesis](./2026-03-12-next-step-product-thesis.md)
   Core product shape, target user, artifact set, MVP, and differentiation.

2. [Tool Capability Matrix](./2026-03-12-tool-capability-matrix.md)
   What is genuinely portable across `Claude Code`, `Codex CLI`, `Cursor`, and `Gemini CLI`, and what is not.

3. [Compliance Operating Model](./2026-03-12-compliance-operating-model.md)
   Data classes, retention, deployment modes, access model, and tool-specific governance rules for transcript-derived intent.

4. [Buyer and Procurement Memo](./2026-03-12-buyer-procurement-memo.md)
   First ICP, buyer path, budget adjacency, trigger events, and land motion.

5. [Benchmark Methodology Memo](./2026-03-12-benchmark-methodology-memo.md)
   How to validate the product with evidence strong enough to matter.

## Supporting Documents

These are supporting context, not the primary entry point:

- [Research Gap Map](./2026-03-12-research-gap-map.md)
- [Brownfield Benchmark Protocol](./2026-03-12-brownfield-benchmark-protocol.md)
- [Specpunk Synthesis](./2026-03-12-specpunk-synthesis.md)

## What The Pack Now Says

### Product

Build the first version around:

- `intent.md`
- `glossary.md`
- `invariants.md`
- `scope.yml`
- `evidence.md`
- `review.md`

The key sequence remains:

`auto-draft -> human-curated compact artifact -> enforcement -> evidence`

### Portability

The portable core is:

- repo-native artifacts
- code-based draft generation
- scope checks
- terminology/invariant checks
- review/evidence bundle generation

The non-portable layer is:

- raw reasoning extraction
- real-time transcript interception
- rich in-session retrieval

So the honest support posture is:

- `Tier A`: Claude Code
- `Tier B`: Cursor, Codex CLI
- `Tier C`: Gemini CLI

### Governance

If transcript-derived intent exists, raw session data must be treated as high-risk material.

Default posture:

- `local-first`
- `sanitized approved artifacts` as the durable truth
- `raw transcript retention` exceptional, not default
- `human approval` before durable sharing

### Commercial Direction

The strongest first wedge is:

- teams already using AI coding tools
- in shared brownfield codebases
- where review/trust/coordination pain is already visible

Most plausible first buyer path:

- `platform / DevEx / VP Eng`

with:

- `EM` as pilot champion
- `senior IC` as design partner

### Validation

The benchmark should answer:

1. Does the product improve `review accuracy`?
2. Does it reduce `out-of-scope change`?
3. What is the real `overhead delta`?
4. How much value comes from the portable core vs Claude-specific enhancements?

The current methodology stance is:

- use a fast `product benchmark` for iteration
- use a stricter `evidence benchmark` for stronger claims

## Recommended Execution Order

Do this next:

1. Build implementation-ready templates for the artifact pack
2. Create benchmark manifests and task packets
3. Run the first portable-core benchmark batch
4. Run a smaller Claude-enhanced comparison batch
5. Start customer discovery against the buyer memo hypotheses

## What Not To Do Next

Do not spend the next cycle on:

- another broad market scan
- another generic competitor teardown
- trying to sell "natural language programming" too early
- pretending the product has uniform cross-tool parity

## Open Questions To Validate In Execution

These are now the main unresolved questions:

1. Which artifact creates the strongest first pull: `scope`, `review bundle`, or `intent continuity`?
2. Does the first budget open under `platform/DevEx`, `EM/VP Eng`, or `compliance`?
3. How much extra value comes from Claude-first enhancements beyond the portable core?
4. What retention/deployment posture do real teams accept for transcript-derived workflows?

## Bottom Line

This pack now supports a coherent position:

- **Specpunk is not an AI coding tool**
- **Specpunk is control, verification, and review infrastructure for AI-assisted brownfield change**

That is the strategy worth testing in implementation.
