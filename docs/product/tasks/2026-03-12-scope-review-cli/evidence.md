# Evidence

Last updated: 2026-03-12
Task status: completed

## Repo Evidence

- the CLI entrypoint exists at `cmd/specpunk/main.go`
- stdin CLI coverage exists at `cmd/specpunk/main_test.go`
- the scope review logic exists at `internal/check/check.go`
- the wedge is covered by tests in `internal/check/check_test.go`
- explicit git-revspec coverage exists in `internal/check/check_test.go` on temporary git repos
- a stable sandbox git repo now exists under `sandbox/git-proof-repo/`
- bounded and drift task inputs exist under `docs/product/tasks/2026-03-12-scope-review-cli/`
- bounded and drift markdown artifacts exist under `docs/product/tasks/2026-03-12-scope-review-cli/`
- plain text and JSON changed-file manifests exist under `docs/product/tasks/2026-03-12-scope-review-cli/`
- bounded and drift diff files exist under `docs/product/tasks/2026-03-12-scope-review-cli/`
- bounded and drift git-backed artifacts now exist under `docs/product/tasks/2026-03-12-scope-review-cli/`

## Behavioral Evidence

- the tool reads structured task data from JSON
- the tool classifies changed files as in-scope or out-of-scope
- the tool highlights blocked paths when touched
- the tool emits a short decision-oriented review artifact
- the tool now shows both `approve` and `inspect` outcomes on repo-local examples
- the tool can take changed files from an external manifest instead of only inline task JSON
- the tool can derive changed files from a patch file without a separate manifest
- the tool can read patch content from stdin, which matches shell piping from a VCS command
- the tool can derive changed files from `git diff --name-only --relative <revspec>`
- the tool now proves that git-based approve and inspect paths can be stored as concrete artifacts, not only exercised in temporary tests

## Validation Notes

- `go test ./...`
- `go run ./cmd/specpunk check --task docs/product/tasks/2026-03-12-scope-review-cli/input.json --output docs/product/tasks/2026-03-12-scope-review-cli/generated-review.md`
- `go run ./cmd/specpunk check --task docs/product/tasks/2026-03-12-scope-review-cli/input-drift.json --output docs/product/tasks/2026-03-12-scope-review-cli/generated-review-drift.md`
- `go run ./cmd/specpunk check --task docs/product/tasks/2026-03-12-scope-review-cli/input.json --changed-manifest docs/product/tasks/2026-03-12-scope-review-cli/changed-manifest.txt --output docs/product/tasks/2026-03-12-scope-review-cli/generated-review-manifest.md`
- `go run ./cmd/specpunk check --task docs/product/tasks/2026-03-12-scope-review-cli/input-drift.json --changed-manifest docs/product/tasks/2026-03-12-scope-review-cli/changed-manifest-drift.json --output docs/product/tasks/2026-03-12-scope-review-cli/generated-review-manifest-drift.md`
- `go run ./cmd/specpunk check --task docs/product/tasks/2026-03-12-scope-review-cli/input.json --changed-diff docs/product/tasks/2026-03-12-scope-review-cli/changed-diff.patch --output docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff.md`
- `go run ./cmd/specpunk check --task docs/product/tasks/2026-03-12-scope-review-cli/input-drift.json --changed-diff docs/product/tasks/2026-03-12-scope-review-cli/changed-diff-drift.patch --output docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff-drift.md`
- `go run ./cmd/specpunk check --task docs/product/tasks/2026-03-12-scope-review-cli/input.json --changed-diff - --output docs/product/tasks/2026-03-12-scope-review-cli/generated-review-diff-stdin.md < docs/product/tasks/2026-03-12-scope-review-cli/changed-diff.patch`
- from `sandbox/git-proof-repo/`: `go run /Users/vi/personal/specpunk/cmd/specpunk check --task /Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/input-git.json --changed-git HEAD~2..HEAD~1 --output /Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/generated-review-git.md`
- from `sandbox/git-proof-repo/`: `go run /Users/vi/personal/specpunk/cmd/specpunk check --task /Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/input-git-drift.json --changed-git HEAD~1..HEAD --output /Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-scope-review-cli/generated-review-git-drift.md`

## Why This Matters

This is the first repo-local implementation of the wedge itself.
It does not rely on the public site alone and does not depend on a coding-agent transcript.
It also sets the long-term CLI direction on a single-binary runtime.
It now also accepts changed-file manifests from external task runners, which is closer to a real repo workflow than hand-entered changed paths.
It can also derive changed files from a patch file, which moves the wedge one step closer to VCS-native review input.
It can now take that diff through stdin, which makes shell piping a real workflow instead of a future idea.
It can now also take an explicit git revspec, and that path is now stored as a real bounded/drift proof through the checked-in sandbox repo even though the top-level workspace itself is not a git checkout.
