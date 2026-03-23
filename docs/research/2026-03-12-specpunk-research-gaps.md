---
title: "Specpunk Research Gaps"
date: 2026-03-12
status: gap-analysis
origin: local-corpus delve pass
scope: docs/research only
---

# Specpunk Research Gaps

## Verdict

The current corpus is already strong on the core product thesis:

- review bottleneck and trust gap
- spec-driven development landscape
- code-to-spec limitations
- terminology / contradiction checking
- transcript-derived intent and thinking-block extraction
- initial brownfield benchmark design

What is still under-researched is not "another technical proof that specs matter." The bigger blind spots are around evaluation rigor, enterprise adoption, privacy/governance, tool asymmetry, and artifact lifecycle.

## What Is Already Covered Well

### 1. Core technical problem map

The synthesis is already coherent on the main chain:

`intent loss -> review bottleneck -> weak verification -> brownfield risk`

Strong anchors:

- `docs/research/2026-03-12-specpunk-synthesis.md`
- `docs/research/2026-03-12-next-step-product-thesis.md`
- `docs/research/2026-03-12-code-review-bottleneck.md`
- `docs/research/2026-03-12-nl-consistency-checking.md`

### 2. Transcript / intent extraction feasibility

The corpus already goes unusually deep on transcript storage, thinking blocks, and extraction mechanics:

- Claude Code / Codex / Gemini / Cursor storage comparison
- hook constraints
- redaction caveats
- provenance / audit-trail analogies

Strong anchors:

- `docs/research/2026-03-12-delve-thinking-blocks.md`
- `docs/research/2026-03-12-deep-intent-preservation.md`
- `docs/research/2026-03-12-intent-preservation.md`

### 3. First-pass benchmark design

The brownfield benchmark protocol already covers:

- A/B design
- diff-only blind pass
- separate artifact utility pass
- task packets
- cost tracking
- basic ground-truth construction

Strong anchor:

- `docs/research/2026-03-12-brownfield-benchmark-protocol.md`

## Highest-Priority Blind Spots

### 1. Benchmark rigor is still below research-grade

**What is covered**

- Blind review is now correctly separated from artifact utility.
- Ground truth is defined from merged PRs, expert review, and checklist.
- Cost and wall-clock deltas are tracked.

Sources:

- `docs/research/2026-03-12-brownfield-benchmark-protocol.md:241-353`
- `docs/research/2026-03-12-delve-deterministic-inference.md:453-490`

**What is missing**

- No inter-rater reliability plan (`kappa`, disagreement protocol, adjudication).
- No sample-size / statistical-power reasoning for `5 repos x 3-5 tasks`.
- No counterbalancing of task order, reviewer order, or tool order.
- No contamination control for reviewer familiarity with the original PR or repo.
- No explicit benchmark manifest for model version pinning, tool version pinning, or system fingerprint capture.

Why this matters:

Without this, the benchmark is good for product iteration but weak as evidence. It can tell you "this looks promising," but not "this result is robust."

Priority: `P0`

### 2. Buyer map and land motion are still fuzzy

**What is covered**

- There is some ROI framing.
- There is rollout-friction research.
- There is a plausible "mandatory infrastructure" analogy via Snyk / compliance pressure.

Sources:

- `docs/research/2026-03-12-deep-spec-driven-dev.md:697-756`
- `docs/research/2026-03-12-deep-code-review.md:539-586`
- `docs/research/2026-03-12-delve-sdd-slowdown.md:340-342`
- `docs/research/2026-03-12-delve-sdd-slowdown.md:613-621`

**What is missing**

- Who actually buys this first: `EM`, `platform`, `AppSec`, `DevEx`, or compliance.
- What event triggers purchase: incident spike, review backlog, AI rollout mandate, audit finding.
- What the initial land motion is: single team, regulated workflow, or enterprise platform pilot.
- What the budget line is: coding-tool spend, review-tool spend, compliance tooling, or platform engineering.

Why this matters:

The product thesis is getting sharper, but go-to-market remains too implicit. Right now the corpus explains why the pain exists, not who will urgently pay to remove it.

Priority: `P0`

### 3. Privacy / compliance coverage is strong on mechanics, weak on policy

**What is covered**

- Audit-trail analogy to regulated systems.
- Secret redaction patterns.
- Local DB access-control recommendations.
- "No cloud upload without consent" posture.
- Signature invalidation and transcript integrity caveats.

Sources:

- `docs/research/2026-03-12-deep-intent-preservation.md:388-406`
- `docs/research/2026-03-12-delve-thinking-blocks.md:748-753`
- `docs/research/2026-03-12-delve-thinking-blocks.md:905-924`

**What is missing**

