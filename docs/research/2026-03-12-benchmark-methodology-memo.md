---
title: "Specpunk Benchmark Methodology Memo"
date: 2026-03-12
status: memo
origin: follow-up after brownfield benchmark protocol and research gap mapping
scope: local corpus only
---

# Specpunk Benchmark Methodology Memo

## Verdict

The current benchmark protocol is good enough for **product iteration**, but not yet strong enough for **defensible evidence**.

The fix is not "make it academic." The fix is to separate two benchmark modes:

1. `Product benchmark`
   Goal: fast learning for feature and workflow iteration

2. `Evidence benchmark`
   Goal: support stronger claims about review quality, containment, and overhead

Right now the protocol partially mixes these two goals.

## Scope Note

This memo is based on the local corpus only.

Important:

- `Corpus-supported` means directly grounded in existing local research.
- `Recommended` means a methodology decision inferred from those sources.
- This memo is about benchmark design, not about changing the product thesis.

## What Is Already Good In The Current Protocol

The existing brownfield protocol already has important strengths:

- same-task `Mode A` vs `Mode B`
- explicit cost tracking
- blind diff-only review pass
- separate artifact utility pass
- task packets and repo selection criteria
- explicit ground-truth section

Source anchor:

- `2026-03-12-brownfield-benchmark-protocol.md:213-353`

That is enough to learn from a pilot. It is not enough to claim robust causal conclusions.

## Why The Current Design Is Still Weak

The corpus already warns against exactly these methodology failures:

1. Single-user or single-task studies are misleading (`2026-03-12-delve-sdd-slowdown.md:79-81`)
2. Some celebrated AI productivity claims were not statistically significant (`2026-03-12-delve-sdd-slowdown.md:145-149`)
3. Review findings depend heavily on reviewer type and issue category; agreement is asymmetric (`2026-03-12-deep-code-review.md:217-219`, `2026-03-12-deep-code-review.md:365-367`)
4. Reproducibility breaks when models and tools are not pinned (`2026-03-12-delve-deterministic-inference.md:453-490`)
5. Review-time measurements can be confounded by noise in generated artifacts rather than true value (`2026-03-12-delve-sdd-slowdown.md:697-711`)

This means the benchmark must control for:

- who reviews
- what task is selected
- in what order modes are seen
- what tool/model/version produced the result
- whether artifact noise is being mistaken for artifact value

## Core Methodology Principle

Benchmark the smallest meaningful unit:

`repo + task + tool + mode + reviewer decision`

Not:

- generic "team productivity"
- one-off anecdotal task completion
- aggregate prompt impressions

## Two-Tier Benchmark Design

## Tier 1: Product Benchmark

Purpose:

- fast signal for feature iteration
- identify obvious failures in scope control, artifact noise, and review workflow

Recommended shape:

- 5 repos
- 2-3 tasks per repo
- 1 reviewer per task in blind pass
- 1 tool first, then cross-tool later

Acceptable weaknesses:

- no formal power calculation yet
- limited inter-rater coverage
- more heuristic thresholds

Use for:

- deciding if a feature is worth keeping
- identifying noisy artifacts
- validating workflow feasibility

## Tier 2: Evidence Benchmark

Purpose:

- support stronger product claims
- compare modes with more defensible methodology

Required additions:

- reviewer agreement measurement
- randomized ordering
- explicit contamination controls
- benchmark manifest for reproducibility
- pre-declared analysis plan

Use for:

- internal strategy decisions
- investor/customer-facing evidence
- positioning claims beyond "promising pilot"

## Experimental Unit And Pairing

Recommended rule:

- compare modes within the same task whenever possible

Why:

- task difficulty varies too much across brownfield work
- within-task comparison reduces variance better than comparing unrelated tasks

Recommended unit record:

```yaml
benchmark_run:
  run_id: "..."
  repo: "..."
  repo_sha: "..."
  task_id: "..."
  task_difficulty: "S|M|L"
  language: "python"
  tool: "claude-code"
  tool_version: "..."
  model: "..."
  model_snapshot: "..."
  system_fingerprint: "..."
  mode: "baseline|intent_control"
  reviewer_id: "..."
  artifact_bundle_version: "..."
```

This is a recommended structure, not a sourced format.

## Reviewer Design

## Blind pass remains diff-only

Keep the current rule:

- reviewers in the blind pass see only task packet, diff, changed files, and tests

This protects against treatment leakage and should stay as-is.

Source anchor:

- `2026-03-12-brownfield-benchmark-protocol.md:295-317`

## Add agreement measurement

Recommended:

- use at least `2 reviewers` on a subset of tasks
- compute simple agreement first
- if the benchmark scales, add a formal agreement metric such as Cohen/Krippendorff later

Reason:

- the corpus already shows reviewer/model agreement changes sharply by finding type
- without agreement measurement, a "better" mode may just be matching one reviewer's preferences

## Add adjudication

Recommended:

- if blind reviewers disagree on merge/reject, send the task to a third adjudicator
- log the disagreement category:
  - correctness uncertainty
  - containment uncertainty
  - insufficient evidence
  - artifact noise

This is more informative than collapsing everything into one score.

## Ground Truth Design

The current protocol already uses:

- merged PR when available
- expert review
- test suite and checklist

Source anchor:

- `2026-03-12-brownfield-benchmark-protocol.md:346-353`

Recommended tightening:

1. Distinguish `gold`, `silver`, and `bronze` ground truth

- `gold`: actual merged PR + clear acceptance behavior
- `silver`: strong checklist + expert adjudication
- `bronze`: tests pass but expected behavior still partly inferred

