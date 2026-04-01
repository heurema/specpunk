# punk Eval

## Summary

`punk-eval` is the **evidence-based ratchet layer**.

It evaluates:
- concrete task outcomes
- candidate skill patches against baselines

And it writes:
- skill promotion / reject / rollback decisions

Core rule:
- `gate` decides whether a run is acceptable now
- `eval` decides whether the system got better over time

v1 covers:
- task eval
- skill eval
- promotion decisions

It does not cover in v1:
- online adaptation during a run
- auto-promotion without review
- full benchmark platform
- broad council-quality benchmarking

---

## Two eval contours

### Task eval
Question:
> Was this конкретный run/task actually good?

Inputs:
- approved contract
- receipt
- decision object
- optional proofpack and suite

Output:
- `TaskEvalRecord`

Required metrics:
- `ContractSatisfaction`
- `ScopeDiscipline`
- `TargetPassRate`
- `IntegrityPassRate`
- `CleanupCompletion`
- `DocsParity`
- `DriftPenalty`
- `GateOutcome`

Task eval must not just mirror gate. It must also score cleanup and project-coherence quality.

### Skill eval
Question:
> Did this candidate skill patch make the agent better for this project/role?

Inputs:
- candidate patch
- active baseline layers
- one or more eval suites
- target role and project

Outputs:
- `SkillEvalRecord`
- `PromotionDecision`

Skill eval is always:
- baseline vs candidate
- on the same suite
- with the same cases
- with the same scoring logic

One successful task is never enough for promotion.

---

## Eval artifacts

### `EvalSuite`
Collection of weighted cases for one purpose.

Must include:
- kind (`task | skill`)
- optional project and role
- case refs
- metric weights
- safety metrics
- primary metrics
- status

### `EvalCase`
One replay/fixture/incident case.

Kinds:
- `ReplayRun`
- `SupersededPair`
- `Incident`
- `Fixture`

### `EvalRun`
One execution of a suite over a target.

Targets:
- run id
- candidate patch id

### `TaskEvalRecord`
Aggregated evaluation of one completed run.

### `SkillEvalRecord`
Comparison between active baseline and candidate patch.

### `PromotionDecision`
Final ratchet decision:
- `promote`
- `reject`
- `rollback`

---

## Scoring and promotion rules

### Metric groups
#### Safety metrics
These must not regress:
- `ScopeDiscipline`
- `IntegrityPassRate`
- `CleanupCompletion`
- `DocsParity`
- `DriftPenalty`

#### Primary metrics
These should improve or stay neutral:
- `ContractSatisfaction`
- `TargetPassRate`
- `BlockedRunRate`
- `EscalationRate`

### Promotion rule
A candidate patch may be promoted only if:
1. no safety regression
2. at least one primary metric improves
3. no large negative delta on other primary metrics
4. suite coverage is sufficient

### Default thresholds
- safety regression tolerance: `0`
- minimum primary improvement: `>= 5% relative` or equivalent weighted delta
- maximum tolerated negative delta elsewhere: `< 3%`
- minimum suite size: at least `5` weighted cases
- at least `1` non-fixture case if such evidence exists

### Decision mapping
- `promote` — meaningful improvement, no safety regression
- `reject` — regression or no meaningful gain
- `rollback` — previously promoted patch later causes regressions/incidents

---

## Storage and events

### Repo-local layout
```text
.punk/
  eval/
    suites/
    runs/
    results/
    decisions/
```

### Event kinds
- `eval.suite_defined`
- `eval.run_started`
- `eval.case_completed`
- `eval.completed`
- `eval.decision_written`
- `skill.patch_promoted`
- `skill.patch_rejected`
- `skill.patch_rolled_back`

---

## Integration

### With `punk-skills`
Consumes:
- active layers
- candidate patches
- packet history

Does not own composition or activation mechanics.

### With `punk-orch`
May be triggered after important runs to create:
- `TaskEvalRecord`
- evidence for candidate skill patches

### With `punk-gate`
`gate` and `eval` are different:
- `gate`: should this run be accepted?
- `eval`: did this system change improve future work?

### With `punk-research`
Research may propose:
- candidate skill patches
- eval suite patches
- hypotheses about failures

But research never writes promotion decisions.

---

## Implementation defaults

v1 defaults:
- eval is offline/post-run
- project-local evidence is preferred
- task eval and skill eval remain separate forever
- promotion is deterministic and conservative

---

## Test scenarios

1. task eval scores a strong successful run correctly
2. task eval penalizes hidden drift like stale docs or leftover v1 paths
3. candidate skill patch promotes only with no safety regression
4. candidate patch is rejected if cleanup/docs discipline worsens
5. promoted patch can later be rolled back after incidents or regressions
