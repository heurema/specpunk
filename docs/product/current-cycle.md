# Current Cycle

Last updated: 2026-03-13
Owner: Vitaly
Cycle: 2026-03-12 to 2026-03-26
Status: active

## Cycle Rule

This file is an execution plan, not product truth.
If it conflicts with `brief.md`, update `decisions.md` and `brief.md` first, or cut the conflicting work.

## Goal

Turn the current research and public surface into a working product loop on this repo.

## Priorities

### Priority 1

Outcome:
- `docs/product/` becomes the active operating layer for the project

Done when:
- the core product docs exist
- the first review file exists
- future product changes have a clear place to land

### Priority 2

Outcome:
- `specpunk.com` serves the current public surface as the product front door

Done when:
- the current site is live on the custom domain
- the public page no longer depends on prototype-only wording

### Priority 3

Outcome:
- the repo has one dogfooded task format for
  `intent -> scope -> evidence -> review`

Done when:
- one real task in this repo uses that format end to end
- the result is easier to reason about than a raw diff

Current proof:
- [first dogfood task](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-first-dogfood-task.md)
- [artifact drawer task](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-artifact-drawer.md)
- [scope review CLI task](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli.md)
  note: this now includes stored git-backed approve/inspect artifacts generated from [sandbox/git-proof-repo](/Users/vi/personal/specpunk/sandbox/git-proof-repo)
- [external git proof task](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof.md)
- [google uuid history proof task](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof.md)
- [google uuid PR context proof task](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof.md)
- [google uuid issue proof task](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-issue-proof.md)
- [task dir workflow task](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-task-dir-workflow.md)

## Success Criteria

- `brief.md` is treated as SSoT
- `specpunk.com` is live
- one dogfooded review artifact exists for this repo
- at least one decision is logged through `decisions.md`, not only discussed in chat

## Kill Criteria

- if a priority starts requiring tool-specific integration, cut it back to repo-local files and scripts
- if an artifact grows past one compact screen without adding review clarity, shrink it instead of polishing it
- if live contact handling creates operational burden, keep the public surface static and remove the obligation
- if the dogfood task cannot produce a meaningful review artifact this cycle, cut secondary work and focus only on wedge proof

## Out for This Cycle

- full benchmark harness
- transcript extraction
- rich contact workflow
- generalized multi-tool support
