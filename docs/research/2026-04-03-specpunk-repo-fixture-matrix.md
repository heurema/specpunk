# Specpunk Repo Fixture Matrix

Date: 2026-04-03
Status: active research track
Priority: P1

## Research question

Which repository classes must be covered by automated smoke/regression fixtures so that dogfood-discovered failures become repeatable tests instead of recurring surprises?

## Why this matters

Recent critical bugs were discovered only via external dogfood:
- stale global events
- project identity collisions
- `.build/` scope pollution
- exact refine scope drift
- fresh intake hangs
- mixed Node+Rust backend contract drift in `baseline`
- stable bounded `pubpunk` slices that needed controller-owned recipes (`init`, cleanup, validate)

That means internal happy paths are not enough.

## Required repo classes

1. fresh repo with no history
2. repo with legacy global events
3. repo with corrupted global events
4. SwiftPM repo with populated `.build/`
5. JS/TS repo with `node_modules/` and build outputs
6. Python repo with `.venv/`, `dist/`, `.pytest_cache/`
7. repo with ambiguous basename collision across different roots
8. repo with generated agent instructions already present
9. mixed Node+Rust repo with nested backend app plus Rust CLI
10. bootstrapped Rust repo with stable bounded `core + cli + tests` product slices

## Proposed v1 fixture matrix

The first useful matrix should be explicit and small enough to maintain.

| Fixture class | Why it exists | Must catch |
|---|---|---|
| `fresh-minimal` | cold start path | bootstrap, project id inference, first `start` / `go` artifact creation |
| `legacy-events` | backward compatibility path | old event schema such as missing fields like `actor` |
| `corrupted-events` | fail-fast truth handling | malformed event lines must fail clearly, not degrade silently |
| `swiftpm-build-noise` | generated artifact pollution | `.build/` excluded from scope candidates and `allowed_scope` |
| `js-build-noise` | frontend build pollution | `node_modules/`, `dist/`, build outputs excluded from scope and scans |
| `python-build-noise` | Python workspace pollution | `.venv/`, `.pytest_cache/`, `dist/` excluded from scope and scans |
| `basename-collision-a` / `basename-collision-b` | identity correctness | project ids and status views do not mix two repos with same basename |
| `bootstrap-reused` | idempotent onboarding | existing `AGENTS.md` / bootstrap files are reused safely and verified |
| `node-rust-mixed-runtime` | nested backend app plus Rust CLI | service/session/API prompts route to backend Node surfaces plus Rust CLI, not `.astro` pages, and choose grounded checks |
| `rust-bounded-controller-slice` | recurring deterministic bounded product work | stable `core + cli + tests` slices can complete through controller-owned templates instead of stalling or damaging repo state |

## Required command matrix by fixture

Not every fixture needs every command, but the baseline should be explicit.

| Fixture class | `init --verify` | `start` | `go --fallback-staged` | `status` | refine exact scope |
|---|---|---|---|---|---|
| `fresh-minimal` | required | required | required | required | optional |
| `legacy-events` | optional | required | optional | required | optional |
| `corrupted-events` | optional | required fail-fast | optional | required fail-fast | optional |
| `swiftpm-build-noise` | required | required | optional | required | required |
| `js-build-noise` | required | required | optional | required | required |
| `python-build-noise` | required | required | optional | required | required |
| `basename-collision-*` | required | optional | optional | required | optional |
| `bootstrap-reused` | required | optional | optional | required | optional |
| `node-rust-mixed-runtime` | required | required | required | required | required |
| `rust-bounded-controller-slice` | required | optional | required | required | required |

## Regression discipline

Every reliability bug found through dogfood should be classified before the fix:

1. is this a new fixture class?
2. is this a missing command in an existing fixture row?
3. is this a shell-only bug that still needs an existing fixture to assert behavior?

The default rule should be:

> no reliability fix is complete until it is mapped to a fixture class or an explicit reason is recorded why it cannot be.

## Suggested implementation shape

Prefer generated fixtures or fixture builders over large checked-in repo snapshots.

Recommended layers:

1. fixture builders for repo trees
2. shell-level smoke helpers
3. command assertions on:
   - exit status
   - artifact creation
   - scope contents
   - project identity
   - blocked/fail-fast behavior

## First fixture-backed bug backlog

The current known bug history suggests these should be the first concrete regression targets:

1. legacy events without `actor`
2. corrupted event line fail-fast behavior
3. unique project id across basename collisions
4. `.build/` exclusion from SwiftPM scope inference
5. exact refine scope preservation
6. `punk start` bounded fail-fast before artifact creation
7. mixed Node+Rust service/session/runtime contract generation for `baseline`
8. stable bounded `pubpunk` slices for init / cleanup / validate

## Required command matrix

For each relevant repo class, at minimum exercise:
- `punk init --enable-jj --verify`
- `punk start "<goal>"`
- `punk go --fallback-staged "<goal>"`
- `punk status`
- refine path where exact `allowed_scope` matters

## Working hypothesis

A small number of high-value fixture repos and shell-level smoke tests will catch more real regressions than deeper unit tests alone.

## Recommended next slices

1. create a `fixtures/` directory or generated fixture builder helpers
2. add shell-level smoke tests for the first backlog above
3. teach contributors to map every new dogfood reliability failure to a fixture row
4. require every reliability bugfix to add one fixture regression if possible

## Acceptance evidence

This track is done when:
- the recent external dogfood failures are reproducible locally in tests
- adding a new repo class becomes a standard maintenance move
- contributors can point to fixture coverage before claiming a flow is reliable
- fixture coverage is discussed in contributor guidance, not just in one research doc

## 2026-04-11 evidence update

New external dogfood classes are now concrete enough to treat as required fixtures:

- `node-rust-mixed-runtime`
  - exact baseline prompts must route service/session/API work into backend Node surfaces and Rust CLI surfaces
  - checks should stay grounded (`cargo check`, `npm run check`, repo wrapper builds), not optimistic workspace-wide `cargo test`
- `rust-bounded-controller-slice`
  - stable `pubpunk` slices now have controller-owned recipes for:
    - `init`
    - cleanup of obsolete `style/examples` references
    - `validate --json --project-root`
  - these slices should be tested as deterministic bounded controller paths, not only as general patch/apply flows
