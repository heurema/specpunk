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
- current repo truth uses one explicit status vocabulary:
  - **active v0 surface**
  - **in-tree but inactive**
  - **planned only**
- the short crate-status note lives in `docs/product/REPO-STATUS.md`
- the full crate/capability/operator-surface matrix lives in `docs/product/IMPLEMENTATION-STATUS.md`

No backward compatibility is required. The project has not launched, so the repo is being reshaped toward the cleanest final design.

## Read this first

If you are orienting in the repo or choosing the next bounded slice, read in this order:

1. `docs/product/REPO-STATUS.md`
2. `docs/product/CURRENT-ROADMAP.md`
3. `docs/product/CLI.md`
4. `docs/product/ARCHITECTURE.md`
5. `docs/product/ADR-provider-alignment.md`
6. `docs/product/IMPLEMENTATION-STATUS.md`
7. `docs/product/VISION.md`
8. `docs/product/ACTION-PLAN.md`
9. `docs/product/NORTH-ROADMAP.md`

Short version:

- `specpunk` should stay a **bounded correctness and stewardship layer**
- provider-native runtimes, tools, tracing, and session primitives should usually be **wrapped**, not rebuilt
- roadmap work that increases platform complexity without improving boundedness, reliability, inspectability, or operator simplicity should be downgraded or cut

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

`Goal` remains part of the target chain, but the current v0 domain/runtime does **not** persist a standalone `Goal` object yet. Today `punk start` and `punk go --fallback-staged` are derived shell mechanisms over plain goal text.

Key rules:
- **one CLI**: `punk`
- **one vocabulary**: `plot / cut / gate`
- **one state truth**: append-only event log + materialized views
- **one decision writer**: only `gate` writes final `DecisionObject`
- **one frozen verification context per run**: `gate` verifies against the persisted run context, not mutable live repo state
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
- **goal-only shell intake** — user describes the goal, `punk` drafts/contracts internally
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

Goal-intake commands will auto-run `git init` if the directory has no VCS yet, then continue in degraded git-only mode. If that automatic init fails, initialize the repo manually:

```bash
git init
punk init --project <id> --enable-jj --verify
```

Bootstrap is native to `punk`; it does not shell out to `punk-run`.

Bootstrap writes the current repo-local packet and guidance:

- `.punk/project.json`
- `.punk/project/capabilities.json`
- `AGENTS.md`
- `.punk/AGENT_START.md`
- `.punk/bootstrap/<project>-core.md`

If a repo already has one legacy `.punk/bootstrap/*-core.md` packet, `punk init` and `punk inspect project` reuse it instead of creating a second competing bootstrap packet.

Bootstrap also ensures safe default ignore coverage for:

- `.punk/`
- `target/`
- `.playwright-mcp/`

Successful `cut run` receipts also backfill the same safe ignore coverage if a repo was bootstrapped without a `.gitignore`.

Inspect the current derived project-intelligence view with:

```bash
punk inspect project
punk inspect project --json
```

`punk inspect project` stays concise and now points at both:

- `.punk/project/overlay.json`
- `.punk/project/capabilities.json`

Inspect the current derived work-ledger view with:

```bash
punk inspect work
punk inspect work <id>
punk inspect work <id> --json
```

Longer-term, project bootstrap should converge on one richer project-intelligence packet instead of a growing set of adjacent bootstrap artifacts.

### Default autonomous path

The default autonomous intake is goal-only:

```bash
punk go --fallback-staged "<goal>"
```

This path runs:

```text
goal -> draft -> approve -> cut -> gate -> proof
```

If the first accepted cycle only proves a controller-created bootstrap scaffold and the same goal still clearly asks for implementation work, `punk go` should immediately continue into one bounded follow-up cycle instead of stopping at the bootstrap proof. For greenfield Rust bootstrap+implementation goals, that follow-up should narrow toward the implementation files instead of rerunning the broad bootstrap prompt unchanged.

For bootstrapped greenfield Rust, Go, and Python repos, scaffold-first goals may also synthesize manifest-first checks/scope and controller-owned starter files inside `allowed_scope` so execution can begin from concrete source surfaces instead of blocking on an empty repo layout.

If autonomy blocks or escalates, `punk` prepares a staged recovery contract and returns a non-zero exit.

If the blocked or escalated proof looks like a `punk` runtime failure rather than a normal project check failure, the shell may also suggest:

```bash
punk incident capture <proof-id>
punk inspect inc_<id>
punk inspect inc_<id> --json
punk incident promote <incident-id> --repo /path/to/specpunk
punk incident promote <incident-id> --auto-run   # uses default --repo when configured
punk inspect prom_<id>
punk inspect prom_<id> --json
punk incident defaults --repo /path/to/specpunk --github owner/repo
punk incident defaults
punk incident defaults --global
punk incident defaults --global --repo /path/to/specpunk --github owner/repo
punk incident promote <incident-id>         # uses default --repo when configured
punk incident submit <incident-id>          # uses default --github when configured
punk incident submit <incident-id> --publish
punk issue admit <issue-number>             # uses default --github when configured
punk issue admit <issue-number> --publish
punk incident admit <issue-number>          # compatibility path
punk incident rerun <promotion-id> --auto-run
punk incident resubmit <submission-id> --publish
punk inspect sub_<id>
punk inspect sub_<id> --json
punk inspect adm_<id>
punk inspect adm_<id> --json
```

