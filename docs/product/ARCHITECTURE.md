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

### Provider-alignment rule

`punk` should evolve as a **correctness and stewardship layer**, not as a parallel general-purpose agent platform.

Build in the kernel only what is local-trust-critical:

- bounded scope and safety policy
- repo scan and source anchors
- VCS-aware isolation and rollback
- gate truth
- receipts and proof artifacts

Wrap rather than rebuild when providers offer stable primitives for:

- agent runtimes
- tool calling and built-in tools
- tracing and observability
- session, memory, caching, or compaction
- structured output controls

Avoid adding kernel complexity that duplicates provider direction, especially:

- a custom universal agent runtime
- a large internal memory platform
- free-text-heavy orchestration magic
- a parallel tracing/eval stack

Reference note:

- `docs/research/2026-04-11-provider-alignment-build-wrap-avoid.md`
- `docs/product/ADR-provider-alignment.md`

### Inner-loop second-opinion rule

When the system needs a second opinion inside the autonomous loop, the default escalation target is:

- another model
- another provider
- or a bounded structured council protocol

It is **not** the user by default.

The user should participate:

- at goal intake
- at final result or final blocker review

but not as the routine tie-breaker for inner-loop uncertainty.

This means:

- model disagreement is an internal execution concern
- selective cross-model/provider checks are a derived mechanism over the same primitives
- user interruption should happen only when the system reaches a real terminal blocker, not when it merely wants another opinion

Security exception:

- sensitive decisions should remain single-model only when policy forbids sending the same material to multiple providers
- in those cases, the system should degrade to a local blocked/escalated outcome rather than silently broadening provider exposure

See also:

- `docs/product/NORTH-ROADMAP.md`
- `docs/research/2026-04-03-specpunk-identity-and-layering.md`
- `docs/research/2026-04-03-specpunk-primitives-and-derived-mechanisms.md`

### Core vs shell split

`punk` is intentionally split into two layers:

#### Layer A — correctness substrate

The substrate owns durable truth and safety invariants:

- `Project` identity
- plain goal intake today; standalone `Goal` record later
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
- native repo bootstrap packet writes (`.punk/project.json`)
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

Target-shape primitives are:

| Primitive | Why it is primitive |
|---|---|
| `Project` | identity and repo boundary |
| `Goal` | normalized user intent anchor in the long-term chain; deferred in current v0 |
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

Harness engineering should be treated the same way: it is a derived harness/evidence plane over existing primitives, not a new primitive truth object.

Current implementation note:

- the v0 runtime already exposes `punk start` and `punk go --fallback-staged`
- those are derived shell mechanisms over plain goal text today
- they do **not** mean the standalone `Goal` primitive is already active in the implemented domain/runtime

### Derived mechanism map

The first practical mapping should stay explicit:

| Mechanism | Type | Current v0 touch / target primitive relation |
|---|---|---|
| `init` | shell bootstrap | `Project`, `Ledger` |
| `start` | staged shell intake | plain goal text -> `Contract`, `Scope` today; standalone `Goal` later |
| `go` | autonomous shell intake | plain goal text -> `Contract`, `Run`, `DecisionObject`, `Proofpack`, `Ledger` today; standalone `Goal` later |
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

### Harness / evidence plane

`punk` should absorb harness engineering as a derived plane that makes runtime behavior more legible and verifiable without redefining substrate truth.

That means:

- `ProjectOverlay` should grow into the shell-facing map of harness capabilities
- `Workspace` remains the isolated execution context for harness boot and evidence collection
- `gate` should gradually move toward typed validation recipes, not only raw command checks
- `Proofpack` should persist enough execution context to distinguish a recorded bundle from a partially reconstructable verdict context
- `Ledger` should keep blocked harness recovery inspectable after the shell output is gone

Current bounded progress:

