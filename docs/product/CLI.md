# punk CLI

This document defines the target command surface for the first working version.

Unless explicitly marked as planned, commands in this document refer to the current v0/v0.1 CLI surface.

Repo-status vocabulary used in this doc:

- **active v0 surface** = current operator/runtime path
- **in-tree but inactive** = present/buildable in workspace, but not part of the current operator path
- **planned only** = target shape, not current workspace surface

Canonical terms: `docs/product/REPO-STATUS.md`
Canonical full matrix: `docs/product/IMPLEMENTATION-STATUS.md`

---

## 1. Two surfaces

### Interactive shell

Planned for **Stage 1**. Not implemented in the current baseline.

```bash
punk
```

### Non-interactive CLI

```bash
punk <command> ...
```

Both must route into the same application services.

Status note:

- the non-interactive CLI above is **active v0 surface**
- the interactive shell is **planned only**
- council-related protocol code may exist **in-tree but inactive**, but that does not add council commands to the current CLI surface

---

## 2. Canonical modes

`punk` uses three runtime modes:

- `plot`
- `cut`
- `gate`

Prompt examples:

```text
[PLOT repo] >
[CUT repo run_01] >
[GATE repo run_01] >
```

---

## 3. Primary entrypoints

These are the preferred user-facing commands in the current v0/v0.1 surface.

### One-face operator shell contract

For initialized repos, the shell should behave as one obvious top-level interface:

- plain user goal in
- one concise progress or blocker summary out
- one obvious next step out

That means:

- the default happy path is goal-first and shell-first
- `plot`, `cut`, and `gate` remain available, but are not the first thing a normal operator should need to think about
- blocked autonomy should degrade to one explicit recovery path, not to pipeline archaeology
- if the inner loop needs another opinion, the default path should be another model/provider or a bounded council step, not a user interruption
- the user should only re-enter at a true terminal blocker or at the final result/review boundary

### Primitive vs mechanism rule

The commands below are not all ontology-level primitives.

- the long-term object chain includes `Project`, `Goal`, `Contract`, `Scope`, `Workspace`, `Run`, `DecisionObject`, `Proofpack`, and `Ledger`
- the current v0 domain/runtime does **not** yet persist a standalone `Goal` object
- `punk start` and `punk go --fallback-staged` are current shell mechanisms over plain goal text and later work-ledger projections
- shell commands such as `init`, `start`, `go`, `status`, and `inspect` are mechanisms built over those primitives.
- `plot`, `cut`, and `gate` are best understood as permission boundaries over primitive operations.

### Command-to-primitive map

| Command/mechanism | Type | Current v0 touch / target primitive relation |
|---|---|---|
| `punk init` | shell bootstrap | `Project`, `Ledger` |
| `punk start` | staged shell intake | plain goal text -> `Contract`, `Scope` today; standalone `Goal` later |
| `punk go --fallback-staged` | autonomous shell intake | plain goal text -> `Contract`, `Run`, `DecisionObject`, `Proofpack`, `Ledger` today; standalone `Goal` later |
| `punk plot ...` | substrate permission boundary | `Contract`, `Scope` |
| `punk cut run ...` | substrate permission boundary | `Workspace`, `Run`, `Receipt` |
| `punk gate ...` | substrate permission boundary | `DecisionObject`, `Proofpack` |
| `punk status` | shell read surface | `Ledger` projections |
| `punk inspect` | shell read surface | canonical artifacts + `Ledger` projections |

### Project bootstrap

```bash
punk init --enable-jj --verify
punk init --project <id> --enable-jj --verify
```

Use `--project <id>` only when the repo basename is not a suitable project id.

Bootstrap is native inside `specpunk`; the init/bootstrap path does not require `punk-run`.

Bootstrap should:

- pin the current repo as a known project
- create repo-local agent guidance
- persist the current repo-local project packet
- verify status scope and VCS mode
- converge project-local intelligence toward one inspectable project packet over time

Generated guidance:

- `.punk/project.json`
- `AGENTS.md`
- `.punk/AGENT_START.md`
- `.punk/bootstrap/<project>-core.md`

Current bootstrap migration rule:

- if the repo already contains exactly one legacy `.punk/bootstrap/*-core.md` packet, `punk init` and `punk inspect project` should reuse that packet instead of creating a competing second bootstrap doc
- if no bootstrap packet exists, `punk init` should write the native `.punk/bootstrap/<project>-core.md` file

Current project inspect surface:

- `punk inspect project`
- `punk inspect project --json`

Current `inspect project` overlay should include:

- persisted packet ref:
  - `overlay_ref`
- existing project capability summary (`bootstrap_ready`, `project_guidance_ready`, `staged_ready`, `autonomous_ready`, `jj_ready`, `proof_ready`)
- derived `harness_summary` fields from current repo state only:
  - `inspect_ready`
  - `bootable_per_workspace`
  - `ui_legible`
  - `logs_legible`
  - `metrics_legible`
  - `traces_legible`
- derived persisted harness packet fields:
  - `harness_spec_ref`
  - `harness_spec`
- explicit project skill resolution fields:
  - `project_skill_resolution_mode`
  - `project_skill_refs`
  - `ambient_project_skill_refs`

Current persisted project-intelligence packet behavior:

- `punk inspect project` writes the canonical repo-local packet to `.punk/project/overlay.json`
- `punk inspect project --json` returns the same persisted packet shape that was written to `.punk/project/overlay.json`
- the overlay packet should be sufficient to inspect bootstrap/guidance refs, current capability status, safe default checks, and active project skill refs without searching ambient directories first
- repo-local project skills resolve from `.punk/skills/overlays/**/*.md`
- external bus / ambient skill discovery is fallback-only and should surface both `project_skill_resolution_mode=ambient_fallback` and explicit `ambient_project_skill_refs`

Current persisted harness packet behavior:

- `punk inspect project` writes a derived repo-local packet to `.punk/project/harness.json`
- `punk inspect project --json` includes both `harness_summary` and the persisted `harness_spec` payload
- the current packet is still inspect-only and derived from repo markers
- derived `validation_recipes[]` now include repo-local `artifact_assertion` entries for persisted bootstrap/guidance refs when a default profile is emitted
- this slice does not add new runtime execution semantics beyond the already-supported `artifact_assertion` recipe

