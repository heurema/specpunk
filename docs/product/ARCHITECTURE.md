# punk Architecture

This document describes the target implementation shape for the clean rebuild.

It replaces the old `punk-run`-centric architecture.

---

## 1. Current stance

The repo has not launched.

So architecture is optimized for:

- clean target shape
- no backward compatibility
- minimal viable end-to-end slice first
- small stable kernel with replaceable edges

Legacy code under `punk/` is source material for extraction, not the target structure.

---

## 2. Architectural north star

`punk` is a **stewarded multi-agent engineering runtime**.

Its target architecture has four pillars:

1. **Kernel** — stable Rust core, small and durable
2. **Stewardship** — feature lifecycle, cleanup, docs, no drift
3. **Council** — selective structured multi-model deliberation
4. **Skill/Eval ratchet** — project-specific skills improved through evidence and promotion

The critical rule is:

> Keep the kernel stable. Let model integrations, skills, councils, and research protocols evolve at the edges.

See also:

- `docs/product/NORTH-ROADMAP.md`
- `docs/research/2026-04-03-specpunk-identity-and-layering.md`
- `docs/research/2026-04-03-specpunk-primitives-and-derived-mechanisms.md`

### Core vs shell split

`punk` is intentionally split into two layers:

#### Layer A — correctness substrate

The substrate owns durable truth and safety invariants:

- `Project` identity
- normalized `Goal` intake record
- `Contract`
- `Scope`
- isolated `Workspace`
- `Run`
- `DecisionObject`
- `Proofpack`
- evented `Ledger`
- VCS isolation
- artifact guarantees
- failure semantics

This layer must stay:

- deterministic where possible
- explicit about invariants
- inspectable through structured artifacts
- resistant to prompt drift

#### Layer B — operator shell

The shell owns operator ergonomics and default paths:

- `punk init`
- `punk start`
- `punk go --fallback-staged`
- summary formatting
- blocked / recovery UX
- generated `AGENTS.md`
- repo-local bootstrap guidance
- shell-facing status output

The shell may simplify interaction, but it must not become a second source of truth.

### Boundary rules

Hard rules:

1. The shell may compose substrate operations, but must not bypass substrate invariants.
2. Safety-critical semantics live in code and persisted artifacts, not only in prompts.
3. `gate` truth must not be downgraded into shell-only chatter.
4. If shell behavior changes operator expectations, update `README.md`, `docs/product/CLI.md`, and `docs/product/NORTH-ROADMAP.md` together.
5. If substrate invariants change, update this file and the linked research notes in the same diff.

### Primitive vs derived rule

If something can be recomposed from deeper objects without changing truth, it is not a primitive.

Current candidate primitives are:

| Primitive | Why it is primitive |
|---|---|
| `Project` | identity and repo boundary |
| `Goal` | normalized user intent anchor |
| `Contract` | executable bounded spec |
| `Scope` | safety boundary for execution |
| `Workspace` | isolated mutation context |
| `Run` | one execution attempt |
| `DecisionObject` | final verification verdict |
| `Proofpack` | immutable verification artifact |
| `Ledger` | durable evented truth and projections |

Derived mechanisms are compositions over those primitives:

- `init`
- `start`
- `go`
- staged fallback
- `status`
- `inspect`
- project overlays
- council
- eval
- research protocols

### Derived mechanism map

The first practical mapping should stay explicit:

| Mechanism | Type | Primary primitives touched |
|---|---|---|
| `init` | shell bootstrap | `Project`, `Ledger` |
| `start` | staged shell intake | `Goal`, `Contract`, `Scope` |
| `go` | autonomous shell intake | `Goal`, `Contract`, `Run`, `DecisionObject`, `Proofpack`, `Ledger` |
| staged fallback | shell recovery | `Contract`, `DecisionObject`, `Ledger` |
| `plot` | substrate permission boundary | `Contract`, `Scope` |
| `cut` | substrate permission boundary | `Workspace`, `Run` |
| `gate` | substrate permission boundary | `DecisionObject`, `Proofpack` |
| `status` | shell read surface | `Ledger` projections |
| `inspect` | shell read surface | canonical artifacts + `Ledger` projections |
| project overlays | derived project-intelligence mechanism | `Project`, `Ledger` |
| `council` | derived advisory subsystem | packets + artifacts |
| `eval` | derived ratchet subsystem | artifacts + baselines |
| `research` | derived bounded inquiry subsystem | packets + artifacts + synthesis |