The current incident lane has four bounded slices:

- repo-local capture freezes an inspectable bundle under `.punk/incidents/`
- internal promote copies that bundle into another `punk` repo, drafts an inspectable upstream contract there, records the handoff under `.punk/promotions/`, and can explicitly continue with `--auto-run`
- external submit prepares a sanitized GitHub issue bundle under `.punk/submissions/` and only publishes when `--publish` is passed explicitly
- inbound admit fetches a published GitHub issue, records an inspectable admission decision under `.punk/admissions/`, and only applies labels/comments/close state when `--publish` is passed explicitly

Current GitHub publish uses `gh`, so missing auth should still leave an inspectable `sub_<id>` bundle even when publication fails.
If publish fails after prepare, retry the same submission bundle with `punk incident resubmit <submission-id> --publish` instead of generating a new issue body snapshot.
You can avoid repeating `--repo` / `--github` by configuring repo-local incident defaults under `.punk/project/incident-defaults.json` via `punk incident defaults`, or operator-wide defaults under `~/.punk/config/incident-defaults.json` via `punk incident defaults --global`.
Resolution precedence is: explicit flag > repo-local default > global default.
`punk incident capture` / `punk inspect inc_<id>` now surface the effective promote target, whether auto-run is eligible, and a setup hint when no promote target is configured.
`punk incident promote --auto-run` is still explicit opt-in; when used it auto-approves the drafted upstream contract, runs it, gates it, writes a proof, and stores that execution snapshot back onto the promotion record.
Auto-run is only suggested and permitted when the effective promote target has a matching `.punk/project.json` identity packet, an `AGENTS.md` guide that identifies `specpunk`, and the expected local `specpunk` markers (`Cargo.toml`, `crates/specpunk/src/main.rs`, `crates/punk-orch/src/lib.rs`, `docs/product/CLI.md`). Otherwise the lane stays draft-only.
If that internal auto-run fails before it reaches a proof, the promotion record now keeps the last failed phase plus any partial run/receipt/decision refs; retry the same promotion with `punk incident rerun <promotion-id> --auto-run` instead of creating a new promotion bundle.
`punk incident submit` now embeds a hidden machine-readable runtime packet in the published GitHub issue body, and `punk issue admit` is the general repo intake gate for both those runtime reports and manual backlog issues. It classifies each published GitHub issue as `admission:close-now`, `admission:defer-after-core`, or `admission:core-now`.
Deterministic runtime reports can now also classify as `core_now` without a named high-severity marker when they clearly block an active core surface instead of describing later-track work.
Only `core_now` admissions are eligible for immediate core-stabilization work intake; `defer_after_core` stays open but out of the active loop, and `close_now` is the close-now path for invalid, duplicate, obsolete, legacy-surface, or otherwise non-admissible issues.

The intended operator experience is:

- plain goal in
- one concise progress or blocker summary out
- one obvious next step out

Longer-term, blocked or escalated autonomy should also be durable and inspectable through runtime state, not only visible in one shell invocation.

`plot / cut / gate` remain available, but they are expert/control surfaces rather than the default path a normal operator must learn first.

Current reality note:

- `punk go --fallback-staged` already exists today as the default shell mechanism for initialized repos
- that does **not** mean the later standalone `Goal` primitive is already active in the v0 domain/runtime

### Staged/manual path

When you want explicit review between stages:

```bash
punk start "<goal>"
punk plot approve <contract-id>
punk cut run <contract-id>
punk gate run <run-id>
punk gate proof <run-id|decision-id>
```

For architecture-sensitive slices, `plot` can also force the deterministic review packet before approval:

```bash
punk plot contract --architecture on "<goal>"
punk plot refine <contract-id> "<guidance>" --architecture on
```

Current v0 architecture steering stays inside the same `plot -> cut -> gate` slice:

- `plot contract` / `plot refine` / `plot approve` always refresh the derived `.punk/contracts/<feature-id>/architecture-signals.json` artifact from deterministic repo scan + current contract state
- `plot` writes the derived `.punk/contracts/<feature-id>/architecture-brief.md` artifact when signals are `critical`, `--architecture on` is used, or the persisted contract already carries architecture integrity constraints
- the approved contract document remains canonical and may persist:
  - `architecture_signals_ref`
  - optional `architecture_integrity { review_required, brief_ref, touched_roots_max?, file_loc_budgets[], forbidden_path_dependencies[] }`
