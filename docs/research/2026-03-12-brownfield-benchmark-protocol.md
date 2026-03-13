---
title: "Brownfield Benchmark Protocol for Intent-Control Layer"
date: 2026-03-12
status: v2
origin: cross-check + thesis review feedback
---

# Brownfield Benchmark Protocol

## Goal

Measure whether an intent-control layer improves AI-assisted work in existing codebases.

The benchmark answers:

1. Does scope enforcement reduce unsafe cross-boundary edits?
2. Does intent + evidence improve reviewer confidence and accuracy?
3. Does behavior evidence catch issues that raw diff + tests miss?
4. Does artifact overhead stay bounded relative to task complexity?
5. What is the cost delta (time + tokens) of the controlled mode?

## Core Hypothesis

Compared to a baseline agent workflow, an intent-control layer with scope enforcement, module intent, glossary/invariants, and behavior evidence should improve **review quality and change containment** in brownfield tasks, even if it does not maximize raw coding speed.

## Design

A/B comparison. Each task runs in two modes on identical repo state.

### Mode A: Baseline

Standard agent workflow (Claude Code, Codex, or Cursor) with normal repo context.
No additional artifacts. Agent receives only the task description.

### Mode B: Intent-Control

Same tool, same repo, same task, plus:

- Module intent pack (`intent.md`, `glossary.md`, `invariants.md`)
- Explicit allowed scope (`scope.yml`)
- Required evidence summary (`evidence.md`)
- Generated review artifact (`review.md`)

The intent pack is bootstrapped before the task (auto-draft + human edit, timed separately).

## Repo Selection

### Criteria

- Active project with working test suite
- Mature brownfield codebase (not toy demo)
- Different sizes and domains
- No private infrastructure required
- Clear module boundaries definable

### Candidate Repos

| Repo | Domain | Size | Why |
|------|--------|------|-----|
| `yt-dlp` | CLI/parser | ~120K LOC | Good brownfield candidate; also gives continuity with prior CodeSpeak examples |
| `FastAPI` | Backend framework | ~30K LOC | Plausible candidate with recognizable module boundaries and active tests |
| `httpx` | HTTP client | ~20K LOC | Plausible candidate with clear protocol-facing behavior and moderate size |
| `Faker` | Data generation | ~50K LOC | Good candidate for module-level tasks; also overlaps with prior CodeSpeak examples |
| `Starlette` | Web framework | ~15K LOC | Plausible candidate with moderate size and active tests |
| `tree-sitter-python` | Parser | ~5K LOC | Candidate for strict-behavior parser tasks if setup friction stays low |
| `click` | CLI framework | ~15K LOC | Plausible candidate with stable API-oriented tasks |

Suggested initial batch: start with 5. Add more only if patterns are unclear.

### Pre-Benchmark Baseline

Before running tasks, establish baseline metrics per repo:

1. Take 20 closed issues/PRs from each repo
2. Run Claude Code on each without constraints
3. Measure:
   - Files touched vs "ideal scope" (from actual merged PR)
   - Out-of-scope edit rate (baseline)
   - Average diff size
   - Test pass rate
4. This gives a rough uncontrolled baseline that later Mode B comparisons can be measured against

## Task Selection

3-5 tasks per repo, sourced from closed issues or well-scoped feature requests.

### Task Types

- Add a small feature to an existing module
- Fix a bug with explicit expected behavior
- Modify behavior without changing public API
- Extend parsing/formatting/validation logic
- Add or change tests for an existing component

### Avoid for v1

- Pure refactors with no behavioral oracle
- Broad architectural rewrites
- Tasks requiring design decisions across half the repo
- Tasks with unclear acceptance criteria

### Task Difficulty Levels

Each repo should have tasks at multiple difficulty levels:

| Level | Scope | Example |
|-------|-------|---------|
| S | Single file, <50 LOC change | Fix edge case in parser |
| M | 2-5 files, one module | Add new output format |
| L | 5-10 files, cross-module | Add filtering feature with CLI + core + tests |

## Task Packet

Every benchmark task gets an identical packet for both modes:

