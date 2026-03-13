---
title: "Specpunk Research Gaps After First Pass"
date: 2026-03-12
status: partially_verified
origin: gap-analysis over local research corpus
---

# Specpunk Research Gaps After First Pass

## Question

What important research dimensions are still underexplored after the first Specpunk research pass?

## Executive Summary

The technical core is already well covered. The corpus is strong on:

- intent evaporation and transcript forensics
- spec-driven development and code-to-spec limits
- natural-language consistency checking
- review bottlenecks and behavior-first verification
- deterministic inference and regeneration limits

The biggest blind spots are not "more technical proof that intent matters." The biggest blind spots are the layers that decide whether the product is adoptable and defensible in real teams:

1. **buyer and rollout economics**
2. **benchmark validity and evaluation design**
3. **operational privacy/compliance model for transcript-derived intent**
4. **tool integration parity and portability limits**

A fifth, lower-priority gap remains around **longitudinal maintenance of intent artifacts**. The corpus identifies drift risk clearly, but does not yet show how a compact intent layer survives over months inside a real team.

## What Is Already Covered Well

### 1. Intent capture and transcript forensics

This is the strongest area in the corpus. We already have:

- detailed Claude Code / Codex / Gemini / Cursor storage analysis
- working extraction design for thinking blocks
- cost model for post-session decision extraction
- provenance and audit-trail framing via PROV / Part 11 analogies

Primary docs:

- `2026-03-12-delve-thinking-blocks.md`
- `2026-03-12-deep-intent-preservation.md`
- `2026-03-12-intent-preservation.md`

### 2. Review bottleneck and behavior-first verification

This area is also strong. We already have:

- review-as-bottleneck synthesis
- behavioral diff and test-as-review wedge
- mutation testing as stronger evidence than raw coverage
- economic framing for AI review tools

Primary docs:

- `2026-03-12-code-review-bottleneck.md`
- `2026-03-12-deep-code-review.md`

### 3. SDD landscape, spec tax, and brownfield difficulty

The corpus already covers:

- CodeSpeak / Tessl / Kiro / Spec Kit landscape
- slowdown and markdown-overhead critiques
- code-to-spec extraction limits
- spec drift and takeover difficulty

Primary docs:

- `2026-03-12-spec-driven-development.md`
- `2026-03-12-deep-spec-driven-dev.md`
- `2026-03-12-delve-sdd-slowdown.md`
- `2026-03-12-code-to-spec-conversion.md`
- `2026-03-12-delve-spec-extraction-gap.md`

## Underexplored Gaps

## P0: Buyer, Budget, and Rollout Motion

### What is already in the corpus

The corpus has fragments:

- SDD adoption friction, training cost, and rollout burden
- review-tool ROI and cost-reduction framing
- spec-tax math and the conditions where the economics flip

### What is missing

We still do not have a product-grade answer to:

- **who buys this first**: engineering manager, platform team, AppSec, compliance, or CTO staff?
- **what forcing function opens budget**: review bottleneck, auditability, security incidents, multi-agent coordination, or onboarding pain?
- **what budget line it competes with**: AI review, platform engineering, AppSec tooling, compliance tooling, or dev productivity budget?
- **what rollout shape is realistic**: bottom-up CLI adoption, platform mandate, or regulated-team wedge?

This matters because the current thesis is technically coherent but still reads like a product for engineers who already agree with the problem. The corpus has not yet converted that into a concrete go-to-market or procurement hypothesis.

### Best local sources

- `2026-03-12-deep-spec-driven-dev.md`
- `2026-03-12-deep-code-review.md`
- `2026-03-12-delve-sdd-slowdown.md`

### Suggested next research

- buyer map by pain type (`review`, `audit`, `security`, `multi-agent coordination`)
- forcing-function map: what event turns this from "nice to have" into mandatory infrastructure
- pricing and packaging study: seat-based vs usage-based vs team-wide compliance layer

## P0: Benchmark Validity and Evaluation Design

### What is already in the corpus

The benchmark protocol is now materially better than the first draft:

- A/B design on identical repo states
- blind diff-only pass separated from artifact-utility pass
- ground truth from merged PR + expert review + tests
- cost tracking and automation skeleton

### What is missing

The research still under-specifies evaluation rigor:

- **inter-rater reliability**: no agreement metric between reviewers
- **statistical power / sample sizing**: no estimate for how many tasks are needed before results are interpretable
- **counterbalancing**: no explicit design for order effects across modes, tools, or reviewers
- **contamination control**: no protocol for reviewers who recognize the original issue or repo history
- **repeatability across model versions**: reproducibility is discussed elsewhere in the corpus, but not operationalized inside the benchmark

This matters because a weak benchmark will produce a persuasive narrative but not a defensible result. Right now the protocol is good enough for exploratory product work, not for a strong research claim.

### Best local sources

- `2026-03-12-brownfield-benchmark-protocol.md`
- `2026-03-12-delve-deterministic-inference.md`
- `2026-03-12-delve-sdd-slowdown.md`

### Suggested next research

