# Repo Status

This note is the small source-of-truth for **what is real today** vs **what is only target shape**.

Use this exact vocabulary everywhere in the repo:

| Status | Meaning |
|---|---|
| **active v0 surface** | Exists in the current workspace and is part of today's operator/runtime surface. |
| **in-tree but inactive** | Exists in the current workspace, but is not part of today's normal v0 operator path yet. |
| **planned only** | Part of the target product shape, but not a current workspace member yet. |

## Current workspace truth

### Active v0 surface

- `punk-cli`
- `punk-domain`
- `punk-events`
- `punk-vcs`
- `punk-core`
- `punk-orch`
- `punk-gate`
- `punk-proof`
- `punk-adapters`

These crates define the current working loop:

`init -> start/go -> plot -> cut -> gate -> proof`

### In-tree but inactive

- `punk-council`

`punk-council` is intentionally kept as a workspace member and buildable in-tree, but it is **not** part of the active v0 operator surface.

Until Stage 2 is promoted:

- council remains advisory-only
- council is not required for the core acceptance loop
- the normal operator path must remain usable without council
- selective council is only justified after the repo already has a usable bootstrap + staged + proof-ready core loop

### Planned only

- `punk-shell`
- `punk-skills`
- `punk-eval`
- `punk-research`

These are target-shape crates, not current workspace members.

## Legacy extraction note

The nested legacy workspace under `punk/` is still present as extraction material.

It is **not** the target architecture and **not** the current public operator surface.

Treat it as:

- source material to extract
- code to relocate
- code to delete once replaced