Current work-ledger inspect surface:

- `punk inspect work`
- `punk inspect work <id>`
- `punk inspect work <id> --json`

Architecture steering should stay visible through these same existing read surfaces:

- `punk status [id]` remains the terse lifecycle pointer
- `punk inspect work [id]` should surface the derived architecture refs and summaries for the current work item:
  - `signals_ref`
  - `brief_ref`
  - `assessment_ref`
  - signal severity / trigger summary
  - assessment outcome / summary
  - copied contract-side `architecture_integrity`
- `punk inspect <contract-id> --json` remains the canonical contract view, including persisted `architecture_signals_ref` and `architecture_integrity`
- `punk inspect <proof-id> --json` remains the final proof-chain view, including the architecture assessment ref/hash when present

Current proof inspect surface:

- `punk inspect proof_<id> --json`
- `punk inspect proof_<id>`

Current incident inspect surface:

- `punk incident defaults`
- `punk incident defaults --global`
- `punk incident defaults --repo </absolute/or/relative/path> --github owner/repo`
- `punk incident defaults --global --repo </absolute/or/relative/path> --github owner/repo`
- `punk incident capture <proof-id>`
- `punk incident promote <incident-id> [--repo </absolute/or/relative/path>]`
- `punk incident promote <incident-id> [--repo </absolute/or/relative/path>] --auto-run`
- `punk incident rerun <promotion-id> --auto-run`
- `punk incident submit <incident-id> --github owner/repo`
- `punk incident submit <incident-id> --github owner/repo --publish`
- `punk incident resubmit <submission-id> --publish`
- `punk inspect inc_<id> --json`
- `punk inspect inc_<id>`
- `punk inspect prom_<id> --json`
- `punk inspect prom_<id>`
- `punk inspect sub_<id> --json`
- `punk inspect sub_<id>`

Current promote semantics:

- `incident defaults` shows the current repo-local defaults, or updates them when `--repo` and/or `--github` is passed
- `incident defaults --global` shows the current operator-wide defaults, or updates them when `--repo` and/or `--github` is passed
- repo-local defaults persist in `.punk/project/incident-defaults.json`
- global defaults persist in `~/.punk/config/incident-defaults.json`
- target resolution precedence is explicit flag > repo-local default > global default
- once defaults exist, `incident promote` may omit `--repo` and `incident submit` may omit `--github`
- `incident capture` writes a repo-local incident bundle under `.punk/incidents/...`
- `incident capture` and `punk inspect inc_<id>` also show the effective promote target plus auto-run eligibility when a promote target is configured
- `incident promote` copies that bundle into the target repo under `.punk/imported-incidents/...`
- `incident promote` also drafts a contract in the target repo and records a durable promotion link under `.punk/promotions/...`
- plain `incident promote` stops at draft creation
- `incident promote --auto-run` is explicit opt-in and then auto-approves, executes, gates, and writes a proof for that drafted upstream contract
- auto-run is suggested and permitted only when the target repo has a matching `.punk/project.json` identity packet, an `AGENTS.md` guide that identifies `specpunk`, and the deterministic local `specpunk` markers (`Cargo.toml`, `crates/specpunk/src/main.rs`, `crates/punk-orch/src/lib.rs`, `docs/product/CLI.md`); otherwise the promotion remains draft-only
- auto-run stores the resulting `run_id`, `receipt_ref`, `decision_id`, and `proof_id` back on the promotion record so `punk inspect prom_<id>` stays inspectable
- promotion records also persist `auto_run_attempts`, `last_attempt_at`, and `last_failure` metadata so failed internal retries remain inspectable without shell scrollback
- if internal auto-run fails before proof creation, `incident rerun <promotion-id> --auto-run` reuses the same promotion record and target contract instead of creating a second promotion bundle
- `incident submit` writes a sanitized GitHub issue bundle under `.punk/submissions/...`
- `incident submit` prepares only by default; `--publish` is the explicit networked step
- publish currently uses `gh api`, so missing `gh` auth should fail after writing an inspectable `sub_<id>` bundle
- `incident resubmit` reuses an existing `.punk/submissions/...` bundle and requires `--publish`
- `incident resubmit` rejects already-published `sub_<id>` records to avoid accidental duplicate issues

Current bounded research freeze/start surface:

```bash
punk research start "<question>" \
  --kind architecture \
  --goal "<goal>" \
  --success "<criterion>" \
  [--success "<criterion>"] \
  [--constraint "<constraint>"] \
  [--subject-ref <repo-local-ref>] \
  [--context-ref <repo-local-ref>] \
  [--max-rounds 3] \
  [--max-worker-slots 5] \
  [--max-duration-minutes 30] \
  [--max-artifacts 12] \
  [--max-cost-usd 10.0]
```

Current structured research artifact surface:

```bash
punk research artifact <research-id> \
  --kind note \
  --summary "<summary>" \
  [--source-ref <repo-local-ref>]
```

Current structured research synthesis surface:

```bash
punk research synthesize <research-id> \
  --outcome risk_memo \
  --summary "<summary>" \
  [--artifact-ref <repo-local-ref>] \
  [--artifact-ref <repo-local-ref>] \
  [--follow-up-ref <repo-local-ref>] \
  [--follow-up-ref <repo-local-ref>] \
  [--replace]
```

Current research terminal-transition surface:

```bash
punk research complete <research-id>
punk research escalate <research-id>
```

Current research inspect surface:

- `punk inspect research_<id> --json`
- `punk inspect research_<id>`

Current implementation note:

- this bounded research capability already lives in `specpunk` + `punk-orch` + `punk-domain`
- it is real today even though the dedicated `punk-research` crate is still **planned only**

Current research-start semantics:

- `punk research start` is an expert/control surface, not the default user path
- this v0 slice freezes repo-local research intent only
- it writes:
  - `.punk/research/<research-id>/question.json`
  - `.punk/research/<research-id>/packet.json`
  - `.punk/research/<research-id>/record.json`
- it appends `research.started`
- it does **not** execute workers, write synthesis, invoke council, or promote anything

Current research-artifact semantics:

