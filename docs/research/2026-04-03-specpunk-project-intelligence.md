# Specpunk Project Intelligence

Date: 2026-04-03
Status: active research track
Priority: P2

## Research question

How should `specpunk` evolve from bootstrap files and project-scoped skills into a coherent project-intelligence layer that survives across sessions and repo classes?

## Why this matters

Current progress is real:
- project bootstrap
- project-aware status
- project-scoped skill triggers
- repo-local `AGENTS.md`
- repo-local `.punk/bootstrap/*`

But these pieces still behave more like adjacent utilities than one coherent project-intelligence system.

## Current components

- inferred project id
- project resolver and path mapping
- bootstrap file
- generated agent instructions
- project-scoped skills
- status scoping

## Gaps

- no explicit project overlay packet
- no canonical project capability summary
- no inspectable project-level repo-kind candidate graph
- no durable project memory packet inside runtime state
- no clear line between repo-local instructions and runtime project policy

## Working hypothesis

A future `ProjectOverlay` or equivalent should bundle:
- project identity
- repo capabilities
- bootstrap rules
- project-scoped skills
- local constraints
- safe default checks

This should remain repo-local and inspectable.

## Proposed `ProjectOverlay`

The next bounded design step should standardize one inspectable packet:

```text
ProjectOverlay
  project_id
  repo_root
  vcs_mode
  bootstrap_ref
  agent_guidance_ref
  capability_summary
  capability_resolution
  project_skill_refs
  local_constraints
  safe_default_checks
  status_scope_mode
  updated_at
```

### Field intent

- `project_id` — stable project identity used by runtime and shell surfaces
- `repo_root` — canonical local path binding for this project
- `vcs_mode` — current VCS substrate (`jj`, `git`, degraded fallback, etc.)
- `bootstrap_ref` — repo-local bootstrap file or packet ref
- `agent_guidance_ref` — repo-root `AGENTS.md` and/or `.punk/AGENT_START.md`
- `capability_summary` — what the repo is ready to do safely right now
- `capability_resolution` — concise summary plus ref to the detailed capability index
- `project_skill_refs` — active project-scoped skill refs
- `local_constraints` — repo-specific rules or caveats
- `safe_default_checks` — default checks this project expects for bounded work
- `status_scope_mode` — how `status` should resolve and present the repo

The detailed built-in repo-kind graph should live in a sibling packet:

```text
.punk/project/capabilities.json
```

That packet should hold:

- detected candidates
- active candidates
- suppressed/conflicted candidates
- advisory ambient candidates
- resolution mode/source
- semantic ids / versions / hashes

The coarse compatibility field such as `project_kind` can stay, but it should no longer be the only inspectable repo-kind truth.

## Proposed capability summary

The first version does not need deep introspection. It needs a reliable summary.

Recommended capability fields:

- `bootstrap_ready`
- `autonomous_ready`
- `staged_ready`
- `jj_ready`
- `proof_ready`
- `project_guidance_ready`

This is intentionally operator-facing. It answers:

> what can this project safely do right now?

## Proposed inspect surface

The future query should be explicit:

```bash
punk inspect project
```

or equivalent.

It should answer in one place:

1. who is this project?
2. which repo-local intelligence is active?
3. what is safe by default?
4. what guidance and skill overlays are in force?

## Relationship to existing files

`ProjectOverlay` should unify, not replace blindly:

- resolver/pin state
- repo-local bootstrap file
- repo-root `AGENTS.md`
- `.punk/AGENT_START.md`
- project-scoped skills
- project-specific default checks
- project-level capability resolution summary/ref

The main goal is to stop making agents assemble project state from scattered special cases.

## Relationship to shell surfaces

`init`, `status`, `start`, and `go` should eventually consume the same project-intelligence packet.

That means:

- `init` should create or refresh it
- `status` should display from it where relevant
- `go` should use it for safe defaults
- contributor agents should inspect it instead of guessing local conventions

When work is approved, the project-level capability graph should narrow into a contract-scoped frozen capability packet under:

```text
.punk/contracts/<feature-id>/capability-resolution.json
```

That frozen packet is the semantic input `cut`, `gate`, and `proof` should trust, rather than live ambient repo re-resolution.

## Anti-goals

- do not turn project intelligence into opaque hidden heuristics
- do not make project overlays silently mutate without traceable evidence
- do not require a heavy config language before proving the packet shape
- do not split runtime project truth across more one-off files

## Recommended next slices

1. define a `ProjectOverlay` packet shape
2. add `punk inspect project` or equivalent
3. add a separate inspectable project capability index for mixed-repo candidate graphs
4. unify bootstrap file, generated instructions, project-scoped skill metadata, and capability-summary refs under one inspectable model
5. freeze narrowed capability semantics into approved contracts

## Acceptance evidence

This track is done when:
- project bootstrap no longer feels like a collection of special cases
- agents can inspect one project packet instead of piecing together conventions from multiple files
- mixed repo-kind candidates are inspectable without turning ambient state into hidden truth
- project-specific behavior is explicit and testable
- shell-facing project status can be explained from one inspectable source