- `gate` reads only frozen persisted inputs, writes the derived `.punk/runs/<run-id>/architecture-assessment.json` artifact, escalates if critical review was required but missing from the approved contract, blocks on breached enforced constraints, and carries the assessment ref/hash into proof through `check_refs` / `hashes`
- enforced now: touched-root budgets, file LOC budgets, deterministic Rust crate/module edges, deterministic JS/TS relative-import edges
- deferred in v0: broader language coverage and whole-repo dependency graph analysis

If the workspace cannot auto-initialize Git, `punk start` should stop early and point back to:

```bash
git init
punk init --project <id> --enable-jj --verify
```

Read-only inspection:

```bash
punk status [id]
punk inspect work [id]
punk inspect work [id] --json
punk inspect <contract-id> --json
punk inspect <proof-id> --json
```

- `punk status [id]` is the concise lifecycle pointer: current work id, next action, suggested command, latest contract/run/decision ids
- `punk inspect work [id]` is the stable human/json view for derived architecture refs: signals summary, brief ref, assessment ref/outcome, and a copied contract-side architecture integrity summary
- `punk inspect <contract-id> --json` is the source for the full persisted contract shape, including `architecture_signals_ref` and the canonical `architecture_integrity` section when present
- `punk inspect <proof-id> --json` is the source for the final proof chain, including the hashed architecture assessment ref when present

### Bounded research expert/control surface

The current CLI already exposes a bounded research slice:

```bash
punk research start "<question>" --kind <kind> --goal "<goal>" --success "<criterion>"
punk research artifact <research-id> --kind note --summary "<summary>"
punk research synthesize <research-id> --outcome <outcome> --summary "<summary>"
punk research complete <research-id>
punk research escalate <research-id>
```

Current reality:

- this capability already lives in `specpunk` + `punk-orch` + `punk-domain`
- it is an **expert/control surface**, not the default operator path
- it freezes repo-local research packets, artifacts, synthesis, and terminal state
- it does **not** imply that a separate `punk-research` crate already exists
- worker orchestration, critique loops, and deeper research execution remain later-stage work

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

Core product docs:

- `docs/product/REPO-STATUS.md` — current repo truth and active/inactive/planned surfaces
- `docs/product/CURRENT-ROADMAP.md` — short active roadmap for current-forward work
- `docs/product/CLI.md` — command surface and shell UX
- `docs/product/ARCHITECTURE.md` — kernel, stewardship, council, skills/eval architecture
- `docs/product/ADR-provider-alignment.md` — accepted build/wrap/avoid rule for provider alignment
- `docs/product/VISION.md` — product boundary and laws
- `docs/product/ACTION-PLAN.md` — current bounded execution plan derived from the 2026-04-11 architecture review
- `docs/product/NORTH-ROADMAP.md` — durable strategic backlog and linked research tracks
- `docs/product/DOCS-SYSTEM.md` — how repo docs and public docs map together
- `docs/product/IMPLEMENTATION-STATUS.md` — canonical matrix for crate reality, capability reality, and operator-surface reality
- `docs/product/COUNCIL.md` — advisory multi-model deliberation protocols
- `docs/product/SKILLS.md` — skill packets, overlays, and candidate skill patches
- `docs/product/EVAL.md` — task eval, skill eval, and promotion decisions
- `docs/product/RESEARCH.md` — bounded deep-research protocols
- `docs/product/DOGFOODING.md` — bounded self-hosting and trust-separation rules
- `docs/product/MASTER-PLAN.md` — staged build plan
- `docs/product/CONTINUE-PROMPT.md` — handoff prompt for the next build session
- `docs/research/2026-04-11-specpunk-architecture-review.md` — review memo that drove the current action plan

Public docs layer:

- `docs.json` — Mintlify config for the public docs site
- `index.mdx` — public overview
- `install.mdx` — public install guide
- `quickstart.mdx` — public quickstart
- `roadmap.mdx` — public roadmap
- `reference/cli.mdx` — public CLI reference
- `reference/architecture.mdx` — public architecture overview
- `reference/repo-status.mdx` — public repo status overview
- `concepts/plot-cut-gate.mdx` — public plot/cut/gate concept page
- `concepts/contracts-runs-proofs.mdx` — public contracts/runs/proofs concept page

The public docs site is a curated layer over the repo. Canonical product truth still lives in the repository.

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

Also note:

- active v0 surface today: `specpunk`, `punk-domain`, `punk-events`, `punk-vcs`, `punk-core`, `punk-orch`, `punk-gate`, `punk-proof`, `punk-adapters`
- `crates/punk-council/` is **in-tree but inactive**: it stays buildable in the workspace, but is **not** part of the active v0 operator surface yet
- `punk-shell`, `punk-skills`, `punk-eval`, and `punk-research` are **planned only** as separate crates
- `punk go --fallback-staged` and `punk start` already exist today as shell mechanisms in `specpunk`, while the standalone `Goal` primitive remains deferred
- `punk research ...` commands already exist today in the active CLI/orch/domain surface, while the dedicated `punk-research` crate remains planned only
- the short crate-status note lives in `docs/product/REPO-STATUS.md`
- the full current-truth matrix lives in `docs/product/IMPLEMENTATION-STATUS.md`

## License

MIT