- `punk research artifact <research-id> ...` writes `.punk/research/<research-id>/artifacts/<artifact-id>.json`
- it updates `.punk/research/<research-id>/record.json`
- it appends `research.artifact_written`
- it advances the record state from `frozen` to `gathering`
- if a synthesized mutable current view existed, artifact writing removes `.punk/research/<research-id>/synthesis.json` so the current-view alias cannot stay stale
- when that invalidation happens, the record may keep minimal invalidation metadata for human inspect output until a new synthesis is written
- invalidation history entries remain on the record even after re-synthesis clears the active invalidation note
- it still does **not** execute workers, critique loops, or synthesis

Current research-synthesis semantics:

- `punk research synthesize <research-id> ...` writes `.punk/research/<research-id>/synthesis.json`
- the same write also persists an immutable identity copy under `.punk/research/<research-id>/syntheses/<synthesis-id>.json`
- it requires at least one previously persisted research artifact
- when no `--artifact-ref` flags are provided, it links the current full `artifact_refs[]` set
- optional `--follow-up-ref <repo-local-ref>` flags persist explicit synthesis `follow_up_refs[]`
- if a synthesis already exists, rerunning `punk research synthesize ...` requires `--replace`
- replacement appends immutable `synthesis_history_refs[]` instead of silently losing prior synthesis identity
- it updates `.punk/research/<research-id>/record.json`
- it appends `research.synthesis_written`
- it advances the record state to `synthesized` and stores `outcome` + `synthesis_ref`
- it clears any temporary invalidation note once a fresh current view exists again
- it still does **not** execute workers, close the research run, invoke council, or promote anything

Current research terminal-transition semantics:

- `punk research complete <research-id>` requires a persisted synthesis and a synthesized state
- `punk research complete <research-id>` rejects `outcome=escalate`; those runs must use `punk research escalate <research-id>`
- `punk research escalate <research-id>` requires a persisted synthesis with `outcome=escalate`
- both commands update `.punk/research/<research-id>/record.json`
- they append `research.completed` or `research.escalated`
- they mark the research run terminal so later artifact/synthesis writes are rejected

Current human-facing inspect expectations:

- `punk inspect project` should show the persisted overlay packet ref, project-skill resolution mode, active project skill refs, and any ambient fallback refs when fallback mode is active
- `punk inspect work` should show a concise latest-proof evidence summary when a latest proof exists
- `punk inspect work` may also show a concise latest-proof harness summary derived from the latest proof's declared and executed harness evidence
- `punk inspect work` should also show architecture signals / brief / assessment refs plus a concise architecture summary when those derived artifacts exist
- `punk inspect work --json` should also expose a copied `architecture.contract_integrity` object so operators can inspect the frozen architecture commitments without first opening the raw contract JSON
- declared harness evidence in human summaries should preserve any declared `source_ref` when the proof carries it
- `punk inspect proof_<id>` should render a concise human summary for typed `command` evidence, `declared_harness_evidence`, and executed `harness_evidence` without requiring raw JSON reading
- `punk inspect research_<id>` should render the frozen question, explicit budget, repo snapshot, stop rules, and the obvious next inspect command without requiring raw JSON reading
- `punk inspect research_<id>` should also show artifact count and persisted artifact refs once research notes are attached
- `punk inspect research_<id>` should also show synthesis outcome, `synthesis_ref`, and linked artifact refs once a synthesis is attached
- `punk inspect research_<id>` should also show synthesis `follow_up_refs[]` once they are attached
- `punk inspect research_<id>` should also show the current immutable synthesis identity ref and any replacement lineage once repeated synthesis writes exist
- when the run returns to `gathering` because a newer artifact invalidated the previous current view, `punk inspect research_<id>` should show an explicit invalidation note plus the invalidated synthesis ref and invalidating artifact ref
- `punk inspect research_<id>` should also show a concise invalidation history section when one or more invalidation cycles have happened
- `punk inspect research_<id>` should suggest the obvious terminal next step (`research complete` or `research escalate`) while the run is still in `state = synthesized`
- once the run is terminal, `punk inspect research_<id>` should switch the obvious next step to follow-up review when persisted follow-up refs exist
- `punk inspect research_<id> --json` should also expose a derived `invalidation` object with `active`, `latest`, and `history_count` fields for downstream tooling
- `punk inspect research_<id> --json` should also expose a derived `synthesis_lineage` object with `active`, `latest`, `history_count`, oldest-to-newest `history[]`, and convenience booleans (`has_active_current_view`, `has_replacements`, `latest_is_active`) for downstream tooling, parallel to the invalidation projection
- JSON object inspect remains the source of full structured proof detail

Current JSON artifact expectations:

- `DecisionObject` and `Proofpack` keep `command_evidence` as the executed command-check record
- they may also carry additive `declared_harness_evidence` copied from `.punk/project/harness.json`
- `declared_harness_evidence` is metadata only for non-command harness surfaces declared by the persisted packet
- they may also carry additive `harness_evidence` for executed non-command harness recipes
- the first supported executed non-command recipe is `artifact_assertion` from profile-local `validation_recipes[]`
- `artifact_assertion` verifies repo-relative artifact existence and can block `gate` on failure
- current slices still do **not** imply runtime execution of UI, log, metric, or trace recipes

Current status behavior:

- `punk status` now prefers `WorkLedgerView` for current work continuity fields
- `punk status` should stay terse; for architecture-specific refs and summaries it should point operators toward `punk inspect work [id]` / contract JSON / proof JSON instead of becoming a second full inspect surface
- recovery-oriented status should surface durable autonomy-linked fields such as `autonomy_outcome`, `recovery_contract_ref`, and a shell-level `suggested_command`
- latest work/lifecycle/next-action data should come from the derived work view, not direct raw-event scanning
- when choosing the implicit current work item, `punk status` / `punk inspect work` should prefer the feature with the latest ledger activity (`run` / `receipt` / `decision` / `proof` / autonomy record), not merely the most recently drafted feature timestamp
- stale/orphaned `running` runs with a dead executor pid, no receipt/decision/proof, and old `heartbeat.last_progress_at` should be ignored by active status/work projections instead of dominating current-work selection
- non-destructive hygiene should start with `punk gc stale --dry-run`, which reports `safe_to_archive` vs `manual_review` candidates without deleting or moving artifacts yet
- normal shell flows may autonomously quarantine `safe_to_archive` stale runs into `.punk/archive/runs/<run-id>/` so the user does not have to participate in inner-loop ledger hygiene
- autonomous stale-run quarantine must remain non-destructive: archive/move is allowed, hard delete is not
- ambiguity that only requires another technical opinion should stay inside the loop and use internal model/provider escalation rather than surfacing as a user question

