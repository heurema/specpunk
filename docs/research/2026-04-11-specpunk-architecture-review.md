# SpecPunk Architecture Review

Date: 2026-04-11
Review scope: product shape, architecture boundary, implementation coherence, upstream alignment

## Executive summary

SpecPunk already has a strong product shape.

The project is not trying to be a generic agent shell, a provider-zoo UI, or a hosted control plane. It is building a local-first engineering runtime with a clear artifact chain, hard runtime mode boundaries, and a meaningful stewardship layer. That is the right direction.

The strongest parts are:

- one public surface: `punk`
- one vocabulary: `plot / cut / gate`
- contract-first execution
- `gate` as the only final decision writer
- proof-bearing artifact flow
- event-log-backed runtime truth
- selective council rather than always-on multi-agent theater
- explicit separation of task acceptance from skill/eval improvement

The main issue is not product thesis. The main issue is architectural closure.

SpecPunk still has a few important gaps where the implementation does not yet fully match the laws stated in the docs. The highest-priority ones are:

1. public-surface drift between `punk` and legacy `punk-run`
2. `gate` evaluating live repo state instead of a fully frozen run context
3. stage-boundary drift between docs and workspace reality
4. risk that the adapter layer grows into a provider-specific execution stack instead of a thin upstream-aligned wrap
5. proof artifacts that are strong as record bundles, but not yet strong enough as reproducible execution proofs

This is a good project that needs sharpening, not redirection.

## Overall assessment

### What is already strong

The architectural north star is coherent.

SpecPunk is trying to own only the missing layer that upstream systems do not fully own:

- bounded correctness
- stewardship
- durable work state
- proof-bearing decisions
- project-specific overlays
- eval ratchets

That is the correct boundary.

The repo already expresses several unusually strong decisions:

- `plot`, `cut`, and `gate` are permission boundaries, not tone presets
- runtime truth is append-only plus derived views
- `gate` alone writes `DecisionObject`
- `Proofpack` is explicit and hash-linked
- `WorkLedgerView` is treated as a projection, not a second mutable truth source
- `council` is advisory-only by design
- `eval` is not allowed to collapse into task-level acceptance

This gives SpecPunk a real chance to be a serious harness rather than a prompt bundle.

### What is not yet closed

The implementation still leaks some transitional assumptions from the old world.

The largest ones are:

- bootstrap and init still depend on `punk-run`
- `gate` runs checks from the live repo root rather than a guaranteed frozen execution context
- the repo sometimes describes subsystems as deferred while already carrying them in the workspace
- project overlay and skill discovery currently depend on ambient filesystem conventions that do not yet look like final system-of-record behavior

These are not cosmetic issues. They affect trust, operator expectations, and long-term maintainability.

## Review findings

## Finding 1 — The product thesis is strong and should be preserved

### Related upstream capability or document

- OpenAI harness engineering guidance
- Anthropic long-running harness guidance
- Google ADK agent/runtime framing

### Recommended action

**Adopt** the upstream harness posture and **preserve** the current SpecPunk thesis.

### Why this is correct

SpecPunk is already aligned with the best current upstream direction:

- short top-level guidance, deeper repo docs as system of record
- explicit artifacts across sessions
- composable agent/runtime patterns
- evaluations as a ratchet, not a vibe

### Real SpecPunk gap

The gap is not thesis. The gap is operational closure.

### SpecPunk owner layer

- product boundary
- kernel/stewardship contract

### Out of scope

- becoming a hosted orchestration product
- building a provider-zoo shell
- replacing official provider workflows with custom parallel machinery

### Docs / evals / skills to update

- keep `README.md`, `VISION.md`, and project prompt aligned
- add a provider-delta log so the thesis remains current as upstream shifts

## Finding 2 — `punk` still depends too visibly on legacy `punk-run`

### Related upstream capability or document

- upstream guidance favors one stable operator surface with repo-local guidance as system of record

### Recommended action

**Retire** the public dependency on `punk-run`.

### Why this matters

The repo states one public surface: `punk`.

But the CLI still auto-bootstraps and initializes through `punk-run`, and its operator-facing messages still instruct users to run `punk-run init ...` manually.

That means the public product surface is not actually singular yet.

This creates three problems:

- operator trust is split across two CLIs
- docs and runtime expectations drift
- legacy compatibility pressure leaks into the rebuild

### Real SpecPunk gap

SpecPunk still needs a native bootstrap path that creates the repo-local project-intelligence packet without delegating to legacy tooling.

### SpecPunk owner layer

- shell
- stewardship
- project bootstrap

### Out of scope

- preserving the legacy CLI as a parallel user-facing product
- keeping a hidden bootstrap dependency indefinitely

### Docs / evals / skills to update

- `README.md`
- `docs/product/CLI.md`
- `docs/product/MASTER-PLAN.md`
- bootstrap tests in `punk-cli`
- project-overlay docs once native bootstrap exists

### Concrete recommendation