- `ProjectOverlay` now exposes derived `harness_summary`
- the current proof step persists typed `command` evidence for existing `target` / `integrity` checks in `DecisionObject` and `Proofpack`
- `Proofpack` now also persists `run_ref`, `workspace_lineage`, `executor_identity`, and an explicit `reproducibility_claim`
- this still does **not** introduce a repo-local harness packet or non-command evidence execution

See also:

- `docs/product/HARNESS.md`
- `docs/research/2026-04-08-specpunk-harness-engineering.md`

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

Current v0 architecture steering stays inside that same loop and does **not** add a fourth runtime mode:

- `plot` always writes a derived `architecture-signals.json` artifact next to the contract
- `plot` may also write a deterministic `architecture-brief.md` plus contract architecture integrity commitments when review is required
- `gate` writes a derived `architecture-assessment.json` artifact and uses it in the final verdict / proof chain

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

Repo-status vocabulary used below:

- **active v0 surface** = current operator/runtime path
- **in-tree but inactive** = workspace member kept buildable, but not part of the current operator path
- **planned only** = target-shape crate not in today's workspace membership

Canonical terms: `docs/product/REPO-STATUS.md`
Canonical full matrix: `docs/product/IMPLEMENTATION-STATUS.md`

Current workspace members in the active v0 surface:

- `punk-cli`
- `punk-domain`
- `punk-events`
- `punk-vcs`
- `punk-core`
- `punk-orch`
- `punk-gate`
- `punk-proof`
- `punk-adapters`

Current workspace members that are in-tree but inactive:

- `punk-council`

Planned-only crates for later stages:

- `punk-shell`
- `punk-skills`
- `punk-eval`
- `punk-research`

### Crate ownership

| Crate | Status | Owns |
|---|---|---|
| `punk-cli` | active v0 surface | non-interactive command entrypoint |
| `punk-shell` | planned only (Stage 1+) | interactive REPL, mode switching, context routing |
| `punk-domain` | active v0 surface | canonical types and schemas |
| `punk-events` | active v0 surface | append-only event log and projections |
| `punk-vcs` | active v0 surface | `jj` / `git` backend abstraction |
| `punk-core` | active v0 surface | deterministic repo helpers, scan, scope, validation |
| `punk-orch` | active v0 surface | feature/contract/task/run lifecycle services |
| `punk-gate` | active v0 surface | validation, checks, decision synthesis |
| `punk-proof` | active v0 surface | proof bundle writing and hashing |
| `punk-adapters` | active v0 surface | external drafting/execution/review adapters |
| `punk-council` | in-tree but inactive (Stage 2+) | structured deliberation protocols |
| `punk-skills` | planned only (Stage 4+) | skill registry, overlays, candidate patches |
| `punk-eval` | planned only (Stage 4+) | task eval, skill eval, promotion evidence |
| `punk-research` | planned only (Stage 5+) | bounded deep-research protocols |

Stage-boundary note:

- workspace membership is not the same thing as active operator surface
- a crate may exist in-tree before its stage is active
- `punk-council` is the current example: it remains a workspace member, but it is still in-tree but inactive until Stage 2 is promoted
- capability reality can lead crate extraction: the current frozen research-packet slice already lives in `punk-cli` + `punk-orch` + `punk-domain` even though `punk-research` remains planned only

Traits are only required where real backend choice exists.

Required ports over time:

- `VcsBackend`
- `Executor`
- `ContractDrafter`
- `CouncilProtocol`
- `SkillProvider`
- `EvalRunner`
- `PromotionPolicy`

### Adapter boundary policy

`punk-adapters` exists to **wrap** upstream runtimes, not to become a second runtime product.

What belongs inside `punk-adapters`:

- provider-specific invocation glue
- preflight/readiness checks
- normalized drafting and execution IO
- bounded prompt/context shaping needed to call upstream runtimes safely
- provider-specific failure classification and retry shaping
- future advisory council slot execution wrappers

What must stay out of `punk-adapters`:

- local scope law
- gate policy and final decision semantics
- proof ownership
- repo truth and source-anchor truth
- a universal provider-zoo runtime
- custom protocol invention where MCP or official provider surfaces already fit

Current minimum provider-agnostic adapter ports:

| Port | Status | Purpose |
|---|---|---|
| `ContractDrafter` | active v0 surface | normalize upstream drafting/refinement into `DraftProposal` |
| `Executor` | active v0 surface | normalize bounded execution into `ExecuteOutput` / receipt-ready facts |
| `ProviderAdapter` | in-tree but inactive | future council slot adapter for advisory-only protocol runs |

Rule of thumb:

- if an upstream-native capability gets better, `punk-adapters` should usually shrink
- new adapter code should preserve local correctness boundaries, not duplicate provider runtimes
- correctness guards remain local, but provider choreography should stay thin

Companion adapter docs:

- `docs/sauce/03-delta/CAPABILITY-MATRIX.md`
- `docs/sauce/03-delta/PROVIDER-DELTAS.md`

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
| `Goal` | target primitive; deferred in current v0 |
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
- `Proofpack` = hash-linked artifact bundle plus an explicit reproducibility claim about what execution context was captured

### Contract requirements

Every `Contract` must carry:

- `entry_points`
- `import_paths`
- `expected_interfaces`
- `behavior_requirements`
- `allowed_scope`
- `target_checks`
- `integrity_checks`

The persisted contract document may also carry deterministic architecture-steering extensions without introducing a new primitive object:

- `architecture_signals_ref`
- optional `architecture_integrity` with:
  - `review_required`
  - `brief_ref`
  - optional `touched_roots_max`
  - optional `file_loc_budgets[]`
  - optional `forbidden_path_dependencies[]`

### Architecture steering storage layout

The storage/layout split must stay explicit:

| Artifact | Path | Canonical or derived | Writer | Role |
|---|---|---|---|---|
| contract document | `.punk/contracts/<feature-id>/vN.json` | canonical | `plot` | approved/draft contract plus persisted `architecture_signals_ref` and optional `architecture_integrity` |
| architecture signals | `.punk/contracts/<feature-id>/architecture-signals.json` | derived | `plot` | deterministic repo-scan summary for the current contract state |
| architecture brief | `.punk/contracts/<feature-id>/architecture-brief.md` | derived | `plot` | reviewable deterministic brief for architecture-sensitive slices |
| project capability index | `.punk/project/capabilities.json` | derived inspect packet | `inspect project` / `plot approve` refresh | built-in repo-kind candidate graph (`active`, `suppressed`, `conflicted`, `advisory`) |
| frozen contract capability resolution | `.punk/contracts/<feature-id>/capability-resolution.json` | derived frozen packet | `plot approve` | frozen built-in capability semantics that shaped inferred checks, scope seeds, ignore rules, and controller scaffold kind |
| run receipt | `.punk/runs/<run-id>/receipt.json` | canonical | `cut` | execution truth consumed by `gate` |
| verification context | `.punk/runs/<run-id>/verification-context.json` | canonical frozen context | `cut` | frozen workspace identity + file states + frozen capability-resolution ref/hash |
| frozen architecture inputs | `.punk/runs/<run-id>/architecture-inputs.json` | derived frozen packet | `cut` | frozen contract-side architecture evidence refs plus copied run-scoped signals/brief hashes for `gate` / `proof` |
| architecture assessment | `.punk/runs/<run-id>/architecture-assessment.json` | derived | `gate` | deterministic assessment of frozen contract commitments vs receipt/check state |
| decision object | `.punk/decisions/<decision-id>.json` | canonical | `gate` | final verdict; carries the architecture assessment ref through `check_refs` |
| proofpack | `.punk/proofs/<decision-id>/proofpack.json` | canonical | `gate` | hash-linked proof chain; hashes the architecture assessment and frozen capability artifact when present |
| incident bundle | `.punk/incidents/<work-id>/<incident-id>/incident.json` | derived recovery/export artifact | `incident capture` | repo-local runtime-failure packet linked back to proof/run/decision/autonomy refs |
| imported incident bundle | `.punk/imported-incidents/<source-project>/<incident-id>/<promotion-id>/incident.json` | derived transfer artifact | `incident promote` | copied upstream evidence bundle inside the target repo before any fix contract runs |
| incident promotion record | `.punk/promotions/<incident-id>/<promotion-id>.json` | derived transfer ledger | `incident promote` | durable link between source incident, imported target bundle, drafted upstream contract, plus auto-run attempt/failure/completion metadata |
| incident submission record | `.punk/submissions/<incident-id>/<submission-id>/submission.json` | derived outbound ledger | `incident submit` | sanitized GitHub issue packet plus publish outcome/error state for external reporting |