Longer-term shell expectation:

- project bootstrap and shell status should keep converging on this same inspectable project-intelligence packet
- work continuity should keep converging on one inspectable work-ledger view instead of ad hoc latest-artifact inference

### Default autonomous intake

```bash
punk go --fallback-staged "<goal>"
```

Behavior:

- accepts a plain goal, not a user-written task decomposition
- drafts and approves a contract internally
- runs `cut`
- runs `gate`
- writes proof artifacts
- exits non-zero when verification blocks or escalates
- prepares a staged recovery contract when `--fallback-staged` is enabled

Default shell expectation:

- this is the normal initialized-repo path
- the operator should receive one concise shell summary
- lower-level commands should only be needed for debugging, review, or explicit control

Status note:

- `punk go --fallback-staged` is already part of the active v0 shell path today
- that does **not** mean the later standalone `Goal` primitive or dedicated `punk-shell` crate already exists

### Staged/manual intake

```bash
punk start "<goal>"
```

Behavior:

- accepts a plain goal
- drafts a contract
- prints the next explicit operator step

Use this when:

- autonomy is blocked
- exact human review is needed between stages
- an expert operator wants tighter manual control

### Mode-level commands

```bash
punk plot contract [--architecture auto|on|off] "<prompt>"
punk plot refine <contract-id> "<guidance>" [--architecture auto|on|off]
punk plot approve <contract-id>
punk cut run <contract-id>
punk gate run <run-id>
punk gate proof <run-id|decision-id>
punk gc stale --dry-run
punk status [id]
punk inspect <id> --json
punk incident defaults [--global] [--repo </absolute/or/relative/path>] [--github owner/repo]
punk incident capture <proof-id>
punk incident promote <incident-id> [--repo </absolute/or/relative/path>] [--auto-run]
punk incident rerun <promotion-id> --auto-run
punk incident submit <incident-id> [--github owner/repo] [--publish]
punk incident resubmit <submission-id> --publish
```

---

## 4. Interactive shell commands

The shell surface below is **planned**, not implemented yet.

Mode switches:

```text
:plot
:cut
:gate
```

Context:

```text
:use <id>
:status
:q
```

Plain text behavior:

- in `plot`: draft/refine contract for active feature
- in `cut`: execute against active approved contract
- in `gate`: review and decide active run

---

## 5. Minimal command semantics

### `punk init [--project <id>] --enable-jj --verify`

Bootstraps the current repo for `punk`.

Creates or refreshes:

- project pin / resolver entry
- bootstrap guidance
- repo-local agent instructions
- default `.gitignore` coverage for `.punk/`, `target/`, and `.playwright-mcp/`
- successful `cut run` receipts should also backfill the same safe `.gitignore` coverage without surfacing `.gitignore` itself as bounded user work

Verifies:

- resolved project id
- status scope
- VCS mode

### `punk go [--fallback-staged] "<goal>"`

Runs the autonomous path from a plain goal.

Internally this executes:

- `plot contract`
- internal approval
- `cut run`
- `gate run`
- `gate proof`

Returns:

- concise human summary with outcome / basis / proof
- structured JSON when `--json` is requested
- non-zero exit for `block` or `escalate`

Auto-chain expectation:

- if the first accepted cycle only proves a controller-created bootstrap scaffold
- and the original goal still clearly asks for implementation work (`implement`, `add`, `support`, `wire`, or `with tests`)
- `punk go` should immediately run one bounded follow-up cycle instead of stopping at the bootstrap proof
- for greenfield Rust bootstrap+implementation goals, that follow-up cycle should narrow toward implementation files like `crates/<app>-cli/src/main.rs`, `crates/<app>-core/src/lib.rs`, and `tests`, rather than reusing the original broad bootstrap prompt unchanged
- if that narrowed follow-up goal declares an exact touch set, later proposal repair must not re-expand it back to bootstrap scope (`Cargo.toml`, `crates`, etc.)
- timeout bootstrap fallback must only preserve scaffold scope for prompts that still actually request scaffold/bootstrap work; rich follow-up implementation prompts on an already bootstrapped repo should stay file-bounded
- timeout fallback must also preserve explicit implementation semantics from the prompt (`--json`, `--force`, `--project-root`, named starter files) instead of collapsing them into a generic bounded slice
- the final shell summary / JSON payload should report the follow-up cycle as the main result and keep the bootstrap cycle as auxiliary context

When `--fallback-staged` is set and autonomy blocks:

- a staged recovery contract is drafted automatically
- recovery metadata and next command are returned
- an autonomy-linked durable record is written so later inspection does not depend on old shell output
- when the blocked or escalated proof matches deterministic runtime-bug markers (`no-progress`, corruption, orphan/stall/timeout, or controller/executor unexpected state), the shell may also return `punk incident capture <proof-id>`
- after capture, operators can explicitly hand the bundle upstream with `punk incident promote <incident-id> --repo <specpunk-repo>`
- if they already trust the drafted upstream contract, `punk incident promote <incident-id> --auto-run` continues all the way through `approve -> cut -> gate -> proof` in the target repo and records those execution artifacts back onto `prom_<id>`
- that `--auto-run` path is only suggested and permitted when the effective promote target has a matching identity packet plus the expected local `specpunk` files; otherwise the shell should keep the lane draft-only
- if that internal auto-run fails partway through, `punk incident rerun <promotion-id> --auto-run` retries the same promoted contract from its current target-repo state instead of drafting a fresh promotion
- `punk inspect prom_<id>` should show whether the promotion is still `drafted`, has a `last_failure`, or already has a completed execution, plus the next obvious recovery command
- external users can instead prepare a GitHub issue with `punk incident submit <incident-id> --github owner/repo`
- repo-local defaults from `punk incident defaults` and global defaults from `punk incident defaults --global` can remove those repeated flags; resolution precedence is explicit flag > repo-local > global
- actual GitHub publication requires explicit `--publish`
- plain promote copies the incident bundle into the target repo, drafts an inspectable contract there, and does not auto-approve or auto-run anything
- submit writes a sanitized `.punk/submissions/...` bundle first so failed publication still leaves something inspectable
- resubmit lets the operator retry the exact same prepared submission bundle after fixing `gh` auth or network issues

