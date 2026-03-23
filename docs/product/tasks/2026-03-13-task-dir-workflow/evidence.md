# Evidence

Last updated: 2026-03-13
Task status: completed

## Repo Evidence

- `specpunk task init` now exists as a CLI command
- `specpunk check` now supports `--task-dir`
- a generated demo task directory exists under `docs/product/tasks/2026-03-13-task-dir-workflow/demo-task/`
- a generated review artifact exists under that demo task directory

## Behavioral Evidence

- the scaffold command creates `input.json`, `intent.md`, `scope.md`, `evidence.md`, and `review.md`
- `check --task-dir` defaults its output to `generated-review.md`
- tests cover both the scaffold path and the task-dir check path
- shell glob patterns must be quoted so they stay patterns instead of expanding to concrete files

## Validation Notes

- `go test ./...`
- `go run ./cmd/specpunk task init --task-dir ...`
- `go run ./cmd/specpunk check --task-dir ... --changed-manifest ...`

## Why This Matters

This is the first step where the product stops proving itself only through handcrafted packets and starts exposing a repeatable user-facing workflow.