2. Record whether the task has a reliable behavioral oracle

Examples:

- contract tests
- parser fixtures
- stable CLI output
- web behavior with existing tests

3. Separate `incorrect merge rejection` from `unsafe merge approval`

These are different errors with different product implications.

## Randomization And Counterbalancing

This is one of the biggest missing pieces.

Recommended minimum:

1. Randomize whether Mode A or Mode B is executed first per task
2. Randomize review order across tasks
3. Randomize cross-tool order once multiple tools are used

Why:

- familiarity accumulates
- reviewers learn repo/task context over time
- later runs may benefit from operator experience rather than product value

## Contamination Controls

The gap report already identified this as a missing area.

Source anchor:

- `2026-03-12-research-gap-map.md:152-175`

Recommended controls:

1. Exclude reviewers who already know the original PR outcome for that task when possible
2. Record reviewer familiarity with the repo:
   - none
   - light
   - strong
3. Hide all mode-specific metadata in blind pass
4. Do not let the same reviewer see both blind outputs for the same task back-to-back
5. Treat operator familiarity as a variable:
   - first run on tool
   - experienced run on tool

## Reproducibility Manifest

This is mandatory for any evidence-grade benchmark.

The corpus is explicit that missing version pinning undermines reproducibility.

Source anchor:

- `2026-03-12-delve-deterministic-inference.md:453-490`

Every task run should record:

- repo commit SHA
- task packet version
- tool version
- model snapshot/version
- system fingerprint when available
- benchmark runner version
- test command and dependency lock state
- date/time and environment

Recommended rule:

- treat any change in model snapshot or tool version as a new benchmark condition, not the same series

## Sample Size Strategy

The corpus does not provide enough effect-size data for a serious up-front power calculation.

So the practical recommendation is staged:

### Stage 1: Calibration pilot

- run a small pilot first
- estimate variance on:
  - review accuracy
  - scope adherence
  - artifact prep overhead
  - reviewer confidence

### Stage 2: Scale only after variance is known

- use pilot variance to size the next batch
- do not present `5 repos x 3 tasks` as inherently sufficient

This is better than pretending the initial batch is statistically justified when it is not.

## Analysis Plan

Recommended primary outcomes:

1. `Review accuracy`
   The most important metric

2. `Scope adherence`
   The most product-specific hard signal

3. `Reviewer confidence`
   Important, but secondary to accuracy

4. `Overhead delta`
   Time + token + artifact prep cost

Recommended secondary outcomes:

- reviewability score
- diff size
- out-of-scope file count
- true/false positive rate for terminology warnings
- evidence usefulness in non-blind pass

Recommended reporting:

- paired within-task comparison
- confidence intervals where possible
- median and distribution, not just averages
- stratification by:
  - task size
  - repo type
  - language
  - tool

## Error Accounting

The benchmark should explicitly count:

### Reviewer false positive

Reviewer rejects or flags a good change unnecessarily.

### Reviewer false negative

Reviewer approves an unsafe or incorrect change.

### Automation false positive

`specpunk check` flags a problem that expert adjudication does not confirm.

### Automation false negative

The check misses a scope/terminology/invariant problem later found by experts.

This is better than a single "confidence improved" narrative.

## Artifact Noise Control

The corpus already warns that generated markdown can distort the measured cost of a workflow.

Source anchor:

- `2026-03-12-delve-sdd-slowdown.md:697-711`

Recommended rule:

- measure artifact size directly
- record reviewer read time separately from total task time
- ask whether each artifact reduced uncertainty or just added prose

This helps distinguish:

- true evidence value
- from faux context

## Tool And Model Effects

Once the benchmark goes cross-tool, treat tool/model as first-class factors.

Why:

- the tool capability matrix already shows materially different transcript and reasoning surfaces
- some value may come from `portable core`
- some value may come only from richer Claude-specific features

Recommended benchmark question split:

1. Does the portable core help regardless of tool?
2. How much additional value comes from Claude-first enhancements?

## Language-Specific Evidence Layer

Do not require the same evidence stack everywhere.

Recommended:

- where mutation tooling is mature, include mutation signal
- where it is not, use the best available behavioral oracle

Why:

- the corpus supports mutation testing as strong signal, but also shows its CI cost and language variance (`2026-03-12-deep-code-review.md:104-141`)

This means the benchmark should report:

- `behavioral evidence used`
- `evidence maturity for that language`

Not pretend the same rigor exists everywhere.

## Stop Conditions

Recommended stop conditions for a benchmark branch:

1. artifact prep overhead consistently exceeds plausible value
2. reviewer accuracy does not improve after multiple tasks
3. automated checks produce unacceptable false-positive noise
4. reproducibility breaks because tool/model versions drift mid-series

These are methodology stop conditions, not product death sentences.

## Minimal Evidence-Grade Upgrade Path

If you want to upgrade the current protocol with minimal extra complexity, do this:

1. Add benchmark manifest per run
2. Add second reviewer on a subset
3. Randomize mode order
4. Record repo familiarity and contamination
5. Report confidence intervals or at least variance, not just means
6. Split portable-core results from Claude-enhanced results

That gets most of the benefit without turning the benchmark into a paper-writing exercise.

## Bottom Line

The right benchmark stance is:

- **pilot fast**
- **separate learning from proof**
- **measure accuracy, not just confidence**
- **pin versions**
- **treat reviewer disagreement as signal**

If Specpunk wants to claim better review and safer brownfield change control, the benchmark must be designed to show exactly that, not just that people liked the artifacts.
