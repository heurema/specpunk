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

## 3. First command set

### Plot

```bash
punk plot contract "<prompt>"
punk plot refine <contract-id> "<guidance>"
punk plot approve <contract-id>
```

### Cut

```bash
punk cut run <contract-id>
```

### Gate

```bash
punk gate run <run-id>
punk gate proof <run-id|decision-id>
```

### Read-only inspection

```bash
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