That means there is still only one contract truth and one final verdict truth:

- contract-side architecture commitments live in the persisted contract document
- signals / brief / assessment stay derived and reviewable under `.punk/`
- final accept/block/escalate still lives only in `DecisionObject`
- final hash-linked chain still lives only in `Proofpack`

Default v0 `ArchitectureSignals` thresholds are:

- `warn_file_loc >= 600`
- `critical_file_loc >= 1200`
- `critical_scope_roots > 1`
- `warn_expected_interfaces > 2`
- `warn_import_paths > 5`

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
- architecture signals / briefs / assessments

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
- `cut` freezes any contract-side architecture evidence expected for the run into `.punk/runs/<run-id>/architecture-inputs.json` plus run-scoped copies of `architecture-signals.json` and optional `architecture-brief.md`
- `gate` reads only those frozen run-scoped architecture refs when architecture evidence is present for the run
- `gate` reads the persisted `Receipt`
- `gate` reads the persisted verification context referenced by `Run`
- `gate` may read persisted check outputs
- `gate` must not rely on live prompt reinterpretation
- `gate` must execute trusted checks from that frozen verification context, not from mutable live repo state

`Proofpack` must point to immutable refs and hashes of:

- run artifact, when present
- approved contract
- receipt
- decision object
- verification context, when present
- check outputs
- architecture assessment, when present

Current architecture decision rule inside `gate`:

- if persisted architecture signals are `critical` and the approved contract has no `architecture_integrity`, return `Escalate`
- if enforced architecture constraints are present and breached, return `Block`
- if enforced architecture constraints pass, keep the architecture assessment in the same decision/proof chain
- if enforced `forbidden_path_dependencies[]` are present, `gate` must deterministically scan touched matching files for direct local dependency edges and return `Block` on a violated edge
- v0 forbidden dependency enforcement is intentionally cheap: it currently resolves deterministic Rust crate/module references and JS/TS relative imports; unsupported matching file types must stay `Unverified`

`Proofpack` v0 should also state an explicit reproducibility claim.

Current claim levels are:

- `frozen_context_v0` — proof records run lineage, executor identity, and the frozen verification-context digest used by `gate`
- `run_record_v0` — proof records run lineage and executor identity, but lacks a frozen verification-context digest
- `record_plus_context_v0` — proof records executor identity and frozen context digest, but the run artifact is missing
- `record_only_v0` — proof is only a hash-linked record bundle

These levels are intentionally honest.
They do **not** claim hermetic rebuilds, notarized provenance, or full environment replay.

---

## 7. Runtime capability policy

| Mode | Allowed | Forbidden |
|---|---|---|
| `plot` | repo read, VCS read, deterministic repo scan, contract draft/refine writes, local planning artifact writes under `.punk/` | source mutation, patch apply, broad mutating execution, final decision writing |
| `cut` | isolated change creation, source mutation within approved scope, executor invocation, test/build/check execution, receipt writing | final decision writing, proof writing |
| `gate` | artifact read, scope validation, deterministic target/integrity checks via trusted direct process execution, decision writing, proof writing | broad source mutation, unrestricted repair edits |

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
  latest_autonomy_ref
  autonomy_outcome
  recovery_contract_ref
  architecture
    signals_ref
    brief_ref
    assessment_ref
    severity
    trigger_reasons[]
    assessment_outcome
    assessment_reasons[]
    contract_integrity
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

