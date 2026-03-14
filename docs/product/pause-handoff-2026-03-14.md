# Pause Handoff

Last updated: 2026-03-14
Owner: Vitaly
Status: active

## Why The Project Is Paused

- the current wedge is proven enough to stop safely
- the next uncertainty is product and idea research, not missing implementation pieces
- continuing to code now would risk polishing the wrong thing

## What Already Exists

Product truth:
- [brief.md](/Users/vi/personal/specpunk/docs/product/brief.md) is the SSoT
- [roadmap.md](/Users/vi/personal/specpunk/docs/product/roadmap.md) defines milestone outcomes
- [current-cycle.md](/Users/vi/personal/specpunk/docs/product/current-cycle.md) is now explicitly paused

Current wedge:
- `scope enforcement + minimal review artifact`
- runtime path exists in `Go`
- repo-local product path exists as `specpunk task init` plus `specpunk check --task-dir`

Working code:
- CLI entrypoint: [main.go](/Users/vi/personal/specpunk/cmd/specpunk/main.go)
- scope logic: [check.go](/Users/vi/personal/specpunk/internal/check/check.go)
- task directory workflow: [taskdir.go](/Users/vi/personal/specpunk/internal/taskdir/taskdir.go)

Working proofs:
- repo-local task-dir proof: [2026-03-13-task-dir-workflow.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-task-dir-workflow.md)
- external git proof: [2026-03-13-external-git-proof.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof.md)
- historical / PR / issue proofs:
  - [2026-03-13-google-uuid-history-proof.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof.md)
  - [2026-03-13-google-uuid-pr-context-proof.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof.md)
  - [2026-03-13-google-uuid-issue-proof.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-issue-proof.md)

Public surface:
- `specpunk.com` is the public front door
- static site files live in:
  - [index.html](/Users/vi/personal/specpunk/site/index.html)
  - [style.css](/Users/vi/personal/specpunk/site/style.css)

## Repo State At Pause

Git state:
- current branch: `bootstrap/initial-import`
- default branch: `main`
- open PR: `#1 Initial Specpunk import`
- PR URL: `https://github.com/heurema/specpunk/pull/1`
- PR state at pause: `OPEN`, `CLEAN`

Local state:
- workspace was clean at handoff time
- current repo root is now a git repo
- external proof repos live under `sandbox/` and are fixtures, not the source of truth

## Do Not Re-Prove First

These things already have enough proof for now:
- the wedge can emit a bounded review artifact from declared scope
- the checker can read changes from task JSON, manifest, diff file, stdin, and git range
- the wedge has been tested against task packet, repo history, PR discussion, and issue text
- the task-directory workflow exists and works on a demo task

The main remaining implementation gap is smaller:
- run the task-directory workflow on one real repo-local code change

## Read This First On Return

1. [brief.md](/Users/vi/personal/specpunk/docs/product/brief.md)
2. [pause-handoff-2026-03-14.md](/Users/vi/personal/specpunk/docs/product/pause-handoff-2026-03-14.md)
3. [queued-next-tasks.md](/Users/vi/personal/specpunk/docs/product/queued-next-tasks.md)
4. [open-questions.md](/Users/vi/personal/specpunk/docs/product/open-questions.md)
5. [2026-03-14.md](/Users/vi/personal/specpunk/docs/product/reviews/2026-03-14.md)

## Resume Rules

- start with a new dated review file before making the next significant product change
- if the thesis changed during research, update `decisions.md` and `brief.md` first
- keep the next implementation diff bounded to one purpose
- prefer product clarity over CLI expansion

## Useful Commands On Return

Verification:
```bash
go test ./...
```

CLI sanity:
```bash
go run ./cmd/specpunk --help
go run ./cmd/specpunk task init --help
go run ./cmd/specpunk check --help
```

PR sanity:
```bash
gh pr view 1
git status --short
git log --oneline --decorate -5
```
