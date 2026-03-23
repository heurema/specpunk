# Generated Review Artifact

Task: Review google/uuid PR #166 as a bounded functional change: add validation error types and related tests without formatter spillover or backward-compatibility regressions outside uuid validation paths.
Decision: inspect
Reason: blocked files were touched

## Scope Summary

- Declared allowed patterns: 2
- Declared blocked patterns: 8
- Changed files: 3
- In scope: 2
- Out of scope: 1
- Blocked touched: 1
- Scope status: drifted

## Allowed Patterns

- `uuid.go`
- `uuid_test.go`

## Blocked Patterns

- `json_test.go`
- `README.md`
- `doc.go`
- `hash.go`
- `version6.go`
- `version7.go`
- `time.go`
- `go.mod`

## Changed Files

- `json_test.go`
- `uuid.go`
- `uuid_test.go`

## Out Of Scope Files

- `json_test.go`

## Evidence

- `PR body frames the change around validation error types and Parse/ParseBytes/Validate behavior`
- `maintainer review asked to avoid cosmetic spillover and preserve backward compatibility`
- `revised PR commit 54f9572 passes go test ./...`

## Reviewer Posture

- inspect the change before approval
- scope drift is visible and must be understood
- evidence is attached to support the change
