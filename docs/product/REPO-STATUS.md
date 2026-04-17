# Repo Status

Use this note for the short status vocabulary only.

Keep these three axes separate:

- **crate status** — workspace membership today
- **capability status** — whether behavior already exists somewhere in the active codebase
- **operator-surface status** — whether something is the default path, an expert/control surface, or target-shape only

For the full matrix, read:

- `docs/product/IMPLEMENTATION-STATUS.md`

## Crate-status vocabulary

| Status | Meaning |
|---|---|
| **active v0 surface** | Present in the workspace and part of today's active v0 implementation surface |
| **in-tree but inactive** | Present/buildable in the workspace, but not part of today's normal operator path |
| **planned only** | Target-shape crate, not present in today's workspace membership |

## Current crate reality

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

## Short truth reminders

- capability reality can lead crate extraction: `punk research ...` is already real today even though `punk-research` is still planned only
- operator-surface reality can differ from crate reality: `punk-council` is buildable in-tree but not part of the active v0 operator path
- current shell behavior can exist before a later primitive exists: `punk go` is real today, while the standalone `Goal` primitive remains deferred
