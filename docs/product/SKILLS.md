# punk Skills

## Summary

`punk-skills` composes a **reproducible skill packet** for a worker under a specific role, project, and task.

It is not just a directory of markdown files and not a self-mutating memory blob.

Core rule:
- skills may improve
- but only through `candidate -> eval -> promotion`
- never through silent mutation during task execution

v1 covers:
- skill layer model
- project overlays
- deterministic packet composition
- candidate skill patches
- active/superseded/rollback states

---

## Core model

### Roles
Supported `SkillRole` values:
- `Architect`
- `ContractDrafter`
- `Implementer`
- `Reviewer`
- `Verifier`
- `CleanupAuditor`
- `DocsAuditor`
- `MigrationAuditor`
- `Researcher`

### Layer kinds
A final skill packet is composed from:
1. `Base`
2. `Domain`
3. `ProjectOverlay`
4. `TaskOverlay`

### `SkillLayer`
A versioned reusable layer.

Must include:
- kind
- role
- optional domain
- optional project id
- version
- status (`active | candidate | rejected | superseded | rolled_back`)
- title
- summary
- instructions ref
- checklists
- heuristics
- known failure patterns

### `SkillPacket`
The actual packet given to a worker.

Must include refs to:
- selected base layer
- selected domain layers
- selected project overlays
- selected task overlays
- current project/task context
- composed instructions ref
- packet hash

### `CandidateSkillPatch`
A proposed improvement to one or more skill layers.

Must include:
- target layer ref
- role
- change summary
- patch ref
- evidence refs
- provenance (`operator | eval | council | research`)
- status (`candidate | promoted | rejected | rolled_back`)

---

## Composition rules

### Final packet shape
A worker gets:

```text
Base(role)
+ Domain(role, stack)
+ ProjectOverlay(role, project)
+ TaskOverlay(current task/run/contract context)
```

### Precedence
Highest precedence wins:
1. `TaskOverlay`
2. `ProjectOverlay`
3. `Domain`
4. `Base`

### Merge rules
Deterministic only:
- `instructions`: append by precedence, highest precedence last as override section
- `checklists`: ordered union by precedence
- `heuristics`: ordered union by precedence
- `anti_patterns`: ordered union by precedence
- scalar fields: highest-precedence non-empty wins

No LLM-based merge in v1.

### Reproducibility
Same selected layers + same task context must produce the same `packet_hash`.

---

## Lifecycle and storage

### States
Supported lifecycle states:
- `active`
- `candidate`
- `rejected`
- `superseded`
- `rolled_back`

### Candidate patch lifecycle

The minimum lifecycle is:

```text
candidate
-> eval
-> promote | reject
-> rollback (later, if promoted behavior regresses)
```

Hard rules:

- a candidate patch is not active just because it exists on disk
- activation requires an explicit `PromotionDecision`
- one successful task/run is never enough to promote a candidate patch
- safety regressions found by eval must block promotion
- rollback is a first-class state, not an implicit delete

### Repo-local storage
Project overlays and candidates live in:

```text
.punk/
  skills/
    overlays/
      <role>/
    candidates/
    packets/
```

The active repo-local overlay refs should be surfaced through the canonical project-intelligence packet at:

```text
.punk/project/overlay.json
```

That packet is the inspectable source of truth for project skill refs.
It should also carry a concise capability-resolution summary plus a ref to the detailed repo-kind packet at:

```text
.punk/project/capabilities.json
```

The detailed capability packet is where built-in repo-kind candidates (`active`, `suppressed`, `conflicted`, `advisory`) remain inspectable without making the overlay itself too noisy.

### Global storage
Shared reusable base/domain layers live in:

```text
~/.punk/
  skills/
    base/
    domain/
```

Global or ambient locations are not the primary source of repo-specific project intelligence.
If a repo has no repo-local overlays yet, ambient discovery may be used only as explicit fallback/migration behavior and must be exposed by `ProjectOverlay`.
Ambient/global discovery is advisory only in v0: it must not become execution-time authority for `cut` or `gate`, and it must not override the repo-local built-in capability resolution that was frozen into approved contracts.

### Hard invariant
Every task/run must be able to reconstruct:
- exact `SkillPacket`
- layer refs and versions
- which overlay set was active at the time

---

## Patch rules

### Allowed candidate improvements
Candidate patches may add or improve:
- checklist items
- cleanup rules
- docs update rules
- project heuristics
- known failure patterns
- anti-pattern warnings
- role-specific routing hints

### Not allowed as skill patches
A candidate patch may not directly change:
- kernel semantics
- gate rules
- event schema
- artifact ownership model
- global architecture policy outside skill scope

### Evidence requirement
Every candidate patch must cite evidence from:
- failed runs
- blocked gates
- superseded runs
- council findings
- curated fixtures
- incident records

No unsupported patches in v1.

Minimum candidate patch packet should therefore be reviewable with:

- target layer ref
- evidence refs
- intended project/role scope
- eval suite refs once evaluation starts
- final promotion decision ref once the ratchet closes

---

## Integration

### With `punk-orch`
Examples:
- `plot contract` -> `ContractDrafter` packet
- `cut run` -> `Implementer` packet
- later council slots -> role-specific packets

### With `punk-council`
Council assignments request packets by:
- role
- model family
- project
- frozen task/council context

### With `punk-eval`
`punk-skills` does not decide promotions.
It exposes:
- active layers
- candidate patches
- packet refs
- packet history

`punk-eval` later decides whether to promote or reject a candidate patch.

That separation is permanent:

- `punk-skills` owns composition and overlay state
- `punk-eval` owns comparison, scoring, and promotion/rollback decisions
- `gate` still decides acceptance for the current run only

---

## Implementation defaults

v1 defaults:
- skills are read-only during task execution
- project overlays are the main personalization unit
- one project may have different overlays per role
- packet composition is deterministic and schema-based
- promotion is deferred to `punk-eval`
- candidate patches stay inert until eval closes with an explicit decision

---

## Test scenarios

1. packet composition follows precedence deterministically
2. same layers/context produce same packet hash
3. rejected/superseded layers are excluded from active packets
4. candidate patch registration requires evidence refs
5. rollback restores previous active overlay deterministically
6. different roles in the same project receive different packets with shared project context