```yaml
task:
  id: "yt-dlp-001"
  repo: "yt-dlp/yt-dlp"
  commit: "abc123def"
  difficulty: "M"
  statement: "Add WebVTT timestamp offset support"
  issue_url: "https://github.com/yt-dlp/yt-dlp/issues/XXXX"
  target_module: "yt_dlp/postprocessor/ffmpeg.py"
  expected_behavior: |
    When --sub-offset N is passed, all WebVTT timestamps
    shift by N seconds. Negative values shift backward.
    Existing tests must still pass.
  setup: "pip install -e '.[dev]' && python -m pytest tests/"
  test_command: "python -m pytest tests/test_postprocessor.py -v"
  evaluation_checklist:
    - timestamps shift correctly for positive offset
    - timestamps shift correctly for negative offset
    - no regression in existing subtitle tests
    - error handling for invalid offset values
```

## Intent-Control Artifacts (Mode B only)

### Preparation (timed separately)

Before the task, create minimal artifacts for the target module:

1. `specpunk init <module>` → auto-draft intent pack from code
2. Engineer reviews and edits (initial target: <10 minutes)
3. `specpunk scope <task-id> --allow "<patterns>"` → scope declaration

### Artifact Specs

**intent.md** — module purpose, constraints, key behaviors. Must be shorter than the module code.

**glossary.md** — domain terms with canonical meanings. Only terms that matter for this module.

**invariants.md** — rules that must remain true. API contracts, data integrity, safety constraints.

**scope.yml**:
```yaml
task: "yt-dlp-001"
allowed:
  - "yt_dlp/postprocessor/ffmpeg.py"
  - "tests/test_postprocessor.py"
  - "yt_dlp/options.py"  # CLI flag registration
forbidden:
  - "yt_dlp/extractor/**"  # extractors must not be touched
```

**evidence.md** (filled after execution):
```markdown
## Behavior Delta
- New: --sub-offset flag shifts WebVTT timestamps
- Changed: FFmpegSubtitlesConvertor now accepts offset parameter
- Unchanged: all existing subtitle conversion paths

## Tests
- Added: 3 new tests (positive offset, negative offset, zero offset)
- Existing: 47 passed, 0 failed
- Coverage delta: +2.1% in postprocessor module

## Known Gaps
- Not tested: offset values exceeding subtitle duration
- Not tested: interaction with --sub-format conversion
```

## Procedure Per Task

```
1. Checkout repo at fixed commit SHA
2. Record baseline metadata (file count, test command, task packet)
3. ─── Mode A ───
   a. Start timer
   b. Run agent with task statement only
   c. Stop timer
   d. Save: prompt, changed files, diff, test results, token usage
   e. Reset to clean state (git worktree or git checkout)
4. ─── Mode B ───
   a. Start artifact prep timer
   b. Bootstrap intent pack (auto-draft + human edit)
   c. Declare scope
   d. Stop artifact prep timer
   e. Start task timer
   f. Run agent with task statement + intent artifacts in context
   g. Stop task timer
   h. Run specpunk check (scope + terminology + invariants)
   i. Generate evidence.md
   j. Generate review.md
   k. Save: all Mode A outputs + intent artifacts + check results + evidence
5. ─── Review evaluation ───
   a. Run blind review pass (see Reviewer Protocol)
   b. Run quality review pass
   c. Record all metrics
```

## Measurements

### Primary Metrics

| Metric | How Measured | Why It Matters |
|--------|-------------|----------------|
| **Task correctness** | Evaluation checklist + test results | Did the change do what was intended? |
| **Scope adherence** | Files changed outside scope.yml | Did the agent stay in bounds? |
| **Reviewer confidence** | 0-2 ordinal scale | Would the reviewer merge this? |
| **Review accuracy** | Blind review: correct merge/reject decision vs ground truth | Does reviewer make better decisions? |

### Secondary Metrics

| Metric | How Measured |
|--------|-------------|
| Total changed files | `git diff --stat` |
| Total diff size (lines) | `git diff --numstat` |
| Out-of-scope files | Diff against scope.yml |
| Tests added/changed | Count in test directories |
| Coverage delta | `pytest --cov` before/after |
| Terminology violations (true positive) | `specpunk check` output, manually verified |
| Terminology violations (false positive) | `specpunk check` output, manually verified |
| Invariant violations | `specpunk check` output |
| Task time (minutes) | Wall clock, agent execution only |
| Artifact prep time (minutes) | Wall clock, Mode B only |
| Token usage | API billing, both modes |
| Token cost ($) | At current API pricing |