Replace the external bootstrap bridge with a native `punk init` path that writes:

- `AGENTS.md`
- `.punk/AGENT_START.md`
- project packet / overlay metadata
- VCS mode verification results
- safe default checks

That keeps the public surface honest.

## Finding 3 — `gate` is not frozen enough yet

### Related upstream capability or document

- long-running harness guidance from Anthropic
- artifact-first harness guidance from OpenAI

### Recommended action

**Compose** the existing `gate` logic with a stricter frozen-run execution context.

### Why this matters

The docs say `gate` must evaluate frozen persisted inputs.

That is exactly right.

But the current `gate` implementation executes checks from `repo_root`, which means the verdict can depend on the live repo state at gate time rather than the isolated mutation context that produced the run and receipt.

That weakens the core trust claim of the runtime.

### Real SpecPunk gap

SpecPunk still needs a canonical notion of the verification context for a run, not just the contract and receipt.

### SpecPunk owner layer

- gate
- proof
- VCS substrate

### Out of scope

- letting `gate` mutate source freely
- letting `gate` repair work until it passes
- replacing deterministic verification with prompt reinterpretation

### Docs / evals / skills to update

- `docs/product/ARCHITECTURE.md`
- `docs/product/MASTER-PLAN.md`
- `punk-gate` tests for repo drift between `cut` and `gate`
- `punk-proof` schema and hashing inputs

### Concrete recommendation

Make verification explicitly bind to the run context.

At minimum:

- persist the exact workspace root or snapshot ref that `cut` executed against
- run gate checks against that frozen execution context
- include execution-context identity in `Proofpack`
- fail verification if the run context can no longer be reconstructed or trusted

Until then, `Proofpack` is a strong record bundle, but not yet a full reproducibility claim.

## Finding 4 — The evented artifact model is the right backbone

### Related upstream capability or document

- OpenAI repo-as-system-of-record guidance
- Google ADK evaluation and trajectory thinking

### Recommended action

**Adopt** and continue the current artifact/event backbone.

### Why this is strong

This is one of the best parts of the project.

The object chain and the append-only log provide the right base for:

- inspectable work continuity
- deterministic derived views
- durable recovery state
- future skill/eval ratchets
- proof-bearing audits

The introduction of `WorkLedgerView` and autonomy-linked durable records is especially good because it turns shell summaries into inspectable runtime state.

### Real SpecPunk gap

The model is correct, but projection policy still needs hardening so projections do not start to accumulate hidden semantics.

### SpecPunk owner layer

- kernel
- events
- stewardship views

### Out of scope

- replacing event truth with ad hoc file scanning
- turning projections into a second mutable truth store

### Docs / evals / skills to update

- projection invariants in architecture docs
- tests for projection correctness under partial artifact presence and recovery flows

### Concrete recommendation

Treat every new operator-facing continuity field as projection-only unless there is a very strong reason to promote it into a new primitive.

That keeps the kernel small.

## Finding 5 — `council` is well-shaped conceptually, but stage boundaries need cleanup

### Related upstream capability or document

- selective subagent / multi-agent patterns upstream

### Recommended action

**Defer** expansion, but **clean up** the stage boundary now.

### Why this matters

The design for `council` is strong because it is:

- selective
- blinded where it matters
- advisory-only
- clearly separated from final acceptance

That is the right way to use multi-model deliberation.

The issue is not the design. The issue is stage clarity.

Docs describe council as deferred beyond the first slice, but the workspace already includes a council crate. That creates ambiguity about what is real, what is staged, and what is merely target shape.

### Real SpecPunk gap

SpecPunk needs a single canonical answer to the question:

“Is council merely specified, partially scaffolded, or already part of the active runtime surface?”

### SpecPunk owner layer

- product staging
- workspace governance

### Out of scope

- turning council into always-on overhead
- letting council write final decisions

### Docs / evals / skills to update

- `README.md`
- `docs/product/MASTER-PLAN.md`
- `Cargo.toml` and stage notes if needed

### Concrete recommendation

Pick one explicit state and reflect it consistently:

- either council is “in-tree but not in active surface yet”
- or council is “out of tree until stage 2”

Do not leave that ambiguous.

## Finding 6 — The adapter layer is powerful, but must stay thin relative to upstream

### Related upstream capability or document

- OpenAI official tool/runtime surfaces
- Anthropic Claude Code / SDK direction
- Google ADK
- MCP as provider-neutral connector boundary

### Recommended action

**Wrap** and **compose**, but do not let adapters become a parallel provider stack.

### Why this matters

The current adapter layer is already doing useful work:

- scope guards
- orientation guards
- retry shaping
- patch/apply lane
- manual lane for self-referential reliability slices
- bounded prompt shaping for execution and drafting

That is valuable.

But this area is also the highest risk for architectural overgrowth.

SpecPunk should not become a permanent custom execution runtime that competes with vendor-native agent runtimes. The local layer should own bounded correctness and artifact discipline, not a sprawling custom provider abstraction surface.

