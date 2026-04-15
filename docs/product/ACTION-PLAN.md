# SpecPunk Action Plan

Date: 2026-04-11
Source inputs: current repo docs, current implementation snapshot, and the architecture review memo

## Purpose

Turn the current review into an execution plan that strengthens SpecPunk without changing its product thesis.

This plan assumes the thesis is already correct:

- one public surface: `punk`
- one vocabulary: `plot / cut / gate`
- contract first
- `gate` writes the final decision
- proof before acceptance
- council is selective and advisory
- skills evolve through candidate -> eval -> promotion
- local-first, VCS-aware operation

The goal is not to redirect the project. The goal is to close the highest-value architectural gaps so the implementation matches the stated laws.

## Planning stance

For each workstream below:

- prefer upstream-native capabilities first
- add only the SpecPunk layer that upstream does not cover
- avoid inventing parallel provider machinery
- update docs and tests in the same change when behavior changes

## Priority summary

### P0

1. remove public dependency on `punk-run`
2. make `gate` evaluate a frozen run context, not the live repo
3. reconcile docs and workspace stage boundaries

### P1

4. keep adapters thin and explicitly upstream-aligned
5. make `ProjectOverlay` the real project-intelligence packet
6. strengthen `Proofpack` from record bundle toward reproducible proof bundle

### P2

7. add council only after the core loop is hermetic
8. add task eval and skill eval as separate offline ratchets
9. add bounded research mode after the above are stable

---

## Workstream 1 — Replace `punk-run` bootstrap with native `punk init`

### Upstream relation

- OpenAI harness/repo guidance
- Anthropic project-memory and harness guidance

### Recommended action

**Retire** public dependency on `punk-run`.

### Real SpecPunk gap

SpecPunk still lacks a native bootstrap path that makes the repo self-describing without delegating to a legacy CLI.

### Owner layer

- shell
- stewardship
- project bootstrap

### Out of scope

- keeping `punk-run` as a second public operator surface
- maintaining a hidden long-term dependency on old bootstrap behavior

### Deliverables

1. Native `punk init` writes and verifies:
   - `AGENTS.md`
   - `.punk/AGENT_START.md`
   - `.punk/project.json`
   - `.punk/bootstrap/<project>-core.md` or a successor packet
2. `punk inspect project` reads only native artifacts.
3. `punk-cli` no longer calls `punk-run init`.
4. All operator-facing errors and help text mention only `punk`.

### Concrete tasks

- remove `maybe_auto_bootstrap_project()` delegation to `punk-run`
- remove `run_project_bootstrap()` and `run_explicit_project_init()` dependency on `punk-run`
- add a native bootstrap writer in the `punk` codebase
- define the minimal bootstrap packet shape
- make `inspect_project_overlay()` consume that packet as first-class input
- update CLI summaries and recovery text
- add migration behavior for repos that already contain legacy bootstrap artifacts

### Acceptance criteria

- fresh repo can run `punk init --enable-jj --verify` with no external `punk-run`
- `punk go` and `punk start` work after native init only
- `punk inspect project --json` returns a complete packet with no missing bootstrap dependency
- no user-facing text instructs operators to run `punk-run`

### Files to update

- `crates/punk-cli/src/main.rs`
- `crates/punk-orch/src/lib.rs`
- `README.md`
- `docs/product/CLI.md`
- `docs/product/ARCHITECTURE.md`
- `docs/product/MASTER-PLAN.md`

---

## Workstream 2 — Freeze `gate` to the run context

### Upstream relation

- OpenAI artifact-first harness thinking
- Anthropic long-running harness / handoff-artifact guidance

### Recommended action

**Compose** the current gate with a stricter frozen verification context.

### Real SpecPunk gap

`gate` still evaluates from the live repo root instead of a canonical verification context for the run.

### Owner layer

- gate
- proof
- VCS substrate

### Out of scope

- letting `gate` repair code
- letting `gate` reinterpret user intent live
- making `gate` another execution agent

### Deliverables

1. `Run` carries a canonical verification context reference.
2. `gate` runs checks against that context, not the mutable live repo.
3. `Proofpack` includes execution-context identity.
4. verification fails explicitly when the context is unavailable or drifted.

### Concrete tasks

- define a `VerificationContext` shape, minimal for v0
- persist enough information at `cut run` time to reconstruct the gate context
- switch `punk-gate` check execution to that frozen context
- include verification-context refs and hashes in `Proofpack`
- add tests where the live repo changes after `cut` but before `gate`
- add tests for missing/invalid verification context reconstruction

### Acceptance criteria

- changing the live repo after `cut run` does not silently affect `gate run`
- `DecisionObject` and `Proofpack` can be traced to the same execution context
- the system fails closed if the run context cannot be trusted

