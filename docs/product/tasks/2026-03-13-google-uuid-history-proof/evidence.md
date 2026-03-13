# Evidence

Last updated: 2026-03-13
Task status: completed

## Repo Evidence

- local execution bridge exists at `sandbox/bin/specpunk`
- cloned repo exists at `sandbox/external-google-uuid`
- bounded historical commit exists at `0e97ed3`
- drift historical commit exists at `d55c313`
- stored approve artifact exists at `docs/product/tasks/2026-03-13-google-uuid-history-proof/generated-review-bounded.md`
- stored inspect artifact exists at `docs/product/tasks/2026-03-13-google-uuid-history-proof/generated-review-drift.md`

## Behavioral Evidence

- historical commit `0e97ed3` touches only `uuid.go` and `uuid_test.go`, and Specpunk returns `approve`
- historical commit `d55c313` is labeled as docs work but also touches `hash.go`, `uuid.go`, `version6.go`, and `version7.go`, and Specpunk returns `inspect`
- commit-specific `go test ./...` passes on both historical revisions

## Validation Notes

- in `sandbox/external-google-uuid`: checkout `0e97ed3` and run `go test ./...`
- in `sandbox/external-google-uuid`: checkout `d55c313` and run `go test ./...`
- in `/Users/vi/personal/specpunk`: `go build -o sandbox/bin/specpunk ./cmd/specpunk`
- in `sandbox/external-google-uuid`: `/Users/vi/personal/specpunk/sandbox/bin/specpunk check --task /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof/input-bounded.json --changed-git 0e97ed3^..0e97ed3 --output /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof/generated-review-bounded.md`
- in `sandbox/external-google-uuid`: `/Users/vi/personal/specpunk/sandbox/bin/specpunk check --task /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof/input-drift.json --changed-git d55c313^..d55c313 --output /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-history-proof/generated-review-drift.md`

## Why This Matters

This is the first proof where both the task input and the changed files come from third-party history rather than from a change we authored.
It is still a small repo, but the evaluation is meaningfully less curated.