Contributor rule:

> every roadmap item, proposal, or implementation slice should be able to answer: which primitive does this touch, and is this a primitive change or a derived mechanism change?

---

## 3. First vertical slice

The first working slice is:

```text
plot contract -> approve -> cut run -> gate run -> proof
```

This is intentionally:

- single-repo
- local-first
- deterministic
- council-free
- daemon-free
- skill-ratchet-free

The first objective is to make this loop real before reintroducing orchestration depth.

---

## 4. Target workspace

```text
specpunk/
├── Cargo.toml
├── crates/
│   ├── punk-cli/
│   ├── punk-shell/
│   ├── punk-domain/
│   ├── punk-events/
│   ├── punk-vcs/
│   ├── punk-core/
│   ├── punk-orch/
│   ├── punk-gate/
│   ├── punk-proof/
│   ├── punk-adapters/
│   ├── punk-council/
│   ├── punk-skills/
│   ├── punk-eval/
│   └── punk-research/
├── docs/
└── .punk/
```

This is the **target** workspace shape, not the current implementation snapshot.

Current v0/v0.1 implemented crates:

- `punk-cli`
- `punk-domain`
- `punk-events`
- `punk-vcs`
- `punk-core`
- `punk-orch`
- `punk-gate`
- `punk-proof`
- `punk-adapters`

Planned crates for Stage 1+:

- `punk-shell`
- `punk-council`
- `punk-skills`
- `punk-eval`
- `punk-research`

### Crate ownership

| Crate | Status | Owns |
|---|---|---|
| `punk-cli` | implemented | non-interactive command entrypoint |
| `punk-shell` | Stage 1+ | interactive REPL, mode switching, context routing |
| `punk-domain` | implemented | canonical types and schemas |
| `punk-events` | implemented | append-only event log and projections |
| `punk-vcs` | implemented | `jj` / `git` backend abstraction |
| `punk-core` | implemented | deterministic repo helpers, scan, scope, validation |
| `punk-orch` | implemented | feature/contract/task/run lifecycle services |
| `punk-gate` | implemented | validation, checks, decision synthesis |
| `punk-proof` | implemented | proof bundle writing and hashing |
| `punk-adapters` | implemented | external drafting/execution/review adapters |
| `punk-council` | Stage 2+ | structured deliberation protocols |
| `punk-skills` | Stage 4+ | skill registry, overlays, candidate patches |
| `punk-eval` | Stage 4+ | task eval, skill eval, promotion evidence |
| `punk-research` | Stage 5+ | bounded deep-research protocols |

Traits are only required where real backend choice exists.

Required ports over time:

- `VcsBackend`
- `Executor`
- `ContractDrafter`
- `CouncilProtocol`
- `SkillProvider`
- `EvalRunner`
- `PromotionPolicy`

Companion subsystem specs:
- `docs/product/COUNCIL.md`
- `docs/product/SKILLS.md`
- `docs/product/EVAL.md`
- `docs/product/RESEARCH.md`
- `docs/product/DOGFOODING.md`

---

## 5. Domain model

Canonical chain:

```text
Project
  -> Goal
    -> Feature
      -> Contract
        -> Task
          -> Run
            -> Receipt
            -> DecisionObject
            -> Proofpack
```

### Primitive ownership inside the chain

Not every object in the chain is equally primitive.