### Current bounded progress

The current v0 implementation should also keep one shared repo-relative path classifier across:

- repo scan
- VCS provenance
- isolated workspace sync
- verification-context capture
- `gate` scope / architecture filtering

This strengthens the same frozen-context workstream without adding a new subsystem.

### Files to update

- `crates/punk-orch/src/lib.rs`
- `crates/punk-gate/src/lib.rs`
- `crates/punk-proof/src/lib.rs`
- `crates/punk-domain/*`
- `docs/product/ARCHITECTURE.md`
- `docs/product/MASTER-PLAN.md`

---

## Workstream 3 — Reconcile docs vs workspace reality

### Upstream relation

- repo-as-system-of-record guidance

### Recommended action

**Adopt or defer explicitly**, but stop leaving stage boundaries ambiguous.

### Real SpecPunk gap

The repo sometimes describes subsystems as future stages while already carrying them in the active workspace.

### Owner layer

- product docs
- workspace shape
- release/stage discipline

### Out of scope

- broad feature expansion before the core loop is sealed

### Deliverables

1. A single truthful stage map.
2. Clear rule for what “implemented”, “in-tree but inactive”, and “planned” mean.
3. README, architecture docs, and workspace membership all tell the same story.

### Concrete tasks

- audit all current crates against stage claims
- decide explicitly for each crate: active / parked / planned
- if `punk-council` is not active yet, mark it clearly as in-tree but not part of the working slice, or remove it from the active workspace until its stage
- add a small repo-status note describing what is real today vs target shape

### Acceptance criteria

- a new contributor can read README + architecture docs and understand the current slice without contradiction
- workspace membership matches stage language

### Files to update

- `Cargo.toml`
- `README.md`
- `docs/product/VISION.md`
- `docs/product/ARCHITECTURE.md`
- `docs/product/MASTER-PLAN.md`
- `docs/product/CLI.md`

---

## Workstream 4 — Keep adapters thin and upstream-first

### Upstream relation

- OpenAI native model/tool/runtime surfaces
- Anthropic Claude Code / SDK direction
- Google ADK direction
- MCP as provider-neutral boundary

### Recommended action

**Wrap** and **compose**, do not build a provider-zoo execution substrate.

### Real SpecPunk gap

SpecPunk needs bounded correctness, ledgers, proof, and eval ratchets around upstream execution, not a permanent custom provider stack.

### Owner layer

- adapters
- shell-to-execution boundary

### Out of scope

- generic multi-provider orchestration theater
- custom substitute for provider runtimes
- parallel protocol surfaces where MCP already fits

### Deliverables

1. A written adapter boundary policy.
2. A provider capability matrix.
3. A delta log that says when to adopt, wrap, compose, defer, or retire local machinery.

### Concrete tasks

- document what belongs in `punk-adapters` and what must stay out
- separate correctness guards from provider-specific command choreography
- define the minimum provider-agnostic adapter traits needed for the product
- add a provider-delta doc that tracks native upstream capabilities relevant to SpecPunk

### Acceptance criteria

- adapters are obviously serving the core laws, not becoming a second runtime product
- adding a new upstream-native capability usually means thinner local code, not more local code

### Files to update

- `docs/product/ARCHITECTURE.md`
- new `docs/sauce/03-delta/PROVIDER-DELTAS.md`
- new `docs/sauce/03-delta/CAPABILITY-MATRIX.md`
- `crates/punk-adapters/*`

---

## Workstream 5 — Make `ProjectOverlay` the actual project-intelligence packet

### Upstream relation

- short top-level guidance plus repo-local deep knowledge
- project memory / skill layering guidance

### Recommended action

**Adopt** `ProjectOverlay` as the canonical inspectable packet and **retire** ambient discovery as primary truth.

### Real SpecPunk gap

`ProjectOverlay` exists, but some project intelligence still depends on ambient filesystem conventions such as external skill directories.

### Owner layer

- stewardship
- project overlay
- skills boundary

### Out of scope

- hidden heuristic project memory
- silent skill mutation
- hidden global state as the main source of repo intelligence

### Deliverables

1. One explicit project-intelligence packet.
2. Repo-local refs to active project skills and constraints.
3. Minimal dependence on ambient non-repo locations.

### Concrete tasks

- decide what belongs in repo-tracked overlay vs global runtime state
- persist explicit project skill refs in the overlay
- make `inspect project` return all important project intelligence without searching ambient directories first
- downgrade external bus scanning to fallback or migration behavior

### Acceptance criteria

- repo-local overlay is sufficient for an agent/operator to understand the project
- project skill discovery is explicit and inspectable

### Files to update

- `crates/punk-orch/src/lib.rs`
- `docs/product/ARCHITECTURE.md`
- `docs/product/SKILLS.md`
- bootstrap packet docs

