# punk Research

## Summary

`punk-research` is a **bounded deep-research subsystem** for hard, ambiguous, or high-risk engineering questions.

It is not endless autoresearch.
It is a controlled protocol with:
- frozen question
- explicit budget
- stop rules
- structured outputs

Core rule:
- research is preparatory and advisory
- research does not write `DecisionObject`
- research does not promote skills directly
- research must stop

v1 covers:
- architecture research
- migration risk research
- cleanup impact research
- skill improvement research
- model/protocol comparison research

---

## Core model

### `ResearchQuestion`
Frozen intent for a research run.

Must include:
- kind
- project id
- optional subject ref
- question
- goal
- constraints
- success criteria

### `ResearchBudget`
Must include:
- `max_rounds`
- `max_worker_slots`
- optional `max_cost_usd`
- `max_duration_minutes`
- `max_artifacts`

### `ResearchPacket`
Frozen execution packet.

Must include:
- question ref
- repo snapshot ref
- optional contract/receipt/skill/eval refs
- context refs
- budget
- stop rules
- output schema ref

### `ResearchArtifact`
One produced material.

Kinds may include:
- note
- hypothesis
- comparison
- critique
- synthesis input

### `ResearchSynthesis`
Final structured output.

Allowed outcomes:
- `answer`
- `candidate_patch`
- `contract_patch`
- `adr_draft`
- `risk_memo`
- `eval_suite_patch`
- `escalate`

### `ResearchRecord`
Stored execution record for one research run.

---

## Protocol model

### Shared invariants
Every research protocol must be:
- frozen
- bounded
- structured
- auditable
- non-authoritative

### Step model
Every v1 research run follows:
1. freeze packet
2. gather
3. critique / compare
4. synthesize
5. emit structured output
6. stop

No recursive endless loop in v1.

### Stop rules
Research stops when one of these triggers:
- `max_rounds` reached
- `max_worker_slots` exhausted
- `max_cost_usd` reached
- `max_duration_minutes` reached
- enough evidence for synthesis
- ambiguity remains -> `escalate`

### Default budgets
v1 defaults:
- `max_rounds = 3`
- `max_worker_slots = 5`
- `max_duration_minutes = 30`
- `max_artifacts = 12`
- `max_cost_usd = null` unless policy or operator sets it

---

## Protocol families

### Architecture research
Used before or alongside architecture council.

Outputs:
- `adr_draft`
- `risk_memo`
- `escalate`

### Migration risk research
Used before risky migrations/refactors.

Outputs:
- `risk_memo`
- `contract_patch`
- `escalate`

### Cleanup impact research
Used to map what must be removed or updated.

Outputs:
- `contract_patch`
- `risk_memo`
- `escalate`

### Skill improvement research
Used to turn repeated failures or incidents into better candidate skill patches or suite ideas.

Outputs:
- `candidate_patch`
- `eval_suite_patch`
- `escalate`

### Model/protocol comparison research
Used to compare model families or protocol settings on the same frozen basis.

Outputs:
- `risk_memo`
- `eval_suite_patch`
- `escalate`

---

## Integration

### With `plot`
Research may prepare:
- architecture recommendations
- contract patch proposals
- risk memos

### With `punk-council`
Research may prepare better council packets, but does not replace council.

### With `punk-skills`
Research may generate candidate skill patches or overlay proposals, but they remain candidates.

### With `punk-eval`
Research may generate eval-suite patch proposals or failure hypotheses, but does not write promotion decisions.

### With `punk-gate`
Research may be attached as advisory evidence, but cannot bypass gate.

---

## Storage and events

### Repo-local layout
```text
.punk/
  research/
    <research-id>/
      packet.json
      artifacts/
      synthesis.json
      record.json
```

### Event kinds
- `research.started`
- `research.artifact_written`
- `research.synthesis_written`
- `research.completed`
- `research.escalated`

---

## Implementation defaults

v1 defaults:
- research is operator-triggered or policy-triggered
- all research is bounded by explicit budget and stop rules
- all outputs are structured artifacts
- research informs council, skills, and eval but never becomes final project truth by itself

---

## Test scenarios

1. architecture research produces ADR draft under fixed budget
2. migration risk research surfaces hidden compatibility concerns
3. skill improvement research creates candidate patch, not promotion
4. eval suite patch research proposes missing regression coverage
5. unresolved ambiguity leads to `escalate`, not endless looping