The intended durable behavior is stronger than shell text:

- blocked or escalated autonomy should remain inspectable later
- the shell summary should point to durable refs
- later `status` / `inspect work` should be able to answer what happened and what comes next without relying on old terminal output

Preflight expectation:

- if no Git or jj repo is detected, `punk go` should first auto-run `git init` in place and continue in degraded git-only mode instead of failing before intake
- if that automatic `git init` fails, return one explicit recovery path instead of a downstream adapter or scan error
- recovery should point to `git init`, then `punk init --project <id> --enable-jj --verify`, then retry the original `punk go ...`
- for a bootstrapped greenfield Rust repo with no existing inferred checks yet, a goal that explicitly asks to scaffold Rust (`rust`, `cargo`, `crate`, or `workspace` + `scaffold`/`init`/`bootstrap`) may derive an initial `cargo test` or `cargo test --workspace` intake check instead of failing at repo scan
- for a bootstrapped greenfield Go repo with no existing inferred checks yet, an explicit Go scaffold goal may derive `go test ./...` plus scaffoldable scope around `go.mod`, `cmd`, `internal`, and `pkg`
- for a bootstrapped greenfield Python repo with no existing inferred checks yet, an explicit Python scaffold goal may derive `pytest` plus scaffoldable scope around `pyproject.toml`, `src`, and `tests`
- for a bootstrapped greenfield TypeScript/Node repo with no existing `package.json` or inferred checks yet, an explicit TS/Node scaffold goal may derive `npm test` plus scaffoldable scope around `package.json`, optional `tsconfig.json`, and `src`/`tests` (or workspace-style `packages`/`apps`)
- that same greenfield Rust scaffold intake should prefer scaffoldable Rust/workspace surfaces like `Cargo.toml`, `crates`, and `tests` over existing docs/archive files when synthesizing initial scope candidates
- if the repo root has no explicit integrity story but nested manifests/scripts do, `punk go` may infer trustworthy intake checks from nested `Cargo.toml`, nested `package.json` scripts, or nested `Makefile` `test` targets instead of failing immediately at repo scan

### `punk start "<goal>"`

Runs the staged/manual intake from a plain goal.

Preflight expectation:

- if no Git or jj repo is detected, `punk start` should first auto-run `git init` in place and continue in degraded git-only mode instead of failing before intake
- do not defer missing-VCS handling to a later drafter or repo-scan failure
- if that automatic `git init` fails, return one explicit recovery path: `git init`, then `punk init --project <id> --enable-jj --verify`, then retry `punk start "<goal>"`
- for a bootstrapped greenfield Rust repo with no existing inferred checks yet, an explicit Rust scaffold goal may derive an initial `cargo test` or `cargo test --workspace` intake check instead of failing at repo scan
- for a bootstrapped greenfield Go repo with no existing inferred checks yet, an explicit Go scaffold goal may derive `go test ./...` and scaffoldable Go scope instead of failing at repo scan
- for a bootstrapped greenfield Python repo with no existing inferred checks yet, an explicit Python scaffold goal may derive `pytest` and scaffoldable Python scope instead of failing at repo scan
- for a bootstrapped greenfield TypeScript/Node repo with no existing `package.json`, an explicit TS/Node scaffold goal may derive `npm test` and manifest-first scaffold scope instead of failing at repo scan
- that same greenfield Rust scaffold draft should route scope toward scaffoldable Rust/workspace surfaces instead of existing docs/archive paths
- if an explicit bootstrap prompt names nested scaffold touch targets under a missing scaffold root (for example `crates/pubpunk-cli` before `crates/` exists), the draft should still preserve those repo-relative paths in `allowed_scope` instead of collapsing back to only the root manifest
- when a plain Rust bootstrap goal also mentions implementation semantics like `pubpunk init`, controller scaffold member inference should prefer the goal-derived app slug (`pubpunk-cli` / `pubpunk-core`) instead of article-shaped placeholders like `a-cli` / `a-core`
- if the repo root has no explicit integrity story but nested manifests/scripts do, `punk start` may infer trustworthy intake checks from nested `Cargo.toml`, nested `package.json` scripts, or nested `Makefile` `test` targets instead of failing immediately at repo scan
- when those nested-manifest fallbacks are active and the prompt clearly looks backend/data-oriented (`db`, `session`, `seed`, `service`, `dispatch`, etc.), candidate targeting should bias away from obvious UI/generated surfaces like `.astro`, `astro.config.*`, `dist`, and `packs`, and prefer source-first anchors such as `package.json`, `drizzle.config.*`, `src/lib/db/*`, `src/lib/persistence/*`, and `src/actions/*`
- for backend service/session/runtime prompts on nested Node repos, candidate augmentation should also surface `src/lib/services/*`, `src/lib/session/*`, and `src/pages/api/*`, and `.astro` pages like `dispatch.astro` must not count as backend anchors just because their path contains words like `dispatch`
- for backend-only service/session prompts on baseline-style repos, scope normalization should preserve those Node backend anchors in `entry_points`/`allowed_scope`, drop incidental root `package.json` file anchors from the final contract, and prefer grounded Node checks such as root-level `npm run check` plus optional `npm run build:web` when wrapper scripts exist (otherwise fall back to nested package commands), instead of collapsing back to a Rust-only file scope or `cargo test`
- when a draft/refine prompt explicitly says to exclude or not touch generated/runtime paths, those excluded prefixes must be pruned back out of persisted `allowed_scope` and `entry_points` instead of leaking in as candidate scope
- when a backend/service prompt clearly spans mixed Node+Rust surfaces (for example `session`, `dispatch`, `handoff`, `service`, `api` plus both `package.json` and `Cargo.toml`/`crates` candidates), scope normalization should preserve a small mixed-service envelope instead of collapsing the contract into a single TS file path
- for mixed Node+Rust service/session/runtime prompts, candidate augmentation should surface backend anchors like `src/lib/services/*`, `src/lib/session/*`, `src/pages/api/*`, and CLI crate manifests/main files, treat `src/pages/api/**` as backend routes rather than UI pages, replace `.astro` page anchors like `report.astro` with backend Node surfaces when backend imports are already present, drop incidental root `package.json` file anchors from the final contract, and prefer lighter mixed checks such as `cargo check -p <cli-crate>`, root-level `npm run check`, and optional `npm run build:web` when the repo exposes those wrappers