---

## Workstream 6 — Strengthen proof from record bundle to reproducible proof bundle

### Upstream relation

- proof-bearing and artifact-first harness direction

### Recommended action

**Compose** stronger proof inputs into the existing `Proofpack` model.

### Real SpecPunk gap

The current proof is already good as a hash-linked record bundle, but it still under-specifies execution environment and reproduction context.

### Owner layer

- proof
- gate
- VCS substrate

### Out of scope

- heavyweight notarization system
- perfect hermetic builds before the core loop is stable

### Deliverables

1. `Proofpack` includes execution-context identity.
2. proof includes tool/executor identity and relevant environment digest.
3. proof has a documented reproducibility claim level.

### Concrete tasks

- add execution context ref/hash to proof
- add executor identity and version where available
- add VCS snapshot identity and workspace lineage
- document “what this proof proves” for v0 vs later stages

### Acceptance criteria

- proof claim is explicit, bounded, and honest
- operators can tell the difference between “recorded evidence” and “reconstructable verdict context”

### Files to update

- `crates/punk-proof/src/lib.rs`
- `docs/product/ARCHITECTURE.md`
- proof schema docs

---

## Workstream 7 — Bring in council only after the core loop is hermetic

### Upstream relation

- multi-agent deliberation should be selective and advisory

### Recommended action

**Defer** expansion until after P0 is complete.

### Real SpecPunk gap

Council is valuable, but it is not the next bottleneck. The next bottleneck is trust in the core loop.

### Owner layer

- council
- plot/gate integration

### Out of scope

- always-on multi-agent chat
- council as default tax on all work
- council writing final acceptance decisions

### Deliverables

1. `council` remains advisory-only.
2. v1 families stay narrow: architecture, contract, review.
3. `gate` remains final writer.

### Concrete tasks

- keep protocol docs, but do not make council a dependency of core acceptance
- only resume council implementation once bootstrap and frozen gate are done
- define the exact threshold for “selective” council invocation

### Acceptance criteria

- core loop remains usable without council
- council output can enrich decisions without owning them

### Files to update

- `docs/product/COUNCIL.md`
- workspace membership and stage notes if needed

---

## Workstream 8 — Add eval as a real ratchet, separate from acceptance

### Upstream relation

- eval-driven skill improvement

### Recommended action

**Adopt** offline eval ratchets after the core loop is trustworthy.

### Real SpecPunk gap

The project has the right eval philosophy, but it still needs stable task evidence and durable overlays before promotion logic can be trusted.

### Owner layer

- eval
- skills
- stewardship

### Out of scope

- online self-modifying skills
- auto-promotion without review

### Deliverables

1. task eval and skill eval remain separate in code and docs
2. candidate patch lifecycle is explicit
3. promotion decisions are conservative and reproducible

### Concrete tasks

- define minimum `EvalSuite` and `PromotionDecision` storage
- mine current runs for first replayable cases
- keep promotion deterministic and reviewable

### Acceptance criteria

- no single successful run can auto-promote a skill patch
- safety regressions block promotion every time

### Files to update

- `docs/product/EVAL.md`
- `docs/product/SKILLS.md`
- future `punk-eval` and `punk-skills` crates

---

## Suggested sequence

### Phase A — close the product surface

- native `punk init`
- remove `punk-run` from operator path
- make `inspect project` fully native

### Phase B — close trust in the core loop

- persist verification context in `Run`
- run `gate` against frozen context
- extend `Proofpack`
- add drift tests

### Phase C — close repo truth

- reconcile docs and workspace stages
- formalize `ProjectOverlay`
- reduce ambient/global discovery

### Phase D — widen carefully

- adapter boundary policy
- provider delta log
- selective council
- eval ratchet
- research mode

---

## 30-day implementation target

### Week 1

- design native bootstrap packet
- remove public `punk-run` references from docs and CLI text
- implement native `punk init` write path

### Week 2

- add verification-context fields to run artifacts
- bind `gate` to frozen context
- add drift and failure-closed tests

### Week 3

- extend `Proofpack` schema
- normalize stage docs vs workspace
- finalize `ProjectOverlay` ownership rules

### Week 4

- document adapter boundary
- create provider delta docs
- prepare council/eval work only after P0 acceptance passes

---

## Exit criteria for this plan

The plan is successful when all of the following are true:

1. `punk` is the only public operator surface.
2. `gate` decisions are tied to the run context, not the mutable live repo.
3. `Proofpack` honestly captures what was verified and where.
4. docs and workspace say the same thing about current vs future stages.
5. project intelligence is inspectable from repo-local artifacts.
6. adapters remain thin and upstream-aligned.

When these are true, SpecPunk will have a much stronger base for council, skills, eval ratchets, and bounded research.