- methodology memo for `sample size`, `reviewer agreement`, `mode ordering`, and `contamination control`
- benchmark reproducibility checklist: model pinning, tool version pinning, repo SHA pinning, cached artifacts
- reviewer-quality rubric with explicit false-positive / false-negative accounting

## P1: Operational Privacy, Compliance, and Governance Model

### What is already in the corpus

This area is not empty. The corpus already covers:

- transcript thinking blocks may contain secrets or sensitive rationale
- redaction and fail-closed storage guidance
- local-only / no-cloud recommendations
- append-only audit-trail analogies
- PROV-based provenance and legal/accountability framing
- lack of clear liability precedent for AI-generated code

### What is missing

What we do **not** yet have is an operational policy model:

- **retention and deletion schedule** for raw transcripts vs extracted decisions
- **access-control model** for who can read transcript-derived intent inside a team
- **approval workflow** for high-risk domains like auth, crypto, payments, PII
- **incident handling** when secrets or regulated data are captured before sanitization
- **enterprise policy mapping** for admin controls, residency, encryption, and audit export

The existing research proves that transcript-derived intent is valuable and risky. It does not yet define the control plane needed to ship it in a serious enterprise environment.

### Best local sources

- `2026-03-12-delve-thinking-blocks.md`
- `2026-03-12-deep-intent-preservation.md`
- `2026-03-12-deep-nextgen-langs.md`

### Suggested next research

- policy matrix: `raw transcript`, `extracted decision`, `review artifact`, `evidence` as separate data classes
- minimal governance model: local mode, team mode, regulated mode
- threat model for transcript ingestion, redaction failure, and unauthorized retrieval

## P1: Tool Integration Parity and Portability Limits

### What is already in the corpus

The corpus documents tool differences well:

- Claude Code exposes readable JSONL with thinking blocks
- Codex stores reasoning encrypted and effectively unavailable
- Gemini stores final responses without separate reasoning
- Cursor is partially accessible but schema-fragile
- Archgate, Git AI, SpecStory, and MCP patterns provide useful adjacent primitives

### What is missing

The product implication is still underdeveloped:

- what is the **minimum common denominator** across tools?
- when reasoning is absent or encrypted, what becomes the fallback artifact source?
- can "tool-agnostic" realistically mean one unified extractor, or does it really mean a repo-native core plus capability-specific adapters?
- which enforcement features are truly portable (`scope`, `glossary`, `evidence`) and which are tool-dependent (`thinking extraction`, hook-based capture, sub-agent lineage)?

This matters because the thesis currently uses "tool-agnostic" as a product virtue. The corpus supports portability for some layers, but not equally for all layers.

### Best local sources

- `2026-03-12-delve-thinking-blocks.md`
- `2026-03-12-intent-preservation.md`
- `2026-03-12-deep-intent-preservation.md`

### Suggested next research

- capability matrix by tool: `history access`, `reasoning access`, `hooks`, `sub-agent visibility`, `scope-enforcement insertion points`
- product-tier model: `portable core` vs `Claude-enhanced mode` vs `best-effort adapters`
- fallback design for non-reasoning tools: issues/PRs/code diffs as auto-draft inputs

## P2: Longitudinal Maintenance of Intent Artifacts

### What is already in the corpus

The corpus already recognizes:

- spec drift is real
- stale markdown is a failure mode
- code may remain the practical ground truth in many teams
- spec-heavy workflows become expensive if artifacts are not compact and curated

### What is missing

What we still do not have is longitudinal evidence:

- how often compact intent packs need updates
- who owns them after the initial author
- whether teams actually read them during review after the novelty wears off
- when to split, merge, or delete module-level artifacts

This is lower priority than the four gaps above, but it becomes critical once the first prototype exists.

### Best local sources

- `2026-03-12-deep-spec-driven-dev.md`
- `2026-03-12-spec-driven-development.md`
- `2026-03-12-next-step-product-thesis.md`

## Prioritized Additions to the Research Backlog

1. **Buyer / procurement memo**
   Map the first buyer, budget owner, forcing function, and replacement motion.

2. **Benchmark methodology memo**
   Tighten reviewer agreement, contamination controls, reproducibility, and sample-size logic.

3. **Compliance operating model**
   Define data classes, retention, access controls, and escalation for transcript-derived intent.

4. **Tool capability matrix**
   Split portable features from vendor-specific ones; downgrade "tool-agnostic" where needed.

5. **Longitudinal maintenance study**
   Run one month of real artifact upkeep on a bounded module set and measure drift, update cost, and actual usage.

## Verdict

The missing research is no longer "prove the thesis again." The missing research is:

- whether the product is buyable,
- whether the evidence is defensible,
- whether the privacy model is shippable,
- and whether portability survives actual tool differences.

That is the next layer of work.

## Verification Status

`partially_verified`

This document is a synthesis over the existing local corpus only. It does not add new external verification. Confidence is highest where the corpus already contains direct source-backed analysis, and lower where the conclusion depends on absence of material rather than presence of explicit evidence.

## Confidence

`medium-high`