| Object | Role |
|---|---|
| `Project` | primitive |
| `Goal` | primitive |
| `Feature` | durable workstream grouping; important, but can evolve as ledger design sharpens |
| `Contract` | primitive |
| `Task` | orchestration unit derived from approved work planning |
| `Run` | primitive |
| `Receipt` | execution truth artifact attached to a run |
| `DecisionObject` | primitive |
| `Proofpack` | primitive |

### Important meanings

- `Feature` = enduring feature/workstream
- `Contract` = versioned executable spec
- `Task` = queueable orchestration unit
- `Run` = one execution attempt
- `Receipt` = execution truth
- `DecisionObject` = final gate verdict
- `Proofpack` = hash-linked artifact bundle

### Contract requirements

Every `Contract` must carry:

- `entry_points`
- `import_paths`
- `expected_interfaces`
- `behavior_requirements`
- `allowed_scope`
- `target_checks`
- `integrity_checks`

Stewardship-oriented contracts should also be able to express:

- cleanup obligations
- documentation obligations
- replacement/removal obligations
- migration-sensitive surfaces

That keeps the system feature-centric and execution-based instead of PR-centric and vague.

`Goal` remains part of the long-term canonical chain, but it is intentionally **deferred from the v0 domain/runtime** until the orchestration-depth stage.

---

## 6. Kernel semantics

### Canonical artifact ownership

The runtime kernel is intentionally small.

- `plot` creates and mutates only:
  - `Feature`
  - `Contract`
- `cut` creates and mutates only:
  - `Task`
  - `Run`
  - `Receipt`
- `gate` creates and mutates only:
  - `DecisionObject`
  - `Proofpack`

Everything else is derived:

- `status`
- `morning`
- budget snapshots
- proof summaries
- cross-project views
- council summaries
- skill leaderboards

These are views over canonical artifacts and the event log, not primary mutable state objects.

### State transitions

#### `ContractStatus`

- `Draft -> Approved`
- `Draft -> Cancelled`
- `Approved -> Superseded`
- `Approved -> Cancelled`

#### `TaskStatus`

- `Queued -> Claimed`
- `Claimed -> Running`
- `Running -> Done`
- `Running -> Failed`
- `Queued|Claimed|Running -> Cancelled`

#### `RunStatus`

- `Running -> Finished`
- `Running -> Failed`
- `Running -> Cancelled`

#### `Decision`

v0 final outcomes are:

- `Accept`
- `Block`
- `Escalate`

`Waive` is intentionally **not** part of v0.

If an operator needs to bypass a strict gate in v0, that must happen through:

- `Escalate`
- explicit operator note

not through a separate verdict kind.

### Frozen inputs

`gate` always evaluates frozen persisted inputs, not evolving operator intent.

That means:

- `gate` reads the persisted approved `Contract`
- `gate` reads the persisted `Receipt`
- `gate` may read persisted check outputs
- `gate` must not rely on live prompt reinterpretation

`Proofpack` must point to immutable refs and hashes of:

- approved contract
- receipt
- decision object
- check outputs

---

## 7. Runtime capability policy

| Mode | Allowed | Forbidden |
|---|---|---|
| `plot` | repo read, VCS read, deterministic repo scan, contract draft/refine writes, local planning artifact writes under `.punk/` | source mutation, patch apply, broad mutating execution, final decision writing |
| `cut` | isolated change creation, source mutation within approved scope, executor invocation, test/build/check execution, receipt writing | final decision writing, proof writing |
| `gate` | artifact read, scope validation, deterministic target/integrity checks, decision writing, proof writing | broad source mutation, unrestricted repair edits |

Mode enforcement must live in runtime behavior, not only in prompts.

---

## 8. Event log as SSoT

Runtime truth lives in an append-only event log plus derived views.

Heavy artifacts do **not** live inline in the log.

The log stores:

- object IDs
- event kind
- timestamp
- refs to artifacts
- hashes of artifacts

### Event envelope

```json
{
  "event_id": "evt_01",
  "ts": "2026-03-29T10:00:00Z",
  "project_id": "signum",
  "feature_id": "feat_01",
  "task_id": "task_01",
  "run_id": "run_01",
  "actor": "operator",
  "mode": "cut",
  "kind": "run.started",
  "payload_ref": ".punk/runs/run_01/meta.json",
  "payload_sha256": "..."
}
```