### Cost Tracking

Every run records:

```yaml
cost:
  mode_a:
    tokens_input: 45000
    tokens_output: 12000
    api_cost_usd: 0.18
    wall_time_minutes: 8.5
  mode_b:
    artifact_prep_minutes: 7.0
    tokens_input: 62000
    tokens_output: 15000
    api_cost_usd: 0.24
    wall_time_minutes: 11.2
    check_time_seconds: 3.4
  delta:
    cost_increase_pct: 33
    time_increase_pct: 32
    artifact_prep_pct_of_total: 38
```

This helps estimate the "Spec Tax" — if Mode B costs materially more than Mode A, the quality signal needs to justify the overhead.

## Scoring Rubric

### Correctness (0-2)

- `0` = incorrect or incomplete (fails evaluation checklist)
- `1` = partially correct (passes some checklist items)
- `2` = correct by available evidence (passes all checklist items)

### Scope Adherence (0-2)

- `0` = major spill (3+ files outside scope, or forbidden area touched)
- `1` = minor spill (1-2 files outside scope, non-forbidden)
- `2` = fully contained (all changes within scope.yml)

### Reviewability (0-2)

- `0` = hard to review (large diff, no narrative, unclear purpose)
- `1` = reviewable with effort (moderate diff, some structure)
- `2` = clear and compact (focused diff, clear intent, good evidence)

### Confidence (0-2)

- `0` = reviewer would not merge
- `1` = merge with caution (would add follow-up tasks)
- `2` = comfortable merge (confident in correctness and containment)

## Reviewer Protocol

### Blind Review Pass

Reviewer does NOT know which mode produced the result.

Reviewer receives:
- Task packet
- Resulting diff
- Changed files
- Test results

Important constraint:

- The blind pass is **diff-only for both modes**. No intent artifacts, evidence bundles, or mode-specific metadata are shown here.
- This pass measures merge/reject accuracy and baseline review confidence without treatment leakage.

Reviewer answers:
1. Would you merge this change? (yes / yes-with-conditions / no)
2. Confidence in correctness (0-2)
3. Confidence in containment (0-2)
4. Time to reach decision (minutes)
5. What information reduced uncertainty? What increased it?

After blind pass, reveal mode and compare decisions.

### Artifact Utility Pass

Non-blind, focused on whether the artifact stack changes the review:

Reviewer now receives the full artifact bundle for controlled runs:

- `intent.md`
- `glossary.md`
- `invariants.md`
- `scope.yml`
- `evidence.md`
- `review.md`

Reviewer answers:
1. Which artifact changed your decision, if any?
2. Which artifact reduced uncertainty the most?
3. Which artifact felt redundant or noisy?
4. Would you want this artifact stack in your normal workflow?

### Quality Review Pass

Non-blind, focused on quality:
1. Does this change match the evaluation checklist?
2. Are there latent bugs not caught by tests?
3. Are there scope violations the automated check missed?
4. Would the behavior evidence have changed your decision?

### Ground Truth

For each task, establish ground truth from:
- The actual merged PR (if sourced from closed issues)
- Manual expert review of both outputs
- Test suite + evaluation checklist

This enables measuring **review accuracy** (did the reviewer make the correct merge/reject decision), not just review time.

## Automation

### Benchmark Runner

```
specpunk-bench/
  config/
    repos.yml              # repo list + commit SHAs
    tasks/
      yt-dlp-001.yml       # task packets
      fastapi-001.yml
  runners/
    baseline.sh            # Mode A runner
    controlled.sh          # Mode B runner
  collectors/
    metrics.py             # extract metrics from run artifacts
    cost.py                # extract token usage and costs
  results/
    raw/                   # per-run artifacts
    summary.csv            # aggregated metrics table
    report.md              # generated comparison report
```

Minimum automation for v1:

```bash
# Run one task in both modes
specpunk-bench run yt-dlp-001 --tool claude-code

# Collect metrics from completed runs
specpunk-bench collect results/raw/ --output results/summary.csv

# Generate comparison report
specpunk-bench report results/summary.csv --output results/report.md
```

### CI Integration

After initial manual runs, automate nightly on a subset:

```yaml
# .github/workflows/benchmark.yml
on:
  schedule:
    - cron: '0 3 * * 1'  # weekly Monday 3am
jobs:
  benchmark:
    strategy:
      matrix:
        task: [yt-dlp-001, fastapi-001, httpx-001]
        mode: [baseline, controlled]
```

## Success Criteria

These are initial working thresholds, not validated constants.

The intent-control layer is promising if, across tasks:

- Out-of-scope edits decrease by roughly >50% (Mode B vs Mode A)
- Reviewer confidence improves by roughly 1 point on average
- Blind review accuracy improves (fewer incorrect merge/reject decisions)
- Correctness is at least as good as baseline
- Artifact prep overhead stays under roughly 40% of total task time
- Token cost increase stays under roughly 50%

Practical threshold for continuing:

- Meaningful improvement on scope adherence and reviewer confidence
- No catastrophic slowdown
- Reviewers consistently say the added artifacts help more than they hurt
- Cost increase is justified by quality improvement

## Failure Criteria

These are working stop conditions for the first benchmark iteration.

Stop or redesign if:

- Artifact prep takes longer than the task itself (>100% overhead)
- Reviewers still rely only on raw diff despite having intent artifacts
- Scope checks are >30% false positives
- Terminology/invariant checks do not change any review decisions
- The layer helps only on toy tasks (S-level) but not M or L
- Token cost increase exceeds 3x with no measurable quality gain

## Phased Rollout

### Phase 1: Scope Only (weeks 1-2)

Run benchmark with scope enforcement only (no glossary, no invariants, no evidence).
This isolates the simplest, hardest-signal check.

Measure: out-of-scope edit rate reduction, false positive rate.

### Phase 2: Scope + Intent Draft (weeks 3-4)

Add auto-drafted intent.md. Measure: does intent context change agent behavior?
Does reviewer use intent.md? Does it change decisions?

### Phase 3: Full Stack (weeks 5-6)

Add glossary + invariants + behavior evidence.
Full A/B comparison on all metrics.

### Phase 4: Cross-Tool (weeks 7-8)

Run same tasks with different tools (Claude Code vs Codex vs Cursor).
Measure: is the value tool-dependent or tool-agnostic?

## Output Format

Store benchmark results in structured YAML per run + aggregated CSV:

### Per-Run YAML

```yaml
run:
  id: "run-20260315-001"
  task: "yt-dlp-001"
  mode: "controlled"
  tool: "claude-code"
  model: "claude-sonnet-4-6"
  timestamp: "2026-03-15T14:30:00Z"

timing:
  artifact_prep_minutes: 7.0
  task_minutes: 11.2
  check_seconds: 3.4
  total_minutes: 18.6

cost:
  tokens_input: 62000
  tokens_output: 15000
  api_cost_usd: 0.24

files:
  changed_total: 3
  changed_in_scope: 3
  changed_out_of_scope: 0
  tests_added: 3
  tests_changed: 0

scores:
  correctness: 2
  scope_adherence: 2
  reviewability: 2
  confidence: 2

review:
  blind_decision: "merge"
  blind_time_minutes: 4.5
  ground_truth: "correct-merge"
  review_accurate: true

checks:
  scope_violations: 0
  terminology_warnings: 1
  terminology_true_positives: 1
  terminology_false_positives: 0
  invariant_violations: 0

notes: "Clean run. Terminology check caught 'subtitle' vs glossary term 'caption'."
```

### Aggregated CSV

```
repo,task_id,difficulty,mode,tool,correctness,scope,reviewability,confidence,review_accurate,task_min,prep_min,cost_usd,out_of_scope_files,term_tp,term_fp
yt-dlp,001,M,baseline,claude-code,2,1,1,1,false,8.5,0,0.18,2,0,0
yt-dlp,001,M,controlled,claude-code,2,2,2,2,true,11.2,7.0,0.24,0,1,0
```
