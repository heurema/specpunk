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

1. v0 core loop + current shell mechanisms
2. dedicated shell crate
3. council activation / diverge
4. orchestration depth + standalone `Goal` primitive
5. skills + eval ratchet
6. research crate extraction + deeper research execution
7. benchmark/eval expansion

Current bounded execution plan:

- `docs/product/ACTION-PLAN.md`
- supporting review memo: `docs/research/2026-04-11-specpunk-architecture-review.md`

Repo-status vocabulary for this plan:

- **active v0 surface** = current operator/runtime path
- **in-tree but inactive** = workspace member kept buildable before its stage is promoted
- **planned only** = target-shape crate not in today's workspace membership

Canonical terms: `docs/product/REPO-STATUS.md`
Canonical full matrix: `docs/product/IMPLEMENTATION-STATUS.md`

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
- native `punk init` bootstrap (`.punk/project.json`, `AGENTS.md`, `.punk/AGENT_START.md`, `.punk/bootstrap/<project>-core.md`)
- no daemon, queue, goals UI, council, or benchmark subsystem
- strict gate against frozen approved contract + persisted receipt
- current shell entrypoints already include `punk go --fallback-staged`, `punk start`, `punk status`, and `punk inspect`
- current bounded research commands already exist as an expert/control surface in the active CLI/orch/domain slice

Important distinction:

- `punk go --fallback-staged` is already real today as a shell mechanism
- the standalone `Goal` primitive is still later-stage work
- `punk research ...` is already real today as a bounded capability
- the separate `punk-research` crate is still later-stage work

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

Current operator/default shell surface:

```bash
punk init --enable-jj --verify
punk go --fallback-staged "<goal>"
punk start "<goal>"
punk status [id]
punk inspect project
punk inspect work [id]
punk inspect <id> --json
```

Current expert/control surface on the same active CLI:

```bash
punk plot contract "<prompt>"
punk plot refine <contract-id> "<guidance>"
punk plot approve <contract-id>
punk cut run <contract-id>
punk gate run <run-id>
punk gate proof <run-id|decision-id>
punk research start "<question>" --kind <kind> --goal "<goal>" --success "<criterion>"
punk research artifact <research-id> --kind note --summary "<summary>"
punk research synthesize <research-id> --outcome <outcome> --summary "<summary>"
punk research complete <research-id>
punk research escalate <research-id>
```

## Post-v0 stages

### Stage 1 — thin shell
Add a dedicated `punk-shell` crate over the same services.

Current note:

- the repo already has shell mechanisms in `punk-cli`
- this stage is about extracting the dedicated shell crate, not inventing the shell path from zero

### Stage 2 — council / diverge
Detailed spec: `docs/product/COUNCIL.md`
Add `punk-council` with protocol families such as:
- architecture council
- contract council
- review council

Deferred beyond current v1 council scope:
- migration/cleanup council
- any later `cut diverge`

Rules:
- `council` is advisory only
- `council` may emit findings, claims, votes, alternatives, confidence estimates, and synthesis proposals
- `council` must not write final verdicts
- `gate` remains the only writer of `DecisionObject`
- council is selective, not always-on
- selective means the repo already has a usable bootstrap + staged + proof-ready core loop, and the family-specific trigger in `docs/product/COUNCIL.md` is met

Stage-boundary note:

- `punk-council` is currently in-tree but inactive
- it may stay as a workspace member before Stage 2 is active
- until Stage 2 is explicitly promoted, that crate is not part of the normal v0 operator path
- `punk-shell`, `punk-skills`, `punk-eval`, and `punk-research` remain planned only until their stages

### Stage 3 — orchestration depth
Promote standalone `Goal`, then add project registry, queue, daemon, and higher-order orchestration.

Current note:

- `punk go --fallback-staged` and `punk start` already exist today as shell mechanisms over plain goal text
- this stage is where `Goal` becomes a first-class runtime primitive instead of staying deferred

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

### Stage 5 — research crate extraction + deeper research mode
Detailed spec: `docs/product/RESEARCH.md`

Current note:

- the bounded research packet/artifact/synthesis surface already exists today in `punk-cli` + `punk-orch` + `punk-domain`
- this stage is about extracting/expanding that behavior into a dedicated `punk-research` crate and deeper execution loops

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
