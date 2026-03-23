---
title: "Research Gap Map for Specpunk"
date: 2026-03-12
status: gap-analysis
origin: follow-up after synthesis, thesis refinement, and benchmark protocol review
---

# Research Gap Map

## One-line

The research base is already strong on `spec`, `review`, `code->spec`, `NL consistency`, and `thinking-block forensics`. The biggest remaining gaps are around `go-to-market`, `governance`, `evaluation rigor`, `integration portability`, and `artifact maintenance`.

## What Is Already Well Covered

These tracks are no longer the main blind spots:

- `Spec-first / spec-anchored / spec-as-source landscape`
- `CodeSpeak / Tessl / Kiro / Spec Kit category mapping`
- `Review bottleneck and trust gap`
- `Code -> spec extraction limits`
- `Natural-language consistency and contradiction checking`
- `Thinking blocks as lost intent`, including local forensics and extraction mechanics

Representative local anchors:

- `docs/research/2026-03-12-specpunk-synthesis.md`
- `docs/research/2026-03-12-spec-driven-development.md`
- `docs/research/2026-03-12-code-to-spec-conversion.md`
- `docs/research/2026-03-12-code-review-bottleneck.md`
- `docs/research/2026-03-12-nl-consistency-checking.md`
- `docs/research/2026-03-12-delve-thinking-blocks.md`

## Highest-Priority Blind Spots

## P0. Buyer Map, Procurement Trigger, and Pricing Logic

### Already covered

The corpus already has:

- `spec tax` and break-even framing
- rollout/training/support cost
- adjacent ROI logic for AI review tools
- clear evidence that economics flips by team shape and regulatory pressure

Strong anchors:

- `docs/research/2026-03-12-deep-spec-driven-dev.md:697-756`
- `docs/research/2026-03-12-delve-sdd-slowdown.md:873-883`
- `docs/research/2026-03-12-deep-code-review.md:539-558`

### Still missing

- Who is the actual economic buyer for an `intent-control layer`
- Which budget line it replaces or expands
- What the first credible pricing corridor is
- Which event triggers purchase:
  - review SLA collapse
  - incident spike
  - regulated rollout
  - AI rollout beyond pilot stage
- Which ICP should be the first beachhead:
  - platform/devex
  - AppSec
  - compliance-heavy engineering orgs
  - teams already deep on `Claude Code` / `Cursor`

### Why it matters

The technical thesis can be correct while the commercial framing is wrong. Right now the corpus explains why the category could matter, but not who buys first and under which pressure.

### Recommended follow-up

1. Buyer map with `economic buyer`, `champion`, `user`, `blocker`
2. Budget map against `AI coding`, `AI review`, `quality`, `platform`, `compliance`
3. 8-12 customer discovery interviews with teams already using agentic coding tools
4. Rough pricing study using adjacent anchors from review, security, and developer productivity tools

Confidence: `high`

## P0. Privacy, Compliance, and Governance for Transcript-Derived Intent

### Already covered

The corpus already has substantial raw material:

- thinking blocks may contain secrets, PII, business logic, and pre-patch vulnerabilities
- extraction creates a second sensitive corpus
- retention, encryption, and local-only controls are discussed
- prompt provenance is mapped to `W3C PROV`
- transcript trails are compared to `21 CFR Part 11` audit trails

Strong anchors:

- `docs/research/2026-03-12-delve-thinking-blocks.md:865-925`
- `docs/research/2026-03-12-delve-thinking-blocks.md:1040-1044`
- `docs/research/2026-03-12-deep-intent-preservation.md:218-233`
- `docs/research/2026-03-12-deep-intent-preservation.md:388-406`

### Still missing

- Data classification policy by artifact type:
  - raw transcript
  - extracted decision
  - sanitized decision
  - evidence bundle
- Retention matrix by tool and sensitivity level
- Consent and disclosure model for teams extracting session reasoning
- Delete/export workflow for stored intent
- Multi-project isolation rules
- Governance answer for cases where `Claude Code` plaintext is available but `Codex` reasoning is encrypted and `Gemini` exposes no reasoning blocks

### Why it matters

`Automatic draft extraction` is the best adoption wedge and also the highest governance risk. Without an explicit governance model, the feature that makes onboarding easy can block enterprise adoption.

### Recommended follow-up

1. Threat model for `transcript -> extraction -> storage -> retrieval -> PR/CI exposure`
2. Data lifecycle spec with retention and deletion rules
3. Minimal compliance position for `local-only` vs `shared service` deployment
4. Sanitization test suite with seeded secrets/PII and failure policy

Confidence: `high`

## P0. Benchmark Validity and Evidence Methodology

### Already covered

The benchmark protocol now includes:

- `Mode A` vs `Mode B`
- blind diff-only pass
- separate artifact utility pass
- ground-truth framing
- cost tracking

Related evidence in the corpus also warns about weak evaluation design and overinterpreted claims:

- single-task / single-developer studies are not enough
- some celebrated productivity claims are not statistically significant
- reviewer agreement varies sharply by defect type

Strong anchors:

- `docs/research/2026-03-12-brownfield-benchmark-protocol.md:213-353`
- `docs/research/2026-03-12-delve-sdd-slowdown.md:79-79`
- `docs/research/2026-03-12-delve-sdd-slowdown.md:145-149`
- `docs/research/2026-03-12-deep-code-review.md:216-219`

