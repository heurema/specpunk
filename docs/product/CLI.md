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

Generated guidance:

- `AGENTS.md`
- `.punk/AGENT_START.md`

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

### Staged/manual intake

```bash
punk start "<goal>"
```

Behavior:

- accepts a plain goal
- drafts a contract
- prints the next explicit operator step

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

### `punk start "<goal>"`

Creates:

- `Feature`
- draft `Contract`

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
