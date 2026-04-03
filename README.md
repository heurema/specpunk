# punk

Local-first, stewarded multi-agent engineering runtime.

`punk` is the new target shape of this repo: one CLI, one vocabulary, one runtime.

It combines:
- **orchestration** from specpunk
- **deliberation** ideas from arbiter
- **assurance / proof** ideas from signum
- **modal shell UX** inspired by forgecode
- **skill/eval ratchet** direction informed by project-skill work such as skillpulse-style tracking

## Status

This repo is in a **design reset / rebuild** phase.

- current code still contains legacy nested workspace pieces under `punk/`
- those crates are treated as **source material for extraction**
- docs in `docs/product/` describe the **target architecture**, not a finished released implementation

No backward compatibility is required. The project has not launched, so the repo is being reshaped toward the cleanest final design.

## Product thesis

`punk` is a local-first runtime for AI-driven engineering work across repositories.

It has four pillars:
- **Kernel** — small, stable Rust core with replaceable edges
- **Stewardship** — goal, scope, cleanup, docs, and project coherence
- **Council** — selective multi-model, multi-role deliberation for high-stakes decisions
- **Skill/Eval ratchet** — project-specific skills improved through evidence and evaluation

It is built around three runtime modes:
- **`plot`** — shape work, inspect the repo, draft/refine contracts
- **`cut`** — execute bounded changes in isolated VCS context
- **`gate`** — verify, decide, and produce proof artifacts

These are not tone presets. They are **permission boundaries**.

## Core model

Canonical object chain:

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

Key rules:
- **one CLI**: `punk`
- **one vocabulary**: `plot / cut / gate`
- **one state truth**: append-only event log + materialized views
- **one decision writer**: only `gate` writes final `DecisionObject`
- **VCS-aware, not git-bound**: `jj` preferred, `git` fallback
- **feature-centric, not PR-centric**
- **skills improve through curated ratchet, not silent mutation**

## What makes `punk` different

`punk` is not just another agent runner.

It is meant to ensure AI agents do not merely write code, but leave the project in a cleaner and more coherent state:
- bounded scope
- superseded code removed or explicitly retained
- docs/config/manifests updated
- migrations actually finished
- high-stakes decisions reviewed through structured council protocols when needed

## Current practical flow

The current usable flow is split into:

- **project bootstrap** — explicit admin action
- **goal-only intake** — user describes the goal, `punk` drafts/contracts internally
- **mode-level control** — `plot / cut / gate` remain available when you want manual staging

### Project bootstrap

Bootstrap a repo once:

```bash
punk init --enable-jj --verify
```

If the repo basename is not a good project id, use:

```bash
punk init --project <id> --enable-jj --verify
```

Bootstrap creates repo-local agent guidance:

- `AGENTS.md`
- `.punk/AGENT_START.md`

### Default autonomous path

The default autonomous intake is goal-only:

```bash
punk go --fallback-staged "<goal>"
```

This path runs:

```text
goal -> draft -> approve -> cut -> gate -> proof
```

If autonomy blocks or escalates, `punk` prepares a staged recovery contract and returns a non-zero exit.

### Staged/manual path

When you want explicit review between stages:

```bash
punk start "<goal>"
punk plot approve <contract-id>
punk cut run <contract-id>
punk gate run <run-id>
punk gate proof <run-id|decision-id>
```

Read-only inspection:

```bash
punk status [id]
punk inspect <id> --json
```

What is explicitly out of scope for v0:
- daemon
- queue
- goals as user-facing flow
- council (`panel / quorum / verify`)
- diverge
- benchmark subsystem
- plugin marketplace
- skill auto-promotion

## Docs

- `docs/product/VISION.md` — product boundary and laws
- `docs/product/ARCHITECTURE.md` — kernel, stewardship, council, skills/eval architecture
- `docs/product/COUNCIL.md` — advisory multi-model deliberation protocols
- `docs/product/SKILLS.md` — skill packets, overlays, and candidate skill patches
- `docs/product/EVAL.md` — task eval, skill eval, and promotion decisions
- `docs/product/RESEARCH.md` — bounded deep-research protocols
- `docs/product/DOGFOODING.md` — bounded self-hosting and trust-separation rules
- `docs/product/CLI.md` — command surface and shell UX
- `docs/product/NORTH-ROADMAP.md` — durable strategic backlog and linked research tracks
- `docs/product/MASTER-PLAN.md` — staged build plan
- `docs/product/CONTINUE-PROMPT.md` — handoff prompt for the next build session

## Target repo shape

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

## Current practical note

Today the repo still has a legacy Rust workspace inside `punk/`.

That is **not** the target architecture anymore.

It should be treated as:
- code to extract
- code to relocate
- code to delete when replaced

## License

MIT