- Data-retention classes by artifact type.
- Access model for team environments: who can read raw transcripts vs extracted decisions.
- Deletion/export policy for captured intent.
- Admin / enterprise posture for sensitive repos.
- Formal treatment of PII, customer data, privileged material, and legal-hold scenarios.
- A clear answer to whether transcript extraction is viable by default outside single-user local mode.

Why this matters:

The current work proves extraction is technically possible. It does not yet prove it is organizationally deployable.

Priority: `P0`

### 4. Tool-agnostic story is still a hypothesis, not a support matrix

**What is covered**

- Strong comparative evidence exists for session storage asymmetry:
  - Claude Code: accessible JSONL thinking blocks
  - Codex: encrypted reasoning
  - Gemini CLI: no separate reasoning blocks
  - Cursor: partial / undocumented SQLite-based access

Sources:

- `docs/research/2026-03-12-delve-thinking-blocks.md:942-1025`
- `docs/research/2026-03-12-next-step-product-thesis.md:145-165`
- `docs/research/2026-03-12-intent-preservation.md:247-271`

**What is missing**

- Explicit `support matrix` for each feature:
  - auto-draft from transcript
  - scope enforcement
  - terminology checking
  - invariant checking
  - provenance / blame linkage
  - sub-agent attribution
- Clear product position on degraded modes when reasoning is encrypted or absent.
- Benchmark stratification by tool family.
- Decision on whether v1 is honestly `Claude-first` even if long-term ambition stays tool-agnostic.

Why this matters:

Today, "tool-agnostic" is best understood as a product goal, not an already-supported technical fact.

Priority: `P0`

### 5. Artifact lifecycle and drift need their own research thread

**What is covered**

- Spec drift is well established as a real problem.
- Unclear ownership is already identified as one cause.
- Long-term maintenance burden is explicitly flagged as a product risk.

Sources:

- `docs/research/2026-03-12-deep-spec-driven-dev.md:540-552`
- `docs/research/2026-03-12-deep-intent-preservation.md:206`
- `docs/research/2026-03-12-codespeak-crosscheck.md:176-183`

**What is missing**

- Update triggers: when `intent.md` or `glossary.md` must be revised.
- Ownership model: code owner, module owner, reviewer, or agent-assisted upkeep.
- Stale-artifact detection rules.
- Merge / split / retire rules for module intent packs.
- Compactness guardrails that keep artifacts short instead of drifting into verbose markdown.

Why this matters:

This is the main failure mode that could turn Specpunk into the exact thing the thesis rejects: another pile of stale documentation.

Priority: `P1`

### 6. Behavior evidence is directionally right, but the minimum viable stack is still underspecified

**What is covered**

- The corpus already identifies behavioral diffing, property-based testing, mutation testing, and differential testing as the strongest verification direction.

Sources:

- `docs/research/2026-03-12-code-review-bottleneck.md:80-90`
- `docs/research/2026-03-12-code-review-bottleneck.md:118`
- `docs/research/2026-03-12-deep-code-review.md:592-615`
- `docs/research/2026-03-12-deep-code-to-spec.md:583`

**What is missing**

- A language-agnostic minimum evidence stack for v1.
- Escalation rules:
  - when plain tests are enough
  - when to add PBT
  - when mutation score is worth the cost
  - when dual-running / shadow mode is required
- Reviewer UX for behavior evidence beyond prose summary.

Why this matters:

Right now "behavior-first evidence" is a strong direction, but still not a bounded product surface.

Priority: `P1`

## Recommended Next Research Pack

If the goal is to reduce uncertainty fastest, the next pass should be:

1. **Benchmark methodology addendum**
   - Add reviewer-adjudication rules, inter-rater reliability, contamination controls, model/tool pinning, and minimum sample assumptions.

2. **Support matrix**
   - Claude Code / Codex / Cursor / Gemini CLI by feature, with `supported`, `degraded`, `not possible`.

3. **Privacy and governance note**
   - Local-only vs team-shared vs enterprise-managed transcript handling, with retention and access defaults.

4. **Buyer / trigger map**
   - Who buys first, under what pain, and in which segment: regulated teams, platform teams, AI-heavy teams with review backlog.

5. **Artifact lifecycle policy**
   - Staleness rules, ownership, update triggers, and a hard compactness budget.

## Bottom Line

The main thing being missed is not another competitor or another paper proving that specs help. The missing layer is the one that decides whether the thesis can survive contact with reality:

- can it be evaluated rigorously,
- can it be deployed safely,
- can it work across real tools,
- can someone actually buy it,
- and can the artifacts stay alive without becoming documentation sludge.

## Confidence

`high` for the claim that these are real blind spots in the current corpus.

Reason:

- the covered areas are backed by multiple existing documents;
- the missing areas are either only partially addressed or absent in direct form;
- the strongest uncertainty is around the buyer / procurement side, because the corpus contains adoption economics and rollout friction, but not a fully explicit market map.