Creates:

- `Feature`
- draft `Contract`

Timeout expectation:

- if the contract drafter times out, `punk start` should attempt one deterministic bounded fallback derived from the repo scan and explicit prompt details before returning an error
- if the drafter times out with no captured draft-JSON progress (including silent timeouts and MCP-only chatter), `punk start` should skip compact retry and fall back immediately instead of spending extra time on another blind attempt
- for a bootstrapped greenfield Rust scaffold goal, timeout fallback should preserve scaffoldable Rust/workspace scope (`Cargo.toml`, `crates`, optional `tests`) instead of collapsing into docs/archive candidates
- bounded fallback repair for a plain Rust bootstrap+implementation goal should also preserve scaffoldable workspace scope (`Cargo.toml`, `crates`, optional `tests`) instead of collapsing to manifest-only/file-only recovery that cannot materialize the requested CLI crates
- that same greenfield scaffold-scope preservation must survive later prompt canonicalization and plain prompt mentions like `tests`, so a natural Rust bootstrap+init goal cannot collapse back to `Cargo.toml` + `tests` after repair
- for bootstrapped greenfield Go and Python scaffold goals, timeout fallback should preserve their manifest-first scaffold scope (`go.mod`/`pyproject.toml` plus ecosystem directories) instead of collapsing into docs/archive candidates
- for a bootstrapped greenfield TypeScript/Node scaffold goal, timeout fallback should preserve manifest-first scaffold scope (`package.json`, optional `tsconfig.json`, plus `src`/`tests` or workspace directories) instead of collapsing into docs/archive candidates
- that same timeout fallback should keep scaffold-oriented `expected_interfaces` / `behavior_requirements` derived from the prompt and manifest kind instead of generic `approve-ready bounded contract` placeholder text
- when timeout fallback keeps or recovers file-level `entry_points`, it must re-add any missing entry points into `allowed_scope` before validation so recovery does not fail on self-inconsistent scope coverage
- the same entry-point coverage repair should also apply to normal draft/refine bounded-repair paths, so intake does not fail solely because a drafter returned a self-inconsistent `entry_points` vs `allowed_scope` shape
- non-timeout drafter failures should still fail closed

Writes:

- `.punk/features/<feature-id>.json`
- `.punk/contracts/<feature-id>/v1.json`
- `feature.created`
- `contract.drafted`

### `punk plot contract [--architecture auto|on|off] "<prompt>"`

Creates:

- `Feature`
- `Contract` with status `draft`

Writes:

- `.punk/contracts/<feature-id>/v1.json`
- `.punk/contracts/<feature-id>/architecture-signals.json`
- `feature.created`
- `contract.drafted`

Architecture steering notes:

- `plot contract` always writes a deterministic `architecture-signals.json` artifact next to the contract
- one feature/run layout should stay explicit:
  - canonical contract document: `.punk/contracts/<feature-id>/vN.json`
  - derived signals artifact: `.punk/contracts/<feature-id>/architecture-signals.json`
  - derived brief artifact: `.punk/contracts/<feature-id>/architecture-brief.md`
  - derived gate assessment artifact: `.punk/runs/<run-id>/architecture-assessment.json`
- default thresholds live in code and docs:
  - `warn_file_loc >= 600`
  - `critical_file_loc >= 1200`
  - `critical_scope_roots > 1`
  - `warn_expected_interfaces > 2`
  - `warn_import_paths > 5`
- if signals are `critical`, `--architecture on` is used, or the draft already carries architecture integrity constraints, `plot contract` must also write `.punk/contracts/<feature-id>/architecture-brief.md`
- that brief is deterministic/template-populated, not an LLM-authored freeform artifact
- the persisted contract document may also carry:
  - `architecture_signals_ref`
  - optional `architecture_integrity` with:
    - `review_required`
    - `brief_ref`
    - `touched_roots_max`
    - `file_loc_budgets[]`
    - optional `forbidden_path_dependencies[]` (deterministically enforced in `gate run` for touched matching files when direct local imports can be resolved; otherwise the assessment stays `Unverified`)

### `punk plot refine <contract-id> "<guidance>" [--architecture auto|on|off]`

Updates:

- existing `Contract` with status `draft`

Timeout expectation:

- if the refine drafter times out, `punk plot refine` should attempt one deterministic fallback by reusing the current draft plus explicit guidance overrides before returning an error
- non-timeout refine failures should still fail closed

Writes:

- updated `.punk/contracts/<feature-id>/vN.json`
- refreshed `.punk/contracts/<feature-id>/architecture-signals.json`
- refreshed `.punk/contracts/<feature-id>/architecture-brief.md` when architecture review remains active
- `contract.refined`

### `punk plot approve <contract-id>`

Updates:

- `Contract.status = approved`

Writes:

- refreshed `.punk/contracts/<feature-id>/architecture-signals.json`
- refreshed `.punk/contracts/<feature-id>/architecture-brief.md` when architecture review is active
- `contract.approved`

### `punk cut run <contract-id>`

Creates:

- `Task`
- `Run`
- `Receipt`

Uses:

- `jj` preferred
- `git` fallback

Writes:

- `task.queued`
- `task.claimed`
- `run.started`
- `receipt.written`
- `run.finished`

Behavior notes:

- if bounded execution makes no additional edits because a file-bounded slice's approved entry points were already changed in-scope before dispatch, `cut run` may still record a successful run summary explaining that the slice was already satisfied before dispatch; directory-scoped implementation contracts must still fail honestly as no-progress
- if bounded execution emits noisy progress lines but still makes no entry-point changes and never reaches meaningful compile/check progress, `cut run` may still fail closed as no-progress instead of waiting indefinitely on output noise
- if file-level entry points like `Cargo.toml`, `go.mod`, `pyproject.toml`, or `package.json` were missing at baseline and remain missing after bounded dispatch, `cut run` should still fail closed as no-progress instead of hanging indefinitely on executor noise
- manifest-only greenfield bootstrap runs should use a short executor timeout budget instead of waiting for the full generic execution timeout before normalizing to blocked/no-progress
- if that same greenfield missing-manifest bootstrap case later degrades into timeout, stall, or orphan classification, `cut run` should still collapse it into the same deterministic blocked summary instead of flapping between blocked and stall-style failures
- for an approved Rust workspace bootstrap contract that explicitly names crate directories under `crates/...`, `cut run` may materialize a minimal controller-owned workspace scaffold inside `allowed_scope` before dispatch so execution starts from concrete files instead of immediately blocking on an empty layout
- for generic Rust workspace bootstrap contracts whose scope is only `Cargo.toml`, `crates`, and `tests`, `cut run` may infer bounded crate members from the contract semantics (for example a named CLI like `pubpunk`) and materialize the corresponding minimal workspace scaffold inside `allowed_scope`
- for a manifest-only greenfield Go bootstrap contract whose scope includes `go.mod` plus `cmd`/`internal`/`pkg`, `cut run` may materialize a minimal controller-owned Go module scaffold inside `allowed_scope` so `go test ./...` starts from concrete files instead of an empty module root
- for a manifest-only greenfield Python bootstrap contract whose scope includes `pyproject.toml`, `src`, and `tests`, `cut run` may materialize a minimal controller-owned package scaffold inside `allowed_scope` so `pytest` starts from concrete package/test files instead of an empty project root
- when cargo-based bounded execution would generate a new `Cargo.lock` outside `allowed_scope`, `cut run` should prune that generated side effect before writing the receipt so the project is not left with avoidable out-of-scope dirt, even for file-scoped follow-up Rust slices
- when bounded execution only performs inspection/check commands inside allowed scope, makes no product-file changes, and then stalls without emitting a sentinel, `cut run` should normalize that case into deterministic no-progress failure instead of surfacing raw noisy stall tails like `mcp: ...` or `succeeded in ...`
- executor prompts should include the original approved goal text alongside condensed behavior requirements so bounded slices do not lose critical implementation details when drafter requirements are abbreviated
- when `allowed_scope` is directory-scoped but bounded, executor prompts should also enumerate the current in-scope files available for direct edit so the model does not stall on ambiguous directory-level scope
- executor may narrow a small directory-scoped bounded contract to the currently existing in-scope files for execution only, which allows fail-closed context packing and patch-apply routing without mutating the persisted approved contract
- for recurring bounded product slices with stable three-file topology (for example a Rust `core + cli + tests` init slice), execution may inject controller-owned recipe/patch seeds so patch/apply starts from concrete edits instead of stalling on the full contract prose
- for the stable bootstrapped `pubpunk init` recipe (`crates/pubpunk-core`, `crates/pubpunk-cli`, `tests`), `cut run` may short-circuit through a controller-owned implementation template before verification instead of waiting on repeated no-progress patch/apply attempts
- that controller-owned `pubpunk init` template should materialize the canonical `.pubpunk` tree (`style/examples`, `targets`, `review`, `lint`, `local/{drafts,reports,cache,generated}`), keep `--json` / `--force` / `--project-root` wiring intact, and write the exact starter `project.toml` shape required by the richer completion contract
- for the stable follow-up `pubpunk` cleanup slice that only removes obsolete `style/examples` references from `crates/pubpunk-core/src/lib.rs` and `tests/init_json.rs`, `cut run` may use a controller-owned cleanup template so the bounded slice completes directly instead of repeatedly failing patch/apply hunks
- for the stable `pubpunk validate --json --project-root` slice over `crates/pubpunk-core`, `crates/pubpunk-cli`, and `tests/validate_json.rs`, `cut run` may use a controller-owned validate template so execution materializes deterministic JSON validation/reporting coverage instead of stalling on missing test context
- for the exact follow-up `pubpunk` core-only validate parseability helper slice over `crates/pubpunk-core/src/lib.rs`, `cut run` may use a controller-owned recipe that keeps the `validate_report` JSON envelope unchanged while making unsupported `style/targets/review/lint` `project.toml` inputs surface as explicit issues, instead of stalling on missing authoritative helper snippets or broadening scope into CLI/Cargo surfaces
- for the exact follow-up `pubpunk` validate parse-check extension over `crates/pubpunk-core/src/lib.rs` plus `tests/validate_json.rs`, `cut run` may use a controller-owned recipe that adds file-level TOML parseability checks for `.pubpunk/style/*.toml` and direct `targets/review/lint/*.toml` files, along with the two bounded JSON tests, instead of collapsing the contract to core-only scope and stalling as no-progress
- if a bounded follow-up Rust slice allows `tests` but the current repo only has placeholder files like `tests/README.md`, execution may synthesize one concrete in-scope test entry point (for example `tests/init_json.rs`) so patch/apply can create real coverage without widening the approved persisted contract
- whenever a bounded Rust slice narrows a `tests` directory into concrete file paths, execution should drop placeholder-only test files from the narrowed execution scope so the bounded slice stays patch/apply-sized instead of tipping into the noisier general exec lane
- if a bounded patch/apply Rust slice exposes sequential failed checks (for example one compile error in `cargo test -p <crate>` and then a later failure in `cargo test --workspace`), `cut run` may spend one bounded repair pass per newly exposed failed check, capped at three total patch/apply passes
- if a bounded patch/apply slice emits prompt/setup text and then goes silent without producing a patch, `cut run` should treat that as a no-output stall, spend at most one bounded retry on it, and then collapse unchanged entry points back into deterministic no-progress instead of waiting for the full raw timeout repeatedly
- if patch/apply partially mutates multiple files and a later hunk fails or validation detects that a previously non-empty source file became zero-byte, `cut run` must restore the original contents of every touched file and surface a blocked corruption summary instead of leaving the repo in a damaged state
- if a blocked or failed patch/apply attempt damages previously non-empty entry-point files outside the final validated patch (for example by leaving them zero-byte or missing), `cut run` must restore those original entry-point contents before returning the blocked result
- if a bounded patch/apply slice applies edits on one pass and later retries or verification still fail, `cut run` must roll those bounded edits back to the pre-run entry-point snapshots instead of leaving the repo partially patched and red
- if entry-point test-boundary masking is active and an execution attempt empties or deletes the masked head, stale-mask restoration must still reconstruct the original file instead of appending only the preserved test tail
- `already satisfied in allowed scope before bounded dispatch` is only valid for deterministic file-bounded no-progress slices; blocked summaries must stay blocked and must not be upgraded to success
- if a bounded implementation run reports `PUNK_EXECUTION_COMPLETE` but the observed repo change set is still empty, `cut run` must normalize that into deterministic no-progress/failure instead of writing a false success receipt
- if `cut run` executes inside an isolated git worktree and produces product-file changes, those in-scope file edits must be synced back into the main repo root before the receipt is written so later `gate` / `proof` phases and the operator-visible worktree see the same result
- if `cut run` executes inside an isolated git worktree while the main repo root already contains uncommitted product files from an earlier stage (for example bootstrap-created manifests or sources), those present repo-root files must be copied into the isolated workspace before execution so follow-up bounded slices see the same baseline instead of starting from bare `HEAD`
- if a VCS backend reports the same path for `repo_root` and `workspace_ref` (for example `jj` operating in-place), pre-run and post-run file sync must skip self-copies instead of copying changed files onto themselves and corrupting the working copy
- if `cut run` needs an isolated git workspace in degraded git-only mode on a repo that has no committed `HEAD` yet, it should create an unborn isolated branch/worktree instead of failing with `invalid reference: HEAD`
- `gate` and `proof` remain authoritative; this cut-time success does not replace verification