### Minimum event kinds

- `feature.created`
- `feature.updated`
- `contract.drafted`
- `contract.refined`
- `contract.approved`
- `task.queued`
- `task.claimed`
- `run.started`
- `receipt.written`
- `run.finished`
- `decision.written`
- `proofpack.written`

Later subsystems may add:

- council events
- skill candidate events
- eval result events
- promotion decision events
- research packet events

but they should extend, not replace, the evented core.

### `WorkLedgerView`

The event log is the source of truth, but operators and agents should not have to reconstruct work continuity manually from raw events and multiple artifact types.

The intended answer is a materialized projection:

```text
WorkLedgerView
  project_id
  work_id
  goal_ref
  feature_ref
  active_contract_ref
  latest_run_ref
  latest_receipt_ref
  latest_decision_ref
  latest_proof_ref
  lifecycle_state
  blocked_reason
  next_action
  next_action_ref
  updated_at
```

### Why this view exists

It answers the three operator questions that raw artifacts answer poorly:

1. what is the current state of this work item?
2. what artifact is currently authoritative?
3. what should happen next?

### Important rules

- `WorkLedgerView` is a **derived projection**, not a second mutable truth object
- append-only events remain canonical truth
- contracts, runs, decisions, and proofs remain canonical artifacts
- shell surfaces should prefer this projection over ad hoc status inference

### Initial lifecycle projection

The first useful `lifecycle_state` set is:

- `drafting`
- `awaiting_approval`
- `ready_to_run`
- `running`
- `awaiting_gate`
- `accepted`
- `blocked`
- `escalated`
- `superseded`
- `cancelled`

These are projection states for operators and shells, not replacements for lower-level object status enums.

### Durable autonomous outcomes

`go --fallback-staged` should eventually rest on durable state, not shell text alone.

The intended durable shape is an autonomy-linked record such as:

```text
AutonomyRecord
  work_id
  goal_ref
  contract_ref
  run_ref
  decision_ref
  proof_ref
  autonomy_outcome
  basis_summary
  recovery_contract_ref
  next_action
  next_action_ref
  recorded_at
```

### Why this matters

Without a durable autonomy-linked record, the shell may report:

- `blocked`
- `escalated`
- prepared staged recovery

but later inspection still depends on remembering shell output instead of inspecting durable state.

### Projection rule

`AutonomyRecord` should feed `WorkLedgerView`, not compete with it.

In practice that means:

- `autonomy_outcome` helps determine `lifecycle_state`
- `basis_summary` helps explain `blocked_reason`
- `recovery_contract_ref` and `next_action_ref` feed durable recovery surfaces

### Recovery distinction

The architecture should distinguish:

- blocked with no durable recovery prepared
- blocked with a staged recovery contract already prepared

That distinction is important for both shell UX and later inspection.

---

## 9. Storage layout

### Global runtime state

```text
~/.punk/
  config.toml
  events/
  views/
```

Use this for:

- event log
- materialized views
- global config
- skill/eval metadata that is not repo-tracked

### Repo-local artifacts

```text
.punk/
  project.json
  contracts/
  runs/
  decisions/
  proofs/
```

Use this for:

- contracts
- receipts
- stdout/stderr artifacts
- decisions
- proof bundles
- project overlays when they are repo-specific

---

## 10. VCS substrate

`punk` is VCS-aware, not git-bound.

Policy:

- prefer `jj`
- fallback to `git`

### Required backend operations

```text
detect()
workspace_root()
create_isolated_change()
change_id()
changed_files()
diff()
cleanup()
```

### Why `jj` matters

The target workflow is change-centric:

- multiple runs per feature
- superseded attempts
- explicit lineage
- cleaner isolation for implementation runs

That matches `jj` better than branch-only thinking.

---

## 11. Stewardship layer

Stewardship is the part of the system that ensures the project ends in a coherent state, not merely that code was produced.

