# punk Master Plan

## Summary

`punk` is rebuilt as one local-first CLI with three canonical runtime modes:

- `plot`
- `cut`
- `gate`

The target product is a **stewarded multi-agent engineering runtime** with four pillars:

1. kernel
2. stewardship
3. council
4. skill/eval ratchet

Implementation sequence:

1. v0 core loop
2. shell
3. council / diverge
4. orchestration depth
5. skills + eval ratchet
6. deep research mode
7. benchmark/eval expansion

Kernel rules that stay fixed across all stages:

- `plot` owns `Feature` and `Contract`
- `cut` owns `Task`, `Run`, and `Receipt`
- `gate` owns `DecisionObject` and `Proofpack`
- event log is the runtime SSoT
- `status`, `morning`, proof summaries, and budget snapshots are derived views
- v0 gate outcomes are only:
  - `Accept`
  - `Block`
  - `Escalate`

`Waive` is deferred until a later policy/audit layer.

## v0 objective

Deliver this first working path:

```text
plot contract -> approve -> cut run -> gate run -> gate proof
```

v0 is:
- CLI-first
- one executor family only (`Codex CLI`)
- single-repo from current `cwd`
- `jj` preferred, `git` fallback
- no daemon, queue, goals UI, council, or benchmark subsystem
- strict gate against frozen approved contract + persisted receipt

## v0 operational rules

### Artifact ownership

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

### Required transitions

- `ContractStatus`
  - `Draft -> Approved`
  - `Draft -> Cancelled`
  - `Approved -> Superseded`
  - `Approved -> Cancelled`
- `TaskStatus`
  - `Queued -> Claimed -> Running -> Done|Failed|Cancelled`
- `RunStatus`
  - `Running -> Finished|Failed|Cancelled`

### Runtime capability policy

- `plot`
  - allow: repo read, VCS read, deterministic scan, contract draft/refine writes
  - deny: source mutation, patch apply, final decision writes
- `cut`
  - allow: isolated change creation, scoped source mutation, executor invocation, receipt writing
  - deny: final decision writes, proof writes
- `gate`
  - allow: scope validation, deterministic checks, decision writing, proof writing
  - deny by default: broad source mutation, unrestricted repair edits

### Frozen-input rule

`gate` must judge persisted artifacts, not live intent.
It always reads:

- approved contract
- receipt
- deterministic check outputs

and never a fresh reinterpretation of the task prompt.

### v0 invariants

1. `cut run` refuses non-approved contracts.
2. Approved contracts must have non-empty:
   - `allowed_scope`
   - `target_checks`
   - `integrity_checks`
3. Every `Run` records:
   - VCS backend
   - `change_ref`
4. Only `gate` writes final decision artifacts.

## v0 command surface

```bash
punk plot contract "<prompt>"
punk plot refine <contract-id> "<guidance>"
punk plot approve <contract-id>
punk cut run <contract-id>
punk gate run <run-id>
punk gate proof <run-id|decision-id>
punk status [id]
punk inspect <id> --json
```

## Post-v0 stages

### Stage 1 — thin shell
Add thin `punk` shell over the same services.

### Stage 2 — council / diverge
Detailed spec: `docs/product/COUNCIL.md`
Add `punk-council` with protocol families such as:
- architecture council
- contract council
- review council
- migration/cleanup council
- later `cut diverge`

Rules:
- `council` is advisory only
- `council` may emit findings, claims, votes, alternatives, confidence estimates, and synthesis proposals
- `council` must not write final verdicts
- `gate` remains the only writer of `DecisionObject`
- council is selective, not always-on

### Stage 3 — orchestration depth
Add `Goal`, project registry, queue, daemon, and higher-order orchestration.

### Stage 4 — skills + eval ratchet
Detailed specs:
- `docs/product/SKILLS.md`
- `docs/product/EVAL.md`
Add:
- `punk-skills`
- `punk-eval`
- project overlays
- candidate skill patches
- promotion/rollback decisions

Rules:
- skills evolve through curated ratchet, not silent mutation
- task eval and skill eval remain separate
- project-specific competence is first-class

### Stage 5 — deep research mode
Detailed spec: `docs/product/RESEARCH.md`
Add bounded research workflows (`delve`-style) for hard problems.

Rules:
- research runs under frozen questions/contracts
- explicit budget and stop rules
- structured outputs only
- evaluator/synthesis stage before reuse in skills or contracts

### Stage 6 — benchmark/eval expansion

### Cross-cutting rule — dogfooding
Use `punk` to build `punk` as early as practical, but keep bounded self-hosting rules in force.
Detailed policy: `docs/product/DOGFOODING.md`

Add contamination-aware, reproducible benchmarking once runtime, councils, and ratchet loops are stable.