### `punk gate run <run-id>`

Reads:

- approved contract
- receipt

Produces:

- `DecisionObject`

Checks:

- scope
- policy
- target checks
- integrity checks
- architecture assessment

Behavior notes:

- `gate run` must never accept a run whose receipt status is not `success`, even if trusted target and integrity checks happen to pass afterward
- `gate run` must also block a bounded implementation receipt that claims success while reporting no observable repo changes, unless the receipt explicitly says the slice was already satisfied before bounded dispatch
- controller-owned runtime artifacts written under `.punk/runs/<run-id>/...` should not count as user scope violations during `gate run`; scope validation should judge only repo changes attributable to the bounded work itself
- `cut run` should persist a canonical verification context for the run and record its ref on `Run` before `gate run` starts
- `gate run` must validate that persisted verification context before running trusted checks and fail closed if the context is missing, unreadable, or drifted
- when a run executed inside an isolated VCS workspace (for example a git worktree in degraded git-only mode), `gate run` must execute trusted target and integrity checks inside that recorded `workspace_ref`, not back on the original repo root
- if `gate run` executes cargo-based trusted checks for a contract whose scope does not include `Cargo.lock`, a newly generated `Cargo.lock` should be pruned after the check rather than left behind as avoidable project litter
- `gate run` must read only frozen persisted architecture inputs:
  - approved contract document
  - persisted `architecture-signals.json`
  - receipt / verification context / trusted check outputs
- `gate run` must write `.punk/runs/<run-id>/architecture-assessment.json` before serializing the final decision; that assessment stays derived while `DecisionObject` and `Proofpack` remain canonical
- if architecture signals are `critical` but the approved contract document has no `architecture_integrity` section, `gate run` must return `Escalate` and record an explicit machine-readable reason in `.punk/runs/<run-id>/architecture-assessment.json`
- if enforced architecture constraints are present and any are breached (for example `touched_roots_max` or `file_loc_budgets[]`), `gate run` must return `Block`
- if enforced `forbidden_path_dependencies[]` are present, `gate run` must deterministically scan touched matching files for direct local dependency edges and return `Block` on a violated edge
- v0 forbidden dependency enforcement is intentionally cheap: it currently covers deterministic Rust crate/module references plus JS/TS relative imports; if matching touched files are outside those supported forms, the assessment must report `Unverified` instead of pretending the rule passed

Target and integrity checks must be validated and executed as direct trusted runners, not interpolated through `/bin/sh -lc` or other shell-fragment execution.

### `punk gate proof <run-id|decision-id>`

Produces:

- `Proofpack`

Writes:

- `.punk/proofs/<decision-id>/proofpack.json`
- `proofpack.written`

Behavior notes:

- `Proofpack` should carry the same `verification_context_ref` and execution-context identity that `gate run` used for the final decision
- when the referenced verification context artifact still exists, `Proofpack` should hash it alongside the contract, receipt, decision, and check outputs
- `Proofpack` must also carry the architecture assessment ref/hash written by `gate run` (currently through `check_refs` / `hashes`)
- `Proofpack` should also persist:
  - `run_ref`
  - `workspace_lineage`
  - `executor_identity`
  - `reproducibility_claim`
- current reproducibility claim levels are:
  - `frozen_context_v0`
  - `run_record_v0`
  - `record_plus_context_v0`
  - `record_only_v0`
- these levels are intentionally bounded: v0 proof distinguishes recorded evidence from partially reconstructable verdict context, but does **not** claim hermetic rebuilds

---

## 6. CLI rules

1. `cut` should refuse unapproved contracts.
2. `gate` is the only path that writes final decision artifacts.
3. `status` must reconstruct state from the event log, not from ad-hoc file scanning.
4. All inspectable commands should support structured output.
5. Plain user goals should route through `punk go --fallback-staged "<goal>"` by default.
6. `punk start "<goal>"` remains the staged/manual escape hatch.
7. New command proposals should state whether they introduce a new primitive or only a new derived mechanism; the default expectation is derived mechanism.
8. Shell-facing commands should minimize reading burden by returning one concise progress or blocker summary plus one obvious next step.

---

## 7. Deliberately postponed commands

Not part of the first working version:

- `goal`
- `queue`
- `daemon`
- `morning`
- `ask`
- `panel`
- `quorum`
- `verify`
- `diverge`
- benchmark commands
