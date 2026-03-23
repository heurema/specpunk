# Context

Last updated: 2026-03-13
Task status: completed

## Issue Text

Issue `#137` asks:

> Is it possible to add `Validate(s string) error` that just validate the input string without created `UUID` (and its underlying byte array)?

The issue body says the current problem is:

- callers often receive UUIDs as strings
- they use `Parse(s string) (UUID, error)`
- they discard the returned `UUID`
- they want validation without creating the UUID object and underlying byte array

Source:

- `https://github.com/google/uuid/issues/137`

## Derived Boundary

From the issue text alone, the compact boundary is:

- add one validation-oriented API path
- change the implementation where UUID parsing and validation already live
- add focused tests for the new validation behavior

That becomes:

- allowed: `uuid.go`, `uuid_test.go`
- blocked: docs, workflows, unrelated generators, and other test files

## Why This Boundary Is Fair

The issue does not ask for:

- documentation changes
- workflow changes
- changes to UUID generation
- changes to serialization helpers

So the expected bounded answer is a new validation function plus its direct tests.