### Real SpecPunk gap

SpecPunk needs a durable adapter contract that says exactly what belongs inside the local layer and what must remain upstream-owned.

### SpecPunk owner layer

- adapters
- shell/runtime integration boundary

### Out of scope

- a universal provider-zoo shell
- duplicating vendor-native SDK/harness semantics just for symmetry
- custom protocol invention where MCP or official tools already work

### Docs / evals / skills to update

- add `docs/product/ADAPTERS.md`
- add provider capability matrix
- add provider delta log
- add tests that separate artifact correctness from provider-specific invocation details

### Concrete recommendation

Define an adapter contract around:

- contract drafting input/output
- bounded execution input/output
- receipt shape
- failure classification
- proof-relevant metadata

Everything else should be thin wrapper territory.

## Finding 7 — Proofs are good, but not yet complete enough for full reproducibility claims

### Related upstream capability or document

- upstream emphasis on artifacts, evals, and durable handoffs

### Recommended action

**Compose** stronger reproducibility metadata into the existing proof model.

### Why this matters

The current proof system already hashes:

- contract
- receipt
- decision
- check outputs

That is good.

But a strong engineering proof for this product shape should also eventually bind:

- the exact run workspace or snapshot ref
- VCS base and change lineage
- executor identity and version
- key environment characteristics needed to replay checks

Without that, proof is still useful, but it is more like an integrity bundle than a full reproducible decision artifact.

### Real SpecPunk gap

SpecPunk lacks a first-class reproducibility boundary around run verification.

### SpecPunk owner layer

- proof
- gate
- VCS substrate

### Out of scope

- full hermetic build systems for v0
- pretending reproducibility is solved before the runtime can actually replay the context

### Docs / evals / skills to update

- proof schema docs
- gate/proof integration tests
- future replay/eval fixtures

## Finding 8 — Project overlay is promising, but ambient dependency on bus/skill paths is still transitional

### Related upstream capability or document

- repo knowledge as system of record
- project memory / project guidance ideas upstream

### Recommended action

**Refine** the project overlay into a stricter repo-owned packet.

### Why this matters

`ProjectOverlay` is the right idea.

It gives the runtime a place to unify:

- bootstrap refs
- repo guidance refs
- capability status
- safe default checks
- project skill refs

The issue is that project skill discovery currently depends on ambient filesystem conventions outside the repo shape. That may be acceptable as a bridge, but it is not the clean final answer if the repository is meant to be the system of record.

### Real SpecPunk gap

SpecPunk still needs a final policy for what project intelligence is repo-tracked vs globally cached.

### SpecPunk owner layer

- stewardship
- project overlay / skill registry boundary

### Out of scope

- hidden ambient heuristics that silently change project behavior
- silent skill mutation

### Docs / evals / skills to update

- project overlay doc
- skills doc
- bootstrap docs
- tests for overlay resolution precedence

## Priority actions

## P0 — Close the trust gap

1. remove public dependency on `punk-run` from `punk init` and auto-bootstrap paths
2. make `gate` evaluate the frozen run context rather than live repo state
3. extend `Proofpack` with explicit verification-context identity

## P1 — Close the repo truth gap

4. make stage boundaries consistent across README, master plan, workspace membership, and surface docs
5. formalize the adapter boundary so it stays thin and upstream-aligned
6. formalize project overlay ownership and skill resolution rules

## P2 — Prepare for higher layers without destabilizing the kernel

7. add replay-style tests for gate/proof reproducibility
8. add projection invariants for `WorkLedgerView`
9. add provider-delta tracking docs so architecture decisions can explicitly adopt, wrap, compose, defer, or retire as upstream evolves

## Recommended 30-day plan

### Week 1

- remove or isolate `punk-run` dependency from the public CLI path
- update docs so `punk` is truly the only operator surface
- add explicit architecture note for transitional components that still exist

### Week 2

- rework `gate` to bind verification to run workspace / snapshot identity
- update `Proofpack` to include that identity
- add tests for repo drift between `cut` and `gate`

### Week 3

- write `ADAPTERS.md`
- define which adapter responsibilities are permanent local policy vs temporary vendor-specific behavior
- add provider-delta tracking docs

### Week 4

- harden `ProjectOverlay`
- clarify skill source precedence
- add projection tests for `WorkLedgerView`, autonomy recovery, and project overlay completeness

## Final judgment

SpecPunk is directionally right.

The project already has a serious architecture and a much better product boundary than most agent tooling efforts. The main remaining work is to make the implementation obey its own laws with less leakage from transitional legacy machinery.

The correct move is not to broaden the system.

The correct move is to tighten it:

- one real CLI
- one real truth source
- one real frozen verification boundary
- one thin upstream-aligned adapter layer
- one explicit project-intelligence packet

If SpecPunk does that, it will have a strong chance of becoming the missing stewardship/correctness layer on top of upstream agent runtimes instead of just another wrapper around them.
