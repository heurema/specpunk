# Evidence

Last updated: 2026-03-13
Task status: completed

## Repo Evidence

- local execution bridge exists at `sandbox/bin/specpunk`
- cloned repo exists at `sandbox/external-google-uuid`
- issue source exists at `https://github.com/google/uuid/issues/137`
- merged revision exists at `9ee7366`
- stored raw diff exists at `docs/product/tasks/2026-03-13-google-uuid-issue-proof/raw-diff.patch`
- stored approve artifact exists at `docs/product/tasks/2026-03-13-google-uuid-issue-proof/generated-review.md`

## Behavioral Evidence

- merged revision `9ee7366` changes only `uuid.go` and `uuid_test.go`
- the issue-derived boundary matches those files and Specpunk returns `approve`
- `go test ./...` passes on revision `9ee7366`

## Validation Notes

- in `/Users/vi/personal/specpunk`: `go build -o sandbox/bin/specpunk ./cmd/specpunk`
- in `sandbox/external-google-uuid`: checkout `9ee7366` and run `go test ./...`
- in `sandbox/external-google-uuid`: `/Users/vi/personal/specpunk/sandbox/bin/specpunk check --task /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-issue-proof/input-issue.json --changed-git 9ee7366^..9ee7366 --output /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-issue-proof/generated-review.md`

## Why This Matters

This is the first proof where the task text comes from an external issue rather than from a PR body or review discussion.
It is still a clean small repo, but the wedge now demonstrates issue-to-diff reasoning directly.
