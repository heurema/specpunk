# punk Research

## Summary

Target-shape `punk-research` is a **bounded deep-research subsystem** for hard, ambiguous, or high-risk engineering questions.

This doc covers both:
- the target-shape `punk-research` crate
- the already-implemented bounded research capability in the active CLI/orch/domain surface

Current status:
- canonical current-truth matrix: `docs/product/IMPLEMENTATION-STATUS.md`
- crate status: dedicated `punk-research` crate is **planned only**
- capability status: bounded `punk research ...` commands already exist today in `punk-cli` + `punk-orch` + `punk-domain`
- operator-surface status: bounded research is a current **expert/control surface**, not the default operator path

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

Current active v0 capability:
- `punk research start` freezes a repo-local `ResearchQuestion`, `ResearchPacket`, and `ResearchRecord`
- `punk research artifact <research-id> ...` appends structured repo-local `ResearchArtifact` records
- `punk research synthesize <research-id> ...` writes one structured repo-local `ResearchSynthesis`
- `punk research complete <research-id>` and `punk research escalate <research-id>` apply terminal operator-triggered stop states after synthesis
- `punk inspect research_<id>` reads that frozen packet bundle back
- `punk inspect research_<id> --json` may also carry a derived invalidation projection (`active`, `latest`, `history_count`) for downstream tooling
- `punk inspect research_<id> --json` may also carry a derived synthesis-lineage projection (`active`, `latest`, `history_count`, `history[]`, `has_active_current_view`, `has_replacements`, `latest_is_active`) built from immutable synthesis history for downstream tooling
- worker orchestration and critique loops are still later-stage work
- this does **not** mean the dedicated `punk-research` crate already exists

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

Current v0 shape:
- `id`
- `research_id`
- `kind`
- `summary`
- optional `source_ref`
- `created_at`

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

Current v0 shape:
- `id`
- `research_id`
- `outcome`
- `summary`
- `artifact_refs[]`
- optional `supersedes_ref`
- `follow_up_refs[]`
- `created_at`

### `ResearchRecord`
Stored execution record for one research run.

Current v0 shape includes:
- `artifact_refs[]`
- optional mutable-current `synthesis_ref`
- immutable `synthesis_history_refs[]`
- optional `invalidated_synthesis_ref`
- optional `invalidation_artifact_ref`
- `invalidation_history[]`
- optional `outcome`

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

### With later `punk-council` activation
Research may prepare better council packets, but does not replace council.

### With later `punk-skills` activation
Research may generate candidate skill patches or overlay proposals, but they remain candidates.

### With later `punk-eval` activation
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
      question.json
      packet.json
      record.json
      artifacts/
      synthesis.json
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

Current v0 start/freeze slice:
- `research.started` is implemented
- the stored `record.json` state is `frozen`
- `packet.json` carries an explicit budget even when defaults are used
- `output_schema_ref` currently points at this doc (`docs/product/RESEARCH.md#researchsynthesis`)
- the current slice does **not** execute workers or write `synthesis.json`

Current v0 artifact slice:
- `research.artifact_written` is now implemented
- `record.json` appends `artifact_refs[]` and moves to `state = gathering`
- if artifact writing arrives after a previously synthesized current view, the mutable `.punk/research/<research-id>/synthesis.json` alias is removed so it cannot remain stale
- that same invalidation may persist minimal metadata explaining which immutable synthesis was cleared and which artifact caused the invalidation
- historical invalidation entries are retained across later re-synthesis cycles for inspectability
- artifact writing is still operator-triggered and repo-local only
- artifact writing does **not** imply synthesis, completion, or promotion

Current v0 synthesis slice:
- `research.synthesis_written` is now implemented
- `punk research synthesize <research-id> ...` writes `.punk/research/<research-id>/synthesis.json`
- each synthesis write also persists an immutable identity copy under `.punk/research/<research-id>/syntheses/<synthesis-id>.json`
- synthesis writing requires at least one persisted artifact and defaults to linking all current `artifact_refs[]` when no explicit subset is given
- synthesis may also persist explicit repo-local `follow_up_refs[]` for the next bounded operator action after research stops
- repeating `punk research synthesize <research-id> ...` now requires explicit replace intent
- replacement is allowed only before terminal `completed` / `escalated` states
- `record.json` stores the mutable current-view `synthesis_ref`, appends immutable `synthesis_history_refs[]`, sets `outcome`, and moves to `state = synthesized`
- writing a new synthesis clears any prior current-view invalidation note on the record
- synthesis writing is still operator-triggered and repo-local only
- synthesis writing does **not** imply worker execution, `research.completed`, or promotion

Current v0 terminal-state slice:
- `research.completed` is now implemented as an operator-triggered terminal transition from `state = synthesized`
- `research.escalated` is now implemented as an operator-triggered terminal transition from `state = synthesized`
- `punk research complete <research-id>` requires a persisted synthesis whose `outcome != escalate`
- `punk research escalate <research-id>` requires a persisted synthesis whose `outcome == escalate`
- after either terminal transition, further artifact/synthesis mutations are rejected
- terminal transitions do **not** add worker execution, council routing, or promotion semantics
- terminal human summaries should show `follow_up_refs[]` and shift the obvious next step from mutation to follow-up review when those refs exist
- terminal or synthesized human summaries may also show the current immutable synthesis identity and any replacement lineage

---

## Test scenarios

1. architecture research produces ADR draft under fixed budget
2. migration risk research surfaces hidden compatibility concerns
3. skill improvement research creates candidate patch, not promotion
4. eval suite patch research proposes missing regression coverage
5. unresolved ambiguity leads to `escalate`, not endless looping
