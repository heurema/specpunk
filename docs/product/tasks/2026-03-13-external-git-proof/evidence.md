# Evidence

Last updated: 2026-03-13
Task status: completed

## Repo Evidence

- local execution bridge exists at `sandbox/bin/specpunk`
- cloned repo exists at `sandbox/external-matryer-is`
- bounded code change commit exists at `dc0c39e`
- drift doc-boundary commit exists at `be9247a`
- stored approve artifact exists at `docs/product/tasks/2026-03-13-external-git-proof/generated-review-bounded.md`
- stored inspect artifact exists at `docs/product/tasks/2026-03-13-external-git-proof/generated-review-drift.md`

## Behavioral Evidence

- `loadComment` in the external repo now accepts compact `//comment` forms without a space
- the external repo has a focused regression test for that behavior
- `go test ./...` passes in the external repo after both commits
- `specpunk check --changed-git` returns `approve` on the bounded range
- `specpunk check --changed-git` returns `inspect` on the drift range where `README.md` is touched

## Validation Notes

- in `sandbox/external-matryer-is`: `go test ./...`
- in `/Users/vi/personal/specpunk`: `go build -o sandbox/bin/specpunk ./cmd/specpunk`
- in `sandbox/external-matryer-is`: `/Users/vi/personal/specpunk/sandbox/bin/specpunk check --task /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof/input-bounded.json --changed-git HEAD~2..HEAD~1 --output /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof/generated-review-bounded.md`
- in `sandbox/external-matryer-is`: `/Users/vi/personal/specpunk/sandbox/bin/specpunk check --task /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof/input-drift.json --changed-git HEAD~1..HEAD --output /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-external-git-proof/generated-review-drift.md`

## Why This Matters

This is the first non-fixture git-backed proof for the wedge.
The review object is no longer based only on a repo we designed around Specpunk.
It now survives contact with a small external codebase, its tests, and its own file boundaries.
It also surfaced a real operational detail: cross-module execution is simpler through a built binary than through `go run` from inside another Go module.