### Current inspect surface

The current bounded implementation surface is:

- `punk inspect work`
- `punk inspect work <id>`
- `punk inspect work <id> --json`

`punk status` should now prefer this derived view for current work continuity fields instead of reconstructing latest state directly from raw events.

This is still a derived repo-local view over existing artifacts, not a new persistence layer.

Current architecture-steering inspect story should stay explicit and boring:

- `punk status [id]` remains the terse lifecycle pointer (`work_id`, latest ids, next action, suggested command)
- `punk inspect work [id]` is the stable derived work view for architecture refs:
  - signals severity / trigger summary
  - `signals_ref`
  - `brief_ref`
  - `assessment_ref`
  - assessment outcome / summary
  - copied contract-side `architecture_integrity`
- `punk inspect <contract-id> --json` remains the full canonical contract view
- `punk inspect <proof-id> --json` remains the full canonical proof-chain view

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

The current bounded implementation now writes a repo-local autonomy-linked record during
`punk go`, and `WorkLedgerView` projects that record back into:

- `latest_autonomy_ref`
- `autonomy_outcome`
- `recovery_contract_ref`
- recovery-aware `lifecycle_state`
- durable `next_action` / `next_action_ref`

Shell status/read surfaces may additionally derive a `suggested_command`, but that remains a shell convenience layered on top of the durable refs above.

### Recovery distinction

The architecture should distinguish:

- blocked with no durable recovery prepared
- blocked with a staged recovery contract already prepared

That distinction is important for both shell UX and later inspection.

### Derived runtime-incident lane

Blocked or escalated autonomy can also expose a narrower derived artifact:

```text
IncidentRecord
  work_id
  goal
  contract_ref
  run_ref
  decision_ref
  proof_ref
  autonomy_ref?
  incident_kind
  decision_outcome
  summary
  blocked_reason
  failure_signature
  capture_basis[]
  issue_draft_ref
  repro_ref
  created_at
```

Rules:

- this is a **derived recovery/export artifact**, not a new primitive truth object
- it should be captured from persisted proof/run/decision/autonomy artifacts, not terminal scraping
- the shell may suggest incident capture only for deterministic suspected-runtime-bug cases, not for every blocked project check failure
- internal promotion copies that bundle into another `punk` repo, drafts an inspectable contract there, and records the handoff under `.punk/promotions/...`
- plain promotion stops at draft creation; `--auto-run` is the explicit opt-in for continuing with approve/execute/gate/proof upstream
- `--auto-run` must stay deterministic and policy-gated: only suggest or permit it when the effective promote target has a matching `.punk/project.json` identity packet, an `AGENTS.md` guide that identifies `specpunk`, and the expected local `specpunk` markers (`Cargo.toml`, `crates/punk-cli/src/main.rs`, `crates/punk-orch/src/lib.rs`, `docs/product/CLI.md`); otherwise keep the lane draft-only
- failed internal auto-run attempts should still update the promotion record with attempt count and the last failed phase/error/partial refs, so retry does not depend on shell history
- external submission writes a sanitized `.punk/submissions/...` bundle first and only publishes to GitHub with explicit operator opt-in
- repo-local incident defaults remain project-scoped under `.punk/project/incident-defaults.json`, while operator-wide defaults live under `~/.punk/config/incident-defaults.json`; target resolution precedence is explicit flag > repo-local default > global default
- this lane exists to make foreign-repo `punk` failures inspectable and transferable without trying to fix `punk` inside the foreign repo itself

The second derived record is:

```text
IncidentPromotionRecord
  incident_id
  source_project_id
  source_repo_root
  source_incident_ref
  source_issue_draft_ref
  source_repro_ref
  target_project_id
  target_repo_root
  imported_incident_ref
  imported_issue_draft_ref
  imported_repro_ref
  prepared_goal
  draft_feature_id
  draft_contract_id
  auto_run_attempts
  last_attempt_at?
  last_failure?
    phase
    summary
    contract_status?
    run_id?
    receipt_ref?
    decision_id?
    failed_at
  execution?
    run_id
    receipt_ref
    decision_id
    proof_id
    decision_outcome
    receipt_summary
    completed_at
  created_at
```

The external-report record is:

```text
IncidentSubmissionRecord
  incident_id
  submission_kind
  github_repo
  issue_title
  body_ref
  preview_command
  state
  published_issue_url?
  published_issue_number?
  publish_error?
  created_at
  updated_at
```

---

## 9. Storage layout

### Global runtime state

```text
~/.punk/
  config.toml
  config/
    incident-defaults.json
  events/
  views/
```

Use this for:

- event log
- materialized views
- global config
- skill/eval metadata that is not repo-tracked
- operator-wide incident routing defaults that should apply across repos

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

### `ProjectOverlay`

Project intelligence should converge on one inspectable repo-local packet rather than a growing set of adjacent special cases.

The intended packet shape is:

```text
ProjectOverlay
  project_id
  repo_root
  overlay_ref
  vcs_mode
  bootstrap_ref
  agent_guidance_ref
  capability_summary
  capability_resolution
  harness_summary
  harness_spec_ref
  harness_spec
  project_skill_resolution_mode
  project_skill_refs
  ambient_project_skill_refs
  local_constraints
  safe_default_checks
  status_scope_mode
  updated_at
```

### Why this packet exists

It should unify:

- resolver/pin state
- bootstrap guidance
- repo-root `AGENTS.md`
- `.punk/AGENT_START.md`
- project-scoped skills
- repo-specific safe defaults

so that operators and agents can inspect one source instead of reconstructing project intelligence from scattered files.

Current bootstrap-packet rule:

- prefer the native `.punk/bootstrap/<project>-core.md` packet written by `punk init`
- if a repo already has exactly one legacy `.punk/bootstrap/*-core.md` packet, reuse that packet as the current `bootstrap_ref` instead of assuming the repo basename
- do not create competing bootstrap docs for the same repo unless migration policy explicitly says so

### Projection rule

`ProjectOverlay` should be inspectable and explicit.

It must not become:

- opaque hidden heuristics
- silent auto-mutation
- a second uncontrolled source of truth for runtime artifacts

The likely correct model is:

- canonical project facts and refs are persisted at `.punk/project/overlay.json`
- built-in repo-kind candidate resolution is persisted separately at `.punk/project/capabilities.json`
- shell commands read and display that one project-intelligence packet instead of reconstructing primary truth from ambient directories
- repo-local `.punk/skills/overlays/**/*.md` refs are the primary project-skill source
- ambient skill discovery is fallback/migration-only and must stay explicit in the packet when it is used
- project-specific skill composition remains explicit and inspectable

### Internal capability-resolution rule

`punk-core` owns one built-in capability registry for current v0 repo kinds:

- `rust-cargo`
- `node-package-scripts`
- `go-mod`
- `python-pyproject-pytest`
- `swiftpm`

This is an internal unification slice, not a public pack system:

- no `punk packs ...`
- no plugin ABI
- no ambient execution-time pack loading
- no same-id shadowing of built-ins

Resolution is frozen at `plot approve` into the contract-scoped capability packet, then copied by ref/hash into the run verification context. `gate` and `proof` must verify against that frozen packet, not against live repo scans or ambient state.

### Shared repo-relative path classification

The current v0 loop must not maintain separate truth for repo-relative path classes.

