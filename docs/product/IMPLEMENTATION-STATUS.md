# Implementation Status

This is the canonical **what is real today?** document for the repo.

Use it to keep three different questions separate:

- **crate status** — is a crate active today, in-tree but inactive, or only planned?
- **capability status** — does the behavior already exist somewhere in the active codebase today?
- **operator-surface status** — is it the current default path, an expert/control surface, or target-shape only?

One public surface today:

- non-interactive `punk` CLI

## Crate reality today

### Active v0 surface

- `specpunk`
- `punk-domain`
- `punk-events`
- `punk-vcs`
- `punk-core`
- `punk-orch`
- `punk-gate`
- `punk-proof`
- `punk-adapters`

### In-tree but inactive

- `punk-council`

### Planned only

- `punk-shell`
- `punk-skills`
- `punk-eval`
- `punk-research`

## Canonical implementation matrix

| Area / capability | Current implementation location | Crate status | Operator-surface status | Reality today | Explicitly not current/default | Later extraction or stage note |
|---|---|---|---|---|---|---|
| Public CLI entry | `crates/specpunk` + active service crates | active v0 surface | current operator surface | `punk <command> ...` is the real public surface today | not an interactive REPL shell | dedicated `punk-shell` crate is later |
| Default shell path | `crates/specpunk`, `crates/punk-orch` | active v0 surface | current operator default | `punk go --fallback-staged "<goal>"` already exists today | not proof that a standalone `Goal` primitive exists; not the future `punk-shell` crate | remains a derived shell mechanism until the later orchestration stage promotes `Goal` |
| Staged shell path | `crates/specpunk`, `crates/punk-orch` | active v0 surface | expert/control surface | `punk start "<goal>"` already exists today | not a separate primitive layer; not the default happy path | same later `Goal`-primitive note as above |
| Core bounded execution loop | `crates/specpunk`, `crates/punk-orch`, `crates/punk-gate`, `crates/punk-proof`, `crates/punk-core`, `crates/punk-vcs` | active v0 surface | expert/control surface | `plot / cut / gate` are real today | not a fourth runtime mode; not council-dependent | current core v0 loop |
| Bootstrap + inspect/status shell surfaces | `crates/specpunk`, `crates/punk-orch`, `crates/punk-events`, `crates/punk-domain` | active v0 surface | current operator surface | `punk init`, `punk status`, `punk inspect project`, `punk inspect work`, and JSON inspect are real today | not limited to a future interactive shell crate | later shell extraction may re-host UX, not truth |
| `Goal` primitive | no standalone v0 object; plain goal text and `goal_ref` projections live in `crates/specpunk`, `crates/punk-orch`, `crates/punk-domain` | active v0 crates, but primitive deferred | target-shape only as a primitive | target chain still includes `Goal`, but current v0 domain/runtime does **not** persist a standalone `Goal` object | do not document `Goal` as current canonical runtime truth today | planned for later orchestration-depth work |
| Bounded research surface | `crates/specpunk`, `crates/punk-orch`, `crates/punk-domain` | capability lives in active v0 crates | expert/control surface | `punk research start / artifact / synthesize / complete / escalate` and `punk inspect research_<id>` already exist today | not worker orchestration, not critique loops, not council execution, not default operator path | dedicated `punk-research` crate and deeper research execution are later |
| `punk-research` crate | no `crates/punk-research/` today | planned only | target-shape only | the separate crate does not exist today | do not infer crate reality from the active `punk research ...` commands | later extraction if research depth justifies it |
| Council subsystem | `crates/punk-council` | in-tree but inactive | not current/default | the crate exists and is buildable in-tree | not part of the active v0 operator path; not required for normal accept/block/escalate flow | selective Stage 2+ activation only |
| `punk-shell` crate | no `crates/punk-shell/` today | planned only | target-shape only | no dedicated interactive shell crate exists today | do not confuse current `go` / `start` / `status` UX with a shipped `punk-shell` crate | later Stage 1+ extraction |
| `punk-skills` / `punk-eval` crates | no `crates/punk-skills/` or `crates/punk-eval/` today | planned only | target-shape only | no dedicated ratchet crates exist today | do not describe skills/eval as current default operator surface | later Stage 4+ work |

## Common answers

- **Is `punk research ...` real today?** Yes. The commands already exist in the active CLI/orch/domain surface.
- **Is `punk-research` real today as a crate?** No. The dedicated crate is still planned only.
- **Is `punk go` real today?** Yes. It is the current default shell mechanism for initialized repos.
- **Does that mean `Goal` is already a standalone v0 primitive?** No. `Goal` remains deferred from the current implemented domain/runtime.
- **Is council active today?** No. `punk-council` is buildable in-tree but inactive, and it is not part of the current default operator path.
