# Context

Last updated: 2026-03-13
Task status: completed

## PR Body

PR `#166` is framed as:

> This PR improves error handling by adding specific error types and ensuring compatibility with `errors.Is` for better validation.

The body says the work should:

- introduce specific validation error types
- refactor error handling around `Parse`, `ParseBytes`, and `Validate`
- keep backward compatibility through `IsInvalidLengthError`

Source:

- `https://github.com/google/uuid/pull/166`

## Review Signals

Maintainer review narrows the expected boundary:

> Please do not add formatting changes to a PR that is making actual changes. In general a PR should either be a cosmetic PR or a functional PR but not both.

> This is a breaking change as it no longer include the invalid length. There is no reason to change the underlying type of this error.

Contributor reply confirms that the original PR scope was too wide:

> I've removed some changes that were added unintentionally by my code formatter in my code editor. Additionally I've slightly modified the approach with `ErrInvalidLength` and `ErrInvalidURNPrefix` error types to keep more backward compatibility.

Sources:

- `https://github.com/google/uuid/pull/166#discussion_r1702416013`
- `https://github.com/google/uuid/pull/166#discussion_r1702414486`
- `https://github.com/google/uuid/pull/166#issuecomment-2266341241`

## Derived Boundary

From the PR body and review discussion, the compact review boundary becomes:

- allowed: `uuid.go`, `uuid_test.go`
- blocked: `json_test.go`, docs, unrelated generators, and module metadata

Why `json_test.go` is blocked here:

- it appears in the initial PR diff
- it disappears after the contributor removes unintended and backward-incompatible changes
- the maintainer feedback points toward preserving existing behavior and avoiding extra change surface