`punk-core` owns the shared classifier for:

- `Product`
- `GeneratedNoise`
- `RuntimeArtifact`
- `ScanOnlyExcluded`

Current required consumers of that same classifier:

- repo scan and scope-candidate walks
- VCS changed-file / provenance filtering
- isolated workspace sync into and out of repo root
- verification-context capture and drift validation
- `gate` scope and architecture changed-file filtering

Rule:

- `ScanOnlyExcluded` paths are excluded from repo walks, but they are **not** global non-product truth
- `GeneratedNoise` and `RuntimeArtifact` paths must not pollute provenance, sync, frozen verification context, or `gate` scope judgments
- product-facing invariants should be strengthened by reusing this classifier, not by adding more per-layer special cases

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

Current v1 families are:

- architecture council
- contract council
- review council

Deferred beyond the current v1 council scope:

- migration/cleanup council
- implementation diverge
- research-backed synthesis

Council is selective, not always-on.

Selective means:

- the repo already has a usable bootstrap + staged + proof-ready core loop
- the council family is advisory-only
- the family-specific trigger from `docs/product/COUNCIL.md` is actually met

---

## 13. Skills and eval ratchet

### Skill architecture

Skills should be composed from layers:

1. base skill
2. domain skill
3. project overlay
4. task packet

A live agent should receive a composed skill packet, not a single monolithic markdown blob.

The long-term project layer should therefore line up with `ProjectOverlay`, not with ad hoc bootstrap files alone.

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

This is the long-term role of a dedicated `punk-research` crate and `delve`-style workflows.

Current status note:

- the dedicated `punk-research` crate is still **planned only**
- the bounded `punk research ...` capability below is already real today in `punk-cli` + `punk-orch` + `punk-domain`

Current implemented boundary:

- the current slice only freezes repo-local research packets and inspectable records
- `punk research start` may write `question.json`, `packet.json`, and `record.json` under `.punk/research/<research-id>/`
- `punk research artifact <research-id> ...` may append structured artifact records under `.punk/research/<research-id>/artifacts/`
- if artifact writing invalidates a previously synthesized current view, the mutable `synthesis.json` alias may be removed while immutable synthesis history remains intact
- repeated invalidation cycles may accumulate typed invalidation history entries on the record even after the active invalidation note is cleared by a later synthesis
- `punk research synthesize <research-id> ...` may write one structured mutable `synthesis.json` current view, persist an immutable identity copy under `syntheses/<synthesis-id>.json`, carry explicit repo-local `follow_up_refs[]`, and require explicit operator replace intent for repeated synthesis writes
- `punk research complete <research-id>` and `punk research escalate <research-id>` may apply terminal operator-triggered stop states to the persisted `record.json`
- `punk inspect research_<id>` may read that frozen bundle back and surface persisted synthesis follow-up refs, immutable synthesis identity, replacement lineage, and current-view invalidation notes in human summaries; the JSON inspect payload may also expose derived `invalidation` and `synthesis_lineage` projections for downstream tooling, including oldest-to-newest synthesis history built from immutable refs plus small convenience booleans derived from the same lineage
- worker orchestration and critique loops remain future Stage 5+ behavior

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
   - `verification_context_ref` once `cut run` persists the frozen check context
   - `architecture_inputs_ref` when the approved contract carries contract-side architecture evidence that must stay frozen for `gate` / `proof`
3. Every approved `Contract` must have non-empty:
   - `allowed_scope`
   - `target_checks`
   - `integrity_checks`
4. Every `DecisionObject` must distinguish:
   - `target_status`
   - `integrity_status`
   - `confidence_estimate`
   - the verification-context ref / identity used for the verdict
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
- frozen verification-context validation
- policy validation
- target/integrity checks via trusted direct execution of validated runners
- decision synthesis

### `punk-proof`

Owns:

- proofpack creation
- artifact hashing
- verification-context hashing / propagation
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
