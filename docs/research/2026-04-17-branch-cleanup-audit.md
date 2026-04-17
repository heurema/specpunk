# 2026-04-17 branch cleanup audit

## Goal

Collapse local feature work into `main`, remove stale topic branches, and leave a clean repo with one durable branch.

## Merged into main

### Pubpunk target-instance guards

Merged from `codex/core-stability-non-rust-bootstrap`:

- `0728f74` — Harden pubpunk target-instance contract guards
- `c60962b` — Fix target-instance guard review follow-ups
- `8df09fe` — Fix review follow-ups for prompt exclusions and symbol guards
- `325df7e` — Align implementation status docs

### Frozen run-scoped architecture evidence

Merged from `codex/freeze-architecture-evidence-rescue`:

- `35e31fe` — Freeze run-scoped architecture evidence

Note: `3fbf421` (`Fail closed patch apply without controller context`) cherry-picked cleanly as an empty change because the effective logic was already present after the target-instance guard integration and current `main` history.

## Intentionally dropped after audit

### Rescue rollback noise

Dropped from `codex/freeze-architecture-evidence-rescue` before merge:

- `AGENTS.md`
- `README.md`
- `site/index.html`
- `site/style.css`
- untracked `site/assets/`
- untracked `site/styles.css`

Reason: this was rollback/noise against newer public docs/site state, not desired product behavior.

### `8c3a64c` from `codex/core-stability-non-rust-bootstrap`

Dropped:

- `8c3a64c` — Materialize Go and Python bootstrap scaffolds

Reason: rebasing this commit on current `main` reverted newer adapter fields and test harness fixes already present in `main` (`capability_resolution`, codex test-bin overrides, and newer controller scaffold plumbing). Current `main` already carries newer controller-scaffold infrastructure, so this older patch was treated as superseded rather than reapplied verbatim.

### Greenfield backlog branches

Dropped and deleted after audit:

- `greenfield-missing-manifest-no-progress`
- `greenfield-cut-run-stdin-finalization`
- `greenfield-cut-run-orphan-finalization`
- `greenfield-go-python-intake-after-init`
- `greenfield-rust-intake-after-init`
- `greenfield-rust-scaffold-scope-routing`
- `ts-node-greenfield-after-init`
- `drafter-timeout-resilience`
- `cut-run-no-progress-noise`
- `already-satisfied-cut-run`
- `artifact-assertion-harness-evidence`
- `declared-harness-evidence-metadata`
- `generated-harness-validation-recipes`
- `generated-runtime-scope-pollution`
- `harness-command-evidence`
- `harness-engineering`
- `harness-engineering-slice2`
- `human-proof-declared-harness-evidence`
- `human-proof-harness-evidence`
- `human-proof-inspect-evidence`
- `legacy-command-evidence-compat`
- `persisted-harness-spec`
- `root-workspace-target-check-fix`
- `work-harness-summary`
- `backup/pre-sync-main-20260412-153118`
- `exact-target-check-refinement`

Reason: bulk-merging the greenfield stack onto current `main` reverted newer, already-shipped behavior in current `main`, including the incident lane and codex drafter test stabilization. These branches need a fresh bounded re-authoring pass against current `main`, not a blind merge.

## Resulting policy

When a historical topic branch significantly predates current `main` and a naive merge reverts newer behavior, prefer:

1. cherry-picking the bounded keep slices that still compose cleanly,
2. documenting the dropped backlog explicitly,
3. deleting the old branch instead of preserving a misleading stale branch head.
