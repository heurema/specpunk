# punk Vision

This document defines the target product boundary.

It describes what `punk` is becoming, not the old `punk-run` world.

---

## Product thesis

`punk` is a local-first, stewarded multi-agent engineering runtime for shaping, executing, reviewing, and improving software work across codebases.

It is not a dashboard, not a SaaS control plane, and not just another chat wrapper around coding models.

---

## What `punk` is

`punk` is:

- **one CLI**
- **one runtime**
- **one vocabulary**
- **one artifact chain**
- **one stewarded project state model**

It helps a single operator:

1. define a feature and its contract
2. execute bounded implementation runs in isolated VCS context
3. verify results with deterministic checks and proof-bearing decisions
4. use structured councils where a single model is too risky
5. improve project-specific skills through evidence and evaluation

---

## What `punk` is not

`punk` is not:

- a separate product next to `arbiter` and `signum`
- a generic “AI coding assistant”
- a branch/PR manager pretending to be feature workflow
- a hosted multi-user system
- an always-on swarm shell
- a provider zoo UI

`arbiter` and `signum` are treated as **idea sources and protocol donors**, not as separate surfaces.

---

## Canonical modes

`punk` has three canonical runtime modes:

| Mode | Meaning | Default capability |
|---|---|---|
| `plot` | plan, inspect, draft, scope, research setup | read-heavy, no repo mutation |
| `cut` | implement, edit, test, produce receipt | repo mutation allowed |
| `gate` | verify, decide, prove | deterministic checks + final decision |

These modes are **hard permission boundaries**, not prompt flavor.

---

## Product laws

1. **One CLI**  
   Public surface is `punk`.

2. **One vocabulary**  
   Canonical mode IDs are `plot`, `cut`, `gate`.

3. **One state truth**  
   Runtime truth lives in an append-only event log plus materialized views.

4. **One decision writer**  
   Only `gate` writes final `DecisionObject`.

5. **Feature-centric, not PR-centric**  
   First-class unit is feature/workstream, not a branch or a single PR.

6. **Contract first**  
   `cut` should run against an approved `Contract`.

7. **Proof before acceptance**  
   A run is not done because code changed. It is done when `gate` emits a decision and proof.

8. **VCS-aware, not git-bound**  
   `jj` is preferred. `git` is fallback.

9. **Council is selective**  
   Multi-model deliberation is used where it materially improves correctness, not as a default tax on all work.

10. **Skills improve through ratchet**  
    Project-specific skills evolve through candidate proposals, eval, and promotion, not silent live mutation.

11. **Self-hosting is bounded**  
    `punk` should build itself with `punk`, but meta-level changes require stronger trust separation than ordinary feature work.

---

## Four pillars

### 1. Kernel
A small, stable Rust core with replaceable edges.

### 2. Stewardship
The runtime must care about:
- bounded scope
- cleanup completion
- removal of superseded paths
- docs/config/manifests parity
- coherent final project state

### 3. Council
The runtime must be able to convene specialized multi-model councils for high-stakes work such as:
- architecture
- contract review
- migration strategy
- cleanup-heavy changes
- final review of risky implementations

### 4. Skill/Eval ratchet
The runtime must support project-specific competence that improves through:
- run history
- failure mining
- candidate skill patches
- eval sets
- promotion or rollback decisions

---

## Layer model

`punk` has these architectural layers:

| Layer | Role |
|---|---|
| Kernel | domain artifacts, transitions, policy, event log, evaluation records |
| Stewardship | feature/contract/task/run lifecycle and project-coherence obligations |
| Council | selective structured deliberation protocols |
| Gate | deterministic validation + final decision |
| Proof | artifact bundling, hashes, reproducible output |
| VCS substrate | `jj`/`git` execution context and lineage |
| Skill/Eval | project overlays, eval loops, promotion/rollback |
| Research | bounded deep-research protocols for hard questions |

For v0, only a subset is implemented. Council, skill ratchet, and research are later layers, but they are part of the target product shape.

Repo-status vocabulary:

- **active v0 surface** = current operator/runtime path
- **in-tree but inactive** = present/buildable in the workspace, but not part of the current operator path
- **planned only** = target-shape crate not yet present in today's workspace

Canonical repo-status note: `docs/product/REPO-STATUS.md`

Detailed subsystem specs live in:
- `docs/product/COUNCIL.md`
- `docs/product/SKILLS.md`
- `docs/product/EVAL.md`
- `docs/product/RESEARCH.md`
- `docs/product/DOGFOODING.md`

---

## Canonical object chain

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

Why this matters:

- `Feature` survives across multiple attempts and replacements
- `Contract` is versioned and explicit
- `Task` is the queueable orchestration unit
- `Run` is one concrete execution attempt
- `DecisionObject` is the gate result
- `Proofpack` is the machine-readable audit bundle

Future layers may attach:
- council outputs
- eval records
- skill candidate patches
- research packets

but they do not replace the canonical artifact chain.

`Goal` is part of the target chain, but it is intentionally deferred from the v0 implemented domain/runtime until the later orchestration stage.

---

## First release scope

The first release is deliberately narrow.

In scope:

- single-repo workflow from current `cwd`
- `plot contract`
- `plot refine`
- `plot approve`
- `cut run`
- `gate run`
- `gate proof`
- `status`
- planned thin interactive shell
- append-only event log
- `jj` preferred, `git` fallback

Out of scope for the first slice:

- daemon
- global queue
- autonomous goals
- multi-model council
- diverge
- benchmark subsystem
- plugin marketplace
- skill auto-promotion
- unbounded autoresearch

---

## Design drivers

Five things shape this product:

1. **specpunk heritage**  
   orchestration, state, receipts, local-first runtime

2. **signum lessons**  
   contract-first execution, deterministic verification, proof artifacts, cleanup obligations

3. **arbiter lessons**  
   independent proposals, blind comparison, structured scoring, synthesis protocols

4. **feature-level workflow pressure**  
   feature work spans multiple iterations and must preserve non-target integrity

5. **project-specific skill pressure**  
   generic agents are not enough; project overlays, eval, and ratchet matter

This means `punk` must care about:

- explicit interfaces and behaviors in contracts
- target checks and integrity checks
- lineage across runs
- cleanup and replacement obligations
- final decision quality, not just implementation speed
- project-specific competence that improves without breaking reproducibility