It must eventually reason about:

- bounded scope
- replacement lineage
- cleanup completion
- docs/config/manifests parity
- migration leftovers
- duplicate v1/v2 paths
- explicit removal obligations

### Stewardship rule

If a feature introduces `v2`, the system must either:

- remove `v1`, or
- record explicit retention and compatibility rationale

Silent duplication is not an acceptable default done-state.

---

## 12. Council vs gate

`council` and `gate` are intentionally different subsystems.

### `council`

`council` is advisory only.

It may produce:

- findings
- claims
- votes
- alternatives
- confidence estimates
- synthesis proposals

It must not write final verdicts.

### `gate`

`gate` is the only subsystem that writes the final `DecisionObject`.

It consumes:

- approved contract
- receipt
- deterministic check outputs
- optional council outputs

and turns them into:

- `Accept`
- `Block`
- `Escalate`

Future docs may still use `verify` as a `council` protocol name, but it must remain advisory and must not be treated as acceptance.

### Council protocol families

The intended families are:

- architecture council
- contract council
- review council
- migration/cleanup council
- implementation diverge
- research-backed synthesis

Council is selective, not always-on.

---

## 13. Skills and eval ratchet

### Skill architecture

Skills should be composed from layers:

1. base skill
2. domain skill
3. project overlay
4. task packet

A live agent should receive a composed skill packet, not a single monolithic markdown blob.

### Skill lifecycle

`punk` should use **curated ratchet**, not self-mutating live skills.

Expected flow:

```text
run history
-> failure mining
-> candidate skill patch
-> eval set
-> promotion decision
-> project overlay update
```

### Skill states

The system should support at least:

- active
- candidate
- rejected
- superseded
- rolled_back

### Eval contours

Two eval loops must remain distinct:

#### Task eval
Did this feature/run complete correctly?

#### Skill eval
Did this new skill or overlay improve agent behavior for this project?

Do not treat task success alone as proof that a skill patch should be promoted.

---

## 14. Research mode

Hard problems need bounded deep research, but not freeform endless loops.

Research must be a protocol with:

- frozen question or contract
- explicit budget
- bounded steps
- structured outputs
- evaluator or synthesis stage

Example uses:

- architecture research
- migration risk research
- cleanup impact research
- skill-improvement research
- model/protocol comparison research

This is the long-term role of `punk-research` and `delve`-style workflows.

---

## 15. Dogfooding and trust separation

`punk` should dogfood itself on real repo work, but under bounded self-hosting rules.

Ordinary feature work may use the normal `plot -> cut -> gate -> proof` path.
Meta-level changes to trust surfaces require stronger review and must not be silently self-certified.

Detailed policy lives in `docs/product/DOGFOODING.md`.

---

## 16. v0 invariants

These rules must hold in the first working version:

1. `cut run` must refuse non-approved contracts.
2. Every `Run` must record:
   - VCS backend
   - `change_ref`
3. Every approved `Contract` must have non-empty:
   - `allowed_scope`
   - `target_checks`
   - `integrity_checks`
4. Every `DecisionObject` must distinguish:
   - `target_status`
   - `integrity_status`
   - `confidence_estimate`
5. No code path outside `gate` may persist final decision artifacts.
6. The event log remains the SSoT for runtime state.

---

## 17. Service boundaries

### `punk-orch`

Owns:

- feature creation
- contract drafting and approval
- task creation
- run creation
- status loading

### `punk-gate`

Owns:

- scope validation
- policy validation
- target/integrity checks
- decision synthesis

### `punk-proof`

Owns:

- proofpack creation
- artifact hashing
- proof writing

### Future `punk-council`

Owns:

- proposal collection
- blind comparison
- rubric scoring
- synthesis protocol

### Future `punk-skills`

Owns:

- skill registry
- project overlays
- candidate skill patches
- skill composition packets

### Future `punk-eval`

Owns:

- task eval runners
- skill eval runners
- promotion evidence
- ratchet decisions
