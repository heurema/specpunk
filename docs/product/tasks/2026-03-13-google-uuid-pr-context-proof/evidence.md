# Evidence

Last updated: 2026-03-13
Task status: completed

## Repo Evidence

- local execution bridge exists at `sandbox/bin/specpunk`
- cloned repo exists at `sandbox/external-google-uuid`
- PR head branch exists locally as `pr-166`
- stored raw initial diff exists at `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/raw-diff-initial.patch`
- stored raw revised diff exists at `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/raw-diff-revised.patch`
- stored inspect artifact exists at `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/generated-review-initial.md`
- stored approve artifact exists at `docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/generated-review-revised.md`

## Behavioral Evidence

- initial PR commit `a0eddd2` touches `json_test.go`, `uuid.go`, and `uuid_test.go`, and Specpunk returns `inspect`
- revised PR commit `54f9572` touches only `uuid.go` and `uuid_test.go`, and Specpunk returns `approve`
- `go test ./...` fails on `a0eddd2`
- `go test ./...` passes on `54f9572`

## Validation Notes

- in `/Users/vi/personal/specpunk`: `go build -o sandbox/bin/specpunk ./cmd/specpunk`
- in `sandbox/external-google-uuid`: `git fetch origin pull/166/head:pr-166`
- in `sandbox/external-google-uuid`: checkout `a0eddd2` and run `go test ./...`
- in `sandbox/external-google-uuid`: checkout `54f9572` and run `go test ./...`
- in `sandbox/external-google-uuid`: `/Users/vi/personal/specpunk/sandbox/bin/specpunk check --task /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/input-pr-context.json --changed-git d55c313874fe007c6aaecc68211b6c7c7fc84aad..a0eddd21d444740fdb10e7bae182f55a935d3877 --output /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/generated-review-initial.md`
- in `sandbox/external-google-uuid`: `/Users/vi/personal/specpunk/sandbox/bin/specpunk check --task /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/input-pr-context.json --changed-git d55c313874fe007c6aaecc68211b6c7c7fc84aad..54f95728c20a700436b4f3a8e699a99235748175 --output /Users/vi/personal/specpunk/docs/product/tasks/2026-03-13-google-uuid-pr-context-proof/generated-review-revised.md`

## Why This Matters

This is the first proof where the boundary is drawn from actual maintainer review language and then tested against two states of the same PR.
It is still one repo and one PR, but it is much closer to a real review workflow than fixture-based proofs.