### Still missing

- Inter-rater reliability
- Sample-size and statistical-power plan
- Randomization / crossover logic across repos, tasks, and tools
- Explicit contamination controls:
  - reviewer memory
  - task familiarity
  - artifact leakage
- Stronger ground-truth rubric for `merge / reject / fix-first`
- Decision about how much `behavioral evidence` must be language-specific

### Why it matters

Without stronger evaluation design, the benchmark risks becoming product theater. It may show lower uncertainty while failing to show better review decisions.

### Recommended follow-up

1. Add `2+ reviewers` for a subset and compute agreement
2. Add a small power plan before scaling task count
3. Randomize tool/task ordering to reduce sequence effects
4. Pre-register the rubric for merge/reject decisions
5. Add at least one language with mutation testing and one without, to see whether the evidence layer generalizes

Confidence: `high`

## P0. Tool Integration Parity and the Real Meaning of "Tool-Agnostic"

### Already covered

The corpus already shows that the substrate differs materially by tool:

- `Claude Code`: plaintext JSONL, hooks, accessible summarized thinking
- `Codex CLI`: encrypted reasoning, plaintext tool calls only
- `Gemini CLI`: no separate reasoning blocks, different retention defaults
- `Cursor`: partial reasoning in SQLite, unstable schema, harder extraction

Strong anchors:

- `docs/research/2026-03-12-delve-thinking-blocks.md:590-607`
- `docs/research/2026-03-12-delve-thinking-blocks.md:952-1030`
- `docs/research/2026-03-12-intent-preservation.md:247-271`
- `docs/research/2026-03-12-spec-driven-development.md:75-77`

### Still missing

- Capability matrix by tool:
  - auto-draft from code
  - auto-draft from session
  - scope enforcement
  - review bundle generation
  - live intent retrieval
- Minimal common denominator for a genuinely portable product
- Fallback design when reasoning access is unavailable
- Decision on whether `tool-agnostic` means:
  - same user promise everywhere
  - same repo artifacts everywhere
  - or just same top-level concept with degraded capabilities

### Why it matters

The thesis currently says `tool-agnostic` as a product goal. The corpus says some of the most compelling features are structurally easier on `Claude Code` than on `Codex`, `Gemini`, or `Cursor`. That tension needs an explicit product stance.

### Recommended follow-up

1. Build a capability matrix for `Claude Code`, `Codex`, `Cursor`, `Gemini CLI`
2. Define `portable core` vs `tool-specific enhancer` features
3. Prototype the same task on 2-3 tools and compare what artifact bundle can be produced without privileged internals

Confidence: `high`

## P1. Artifact Maintenance, Drift, and Ownership

### Already covered

The corpus already documents that spec/code sync is a structural problem:

- spec drift is common and measured
- only a minority of formal-methods papers address automated sync
- practitioner consensus often falls back to `code is current truth`
- multiple tool communities still rely on manual discipline

Strong anchors:

- `docs/research/2026-03-12-deep-spec-driven-dev.md:540-584`
- `docs/research/2026-03-12-spec-driven-development.md:198-214`
- `docs/research/2026-03-12-deep-spec-driven-dev.md:332-332`
- `docs/research/2026-03-12-intent-preservation.md:78-80`

### Still missing

- Ownership model for keeping an intent pack current
- Aging/staleness policy
- Update triggers:
  - only on behavior change
  - on every merged PR
  - on named module checkpoints
- Archival/compaction model for old decisions
- Review rule for when stale intent should block a change vs just warn

### Why it matters

This is the most likely place for a good-looking system to die in practice. Many rationale/spec systems failed not because the idea was wrong, but because upkeep cost was badly governed.

### Recommended follow-up

1. Define `fresh`, `stale`, and `archived` artifact states
2. Add ownership rules to the product thesis
3. Run a small maintenance study on one real module over several sequential changes

Confidence: `high`

## Lower-Priority But Still Useful Gaps

- `Behavior evidence by language`: mutation testing, contract testing, PBT, and differential testing need a language/tooling matrix, not a generic promise
- `Standards path`: `PROV`, `Agent Trace`, and related schemas are present in the corpus, but not yet converted into an interoperability strategy
- `Cross-functional workflow`: PM/BA/designer participation is recognized as an adoption blocker, but there is no concrete repo workflow for non-engineers

## Suggested Research Queue

If the goal is to reduce uncertainty fastest, the next queue should be:

1. `Buyer / budget / ICP` study
2. `Transcript governance` threat model
3. `Benchmark methodology appendix`
4. `Tool capability matrix`
5. `Artifact maintenance governance`

## What Not to Spend Another Week On

Do not spend the next cycle on:

- another broad competitor teardown
- more generic evidence that `review is the bottleneck`
- more generic evidence that `spec drift exists`

Those are already established strongly enough for the current stage.

## Bottom Line

The current research stack already supports the core technical thesis.

What it does **not** yet support well enough is:

- who buys,
- under which governance constraints,
- how the benchmark becomes methodologically defensible,
- how portable the product really is across tools,
- and how intent artifacts stay alive after the first enthusiastic week.

That is where the next research delta is.
