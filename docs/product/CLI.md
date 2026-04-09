# punk CLI

This document defines the target command surface for the first working version.

Unless explicitly marked as planned, commands in this document refer to the current v0/v0.1 CLI surface.

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

### Primitive vs mechanism rule

The commands below are not all ontology-level primitives.

- `Project`, `Goal`, `Contract`, `Scope`, `Workspace`, `Run`, `DecisionObject`, `Proofpack`, and `Ledger` are the deeper primitives.
- shell commands such as `init`, `start`, `go`, `status`, and `inspect` are mechanisms built over those primitives.
- `plot`, `cut`, and `gate` are best understood as permission boundaries over primitive operations.

### Command-to-primitive map

| Command/mechanism | Type | Primary primitives touched |
|---|---|---|
| `punk init` | shell bootstrap | `Project`, `Ledger` |
| `punk start` | staged shell intake | `Goal`, `Contract`, `Scope` |
| `punk go --fallback-staged` | autonomous shell intake | `Goal`, `Contract`, `Run`, `DecisionObject`, `Proofpack`, `Ledger` |
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

Bootstrap should:

- pin the current repo as a known project
- create repo-local agent guidance
- verify status scope and VCS mode
- converge project-local intelligence toward one inspectable project packet over time

Generated guidance:

- `AGENTS.md`
- `.punk/AGENT_START.md`

Current project inspect surface:

- `punk inspect project`
- `punk inspect project --json`

Current `inspect project` overlay should include:

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

Current proof inspect surface:

- `punk inspect proof_<id> --json`
- `punk inspect proof_<id>`

Current human-facing inspect expectations:

- `punk inspect work` should show a concise latest-proof evidence summary when a latest proof exists
- `punk inspect work` may also show a concise latest-proof harness summary derived from the latest proof's declared and executed harness evidence
- declared harness evidence in human summaries should preserve any declared `source_ref` when the proof carries it
- `punk inspect proof_<id>` should render a concise human summary for typed `command` evidence, `declared_harness_evidence`, and executed `harness_evidence` without requiring raw JSON reading
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
- recovery-oriented status should surface durable autonomy-linked fields such as `autonomy_outcome`, `recovery_contract_ref`, and a shell-level `suggested_command`
- latest work/lifecycle/next-action data should come from the derived work view, not direct raw-event scanning

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
punk plot contract "<prompt>"
punk plot refine <contract-id> "<guidance>"
punk plot approve <contract-id>
punk cut run <contract-id>
punk gate run <run-id>
punk gate proof <run-id|decision-id>
punk status [id]
punk inspect <id> --json
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

When `--fallback-staged` is set and autonomy blocks:

- a staged recovery contract is drafted automatically
- recovery metadata and next command are returned
- an autonomy-linked durable record is written so later inspection does not depend on old shell output

The intended durable behavior is stronger than shell text:

- blocked or escalated autonomy should remain inspectable later
- the shell summary should point to durable refs
- later `status` / `inspect work` should be able to answer what happened and what comes next without relying on old terminal output

Preflight expectation:

- fail early if no Git or jj repo is detected
- return one explicit recovery path instead of a downstream adapter or scan error
- recovery should point to `git init`, then `punk init --project <id> --enable-jj --verify`, then retry the original `punk go ...`
- for a bootstrapped greenfield Rust repo with no existing inferred checks yet, a goal that explicitly asks to scaffold Rust (`rust`, `cargo`, `crate`, or `workspace` + `scaffold`/`init`/`bootstrap`) may derive an initial `cargo test` or `cargo test --workspace` intake check instead of failing at repo scan
- for a bootstrapped greenfield Go repo with no existing inferred checks yet, an explicit Go scaffold goal may derive `go test ./...` plus scaffoldable scope around `go.mod`, `cmd`, `internal`, and `pkg`
- for a bootstrapped greenfield Python repo with no existing inferred checks yet, an explicit Python scaffold goal may derive `pytest` plus scaffoldable scope around `pyproject.toml`, `src`, and `tests`
- that same greenfield Rust scaffold intake should prefer scaffoldable Rust/workspace surfaces like `Cargo.toml`, `crates`, and `tests` over existing docs/archive files when synthesizing initial scope candidates

### `punk start "<goal>"`

Runs the staged/manual intake from a plain goal.

Preflight expectation:

- fail early if no Git or jj repo is detected
- do not defer this to a later drafter or repo-scan failure
- return one explicit recovery path: `git init`, then `punk init --project <id> --enable-jj --verify`, then retry `punk start "<goal>"`
- for a bootstrapped greenfield Rust repo with no existing inferred checks yet, an explicit Rust scaffold goal may derive an initial `cargo test` or `cargo test --workspace` intake check instead of failing at repo scan
- for a bootstrapped greenfield Go repo with no existing inferred checks yet, an explicit Go scaffold goal may derive `go test ./...` and scaffoldable Go scope instead of failing at repo scan
- for a bootstrapped greenfield Python repo with no existing inferred checks yet, an explicit Python scaffold goal may derive `pytest` and scaffoldable Python scope instead of failing at repo scan
- that same greenfield Rust scaffold draft should route scope toward scaffoldable Rust/workspace surfaces instead of existing docs/archive paths

Creates:

- `Feature`
- draft `Contract`

Timeout expectation:

- if the contract drafter times out, `punk start` should attempt one deterministic bounded fallback derived from the repo scan and explicit prompt details before returning an error
- for a bootstrapped greenfield Rust scaffold goal, timeout fallback should preserve scaffoldable Rust/workspace scope (`Cargo.toml`, `crates`, optional `tests`) instead of collapsing into docs/archive candidates
- for bootstrapped greenfield Go and Python scaffold goals, timeout fallback should preserve their manifest-first scaffold scope (`go.mod`/`pyproject.toml` plus ecosystem directories) instead of collapsing into docs/archive candidates
- non-timeout drafter failures should still fail closed

Writes:

- `.punk/features/<feature-id>.json`
- `.punk/contracts/<feature-id>/v1.json`
- `feature.created`
- `contract.drafted`

### `punk plot contract "<prompt>"`

Creates:

- `Feature`
- `Contract` with status `draft`

Writes:

- `.punk/contracts/<feature-id>/v1.json`
- `feature.created`
- `contract.drafted`

### `punk plot refine <contract-id> "<guidance>"`

Updates:

- existing `Contract` with status `draft`

Timeout expectation:

- if the refine drafter times out, `punk plot refine` should attempt one deterministic fallback by reusing the current draft plus explicit guidance overrides before returning an error
- non-timeout refine failures should still fail closed

Writes:

- updated `.punk/contracts/<feature-id>/vN.json`
- `contract.refined`

### `punk plot approve <contract-id>`

Updates:

- `Contract.status = approved`

Writes:

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

- if bounded execution makes no additional edits because the approved entry points were already changed in-scope before dispatch, `cut run` may still record a successful run summary explaining that the slice was already satisfied before dispatch
- if bounded execution emits noisy progress lines but still makes no entry-point changes and never reaches meaningful compile/check progress, `cut run` may still fail closed as no-progress instead of waiting indefinitely on output noise
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

Target and integrity checks must be validated and executed as direct trusted runners, not interpolated through `/bin/sh -lc` or other shell-fragment execution.

### `punk gate proof <run-id|decision-id>`

Produces:

- `Proofpack`

Writes:

- `.punk/proofs/<decision-id>/proofpack.json`
- `proofpack.written`

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
