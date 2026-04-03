# AI Contributor Guide for specpunk

This file is for AI agents contributing changes to `specpunk` itself.

## First read

Before changing architecture or UX, read:
- `docs/product/NORTH-ROADMAP.md`
- `docs/product/ARCHITECTURE.md`
- `docs/product/CLI.md`

If you are working on a strategic track, also read the linked note in `docs/research/` first.

## Repo bootstrap

If the repo is not initialized for `punk`, run:

```bash
punk init --enable-jj --verify
```

If repo basename is not a safe project id:

```bash
punk init --project <id> --enable-jj --verify
```

## Default work intake

For initialized repos, plain user goal should default to:

```bash
punk go --fallback-staged "<goal>"
```

Use staged/manual flow when you need exact control or autonomy is blocked:

```bash
punk start "<goal>"
punk plot approve <contract-id>
punk cut run <contract-id>
```

For normal work, do not force the user to choose `plot` / `cut` / `gate`.
Treat those as expert/control surfaces.
The default shell contract should be:

- plain goal in
- one concise progress or blocker summary out
- one obvious next step out

## Scope and trust rules

- Do not trust drafted or refined `allowed_scope` blindly on a new repo class
- Inspect the contract before destructive or broad execution
- Keep one diff, one purpose
- Prefer minimal bounded slices over sweeping refactors

## Documentation rules

Update docs in the same diff when behavior changes:

- `README.md` for primary user-facing flow
- `docs/product/CLI.md` for command semantics
- `docs/product/ARCHITECTURE.md` for layer or invariant changes
- `docs/product/NORTH-ROADMAP.md` when strategic status changes
- relevant `docs/research/*.md` note when a strategic track gains new evidence

When proposing a bounded slice, name:

- which primitive it touches
- whether it changes a primitive or only a derived mechanism

## Reliability rules

- If a bug came from external dogfood, add a regression or fixture note if possible
- Check `docs/research/2026-04-03-specpunk-repo-fixture-matrix.md` before calling a reliability fix complete
- If autonomy is blocked, downgrade to staged/manual flow and report the blocker explicitly
- Do not hide failed verification behind shell success
- No destructive cleanup without explicit confirmation

## Git rules

- Never push directly to `main`
- Use a branch + PR flow
- Keep working tree reviewable

## What specpunk is trying to be

`specpunk` is currently aiming for:
- a bounded correctness substrate
- a durable work ledger
- a simple operator shell

It is **not** trying to copy Gas Town role mythology. Keep the system sharp, explicit, and inspectable.
