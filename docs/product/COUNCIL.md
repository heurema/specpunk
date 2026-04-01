# punk Council

## Summary

`punk-council` is a **selective advisory protocol engine** for high-stakes work.

It does not make final ship/block decisions. It produces structured deliberation artifacts that feed `plot` and `gate`.

Core rule:
- `council` is advisory only
- only `gate` writes final `DecisionObject`

v1 covers:
- architecture council
- contract council
- review council

v1 does not cover:
- migration/cleanup council
- implementation diverge
- freeform multi-agent chat
- final acceptance decisions

---

## Public model

### Protocol families
- `Architecture`
- `Contract`
- `Review`

### Outcomes
Every council run ends with one advisory outcome:
- `leader`
- `hybrid`
- `escalate`

### Core artifacts

#### `CouncilPacket`
Frozen input shared across all participants.

Must include:
- council kind
- subject ref (`feature_id`, `contract_id`, or `run_id`)
- project id
- repo snapshot ref
- optional contract/receipt refs
- prompt/question
- constraints
- rubric
- role assignments
- budget

#### `CouncilProposal`
Independent response from one role×model slot.

Must include:
- proposal label (`A`, `B`, `C`)
- summary
- risks
- cleanup obligations
- confidence estimate
- content ref

#### `CouncilReview`
Blind comparative review over anonymized proposals.

Must include:
- reviewer slot id
- proposal label
- criterion scores
- findings
- blockers
- confidence estimate

#### `CouncilSynthesis`
Final advisory synthesis.

Must include:
- outcome (`leader | hybrid | escalate`)
- selected labels
- rationale
- `must_keep[]`
- `must_fix[]`
- `unresolved_risks[]`
- confidence estimate

#### `CouncilRecord`
Stored umbrella artifact for one completed council run.

Must include refs to:
- packet
- proposals
- reviews
- synthesis
- scoreboard

---

## Protocol invariants

Every council protocol must do this in order:

1. freeze packet
2. independent generation
3. anonymize proposals
4. blind comparative review
5. deterministic scoring
6. synthesis
7. persist advisory artifacts

Hard rules:
- all proposal slots get the same frozen packet
- reviewers do not see model identity
- score totals are computed by code, not by LLM prose
- no protocol may write final verdicts

---

## Per-family behavior

### Architecture council
Used in `plot` for risky subsystem design or large refactors.

Proposal must cover:
- architecture summary
- touched modules/components
- tradeoffs
- migration plan
- cleanup obligations
- docs/config impacts
- risks
- reversibility

### Contract council
Used in `plot` before approving an expensive or risky contract.

Proposal must cover:
- missing obligations
- weak checks
- hidden docs/config/update surfaces
- cleanup/replacement obligations
- approve-readiness assessment

### Review council
Used in `gate` for risky runs or ambiguous review situations.

Proposal must cover:
- findings
- blockers
- warnings
- cleanup misses
- docs/config parity concerns
- confidence

---

## Scoring and selection

### Default rubric weights

#### Architecture council
- correctness/completeness: `0.30`
- scope safety: `0.20`
- migration realism: `0.15`
- cleanup coverage: `0.15`
- operational simplicity: `0.10`
- reversibility: `0.10`

#### Contract council
- explicitness: `0.25`
- scope boundedness: `0.20`
- interface clarity: `0.20`
- check quality: `0.20`
- cleanup/docs obligations: `0.15`

#### Review council
- issue quality: `0.30`
- correctness of concerns: `0.25`
- severity calibration: `0.20`
- coverage: `0.15`
- actionability: `0.10`

### Deterministic selection rules
Choose `leader` if:
- top score >= `75`
- gap to second >= `8`
- no unresolved critical blocker
- disagreement is not too high

Choose `hybrid` if:
- top two scores are within `8`
- strengths are complementary
- no critical contradiction blocks synthesis

Choose `escalate` if:
- any critical blocker remains unresolved
- disagreement is high
- top score < `75`
- synthesis would require unsupported assumptions

### Confidence
Council confidence is advisory only.

It is derived from:
- top normalized score
- reviewer agreement
- blocker severity
- top-vs-second gap

It is not a final truth score.

---

## Integration

### With `plot`
`plot` may use council for:
- architecture exploration
- contract hardening

Council outputs may produce:
- recommended leader
- hybrid synthesis
- `must_fix[]` for contract changes

### With `gate`
`gate` may consume:
- council findings
- council synthesis
- council confidence

But `gate` still alone writes:
- `DecisionObject`
- `Proofpack`

---

## Storage and events

### Repo-local layout
```text
.punk/
  council/
    <council-id>/
      packet.json
      proposals/
      reviews/
      synthesis.json
      record.json
```

### Event kinds
- `council.started`
- `council.proposal_written`
- `council.review_written`
- `council.synthesis_written`
- `council.completed`

---

## Implementation defaults

v1 defaults:
- exactly `3` generator slots
- exactly `2` blind reviewer slots
- supported families: architecture, contract, review
- model identity hidden from reviewers, retained in audit metadata
- no freeform inter-model discussion between proposal slots

---

## Test scenarios

1. anonymization removes model identity from reviewer inputs
2. score totals are deterministic for the same reviews
3. leader selection works when thresholds are met
4. hybrid selection works for close complementary proposals
5. critical blocker forces `escalate`
6. gate integration keeps final decision ownership in `gate`
