# punk Harness

This document defines how `punk` should model harness engineering.

It does **not** introduce a new primitive layer.
It defines a derived harness/evidence plane that makes project runtime behavior legible and verifiable for agents.

---

## 1. Current stance

`punk` already has the right substrate primitives:

- `Project`
- `Goal`
- `Contract`
- `Scope`
- `Workspace`
- `Run`
- `DecisionObject`
- `Proofpack`
- `Ledger`

What is still missing is a first-class way to describe:

- how an agent boots a project-local runtime safely
- what evidence sources are available
- how validation should be performed beyond plain command checks
- which recovery paths are available when automation blocks

That missing layer is the harness.

---

## 2. What harness engineering means in `punk`

In `punk`, harness engineering means making a project's execution environment:

- repo-local
- inspectable
- bounded
- typed
- reusable across runs
- legible to both humans and agents

The harness is the answer to questions like:

- how does this project boot per isolated workspace?
- how do we know the app or service is ready?
- what can the agent observe directly: UI, logs, metrics, traces, generated artifacts?
- which validation recipes are canonical for this project?
- what should happen when one of those validation paths is unavailable or blocked?

---

## 3. Architectural placement

### Harness is a derived mechanism

Harness engineering is **not** a new primitive next to `Run` or `Proofpack`.

It is a derived evidence plane over existing primitives.

Why:

- durable truth already lives in canonical artifacts and events
- harness behavior can be recomposed from existing primitives
- introducing a new primitive too early would blur truth ownership
- the immediate need is better legibility and stronger evidence, not a second substrate

### Relationship to primitives

| Primitive | Harness relationship |
|---|---|
| `Project` | owns the repo-local harness binding |
| `Contract` | can declare which harness profile or validation path is expected |
| `Scope` | bounds what setup, evidence collection, and recovery are allowed |
| `Workspace` | provides isolated runtime context for harness execution |
| `Run` | records one execution attempt using a chosen harness profile |
| `DecisionObject` | synthesizes harness-derived evidence into a verdict |
| `Proofpack` | persists the executed evidence set and assertions |
| `Ledger` | keeps harness outcomes and recovery continuity inspectable later |

### Relationship to existing derived mechanisms

| Mechanism | Harness role |
|---|---|
| `init` | may create or refresh repo-local harness metadata |
| `start` / `go` | should prefer project-known harness defaults instead of ad hoc assumptions |
| `inspect project` | should explain harness capabilities and active profiles |
| `gate` | should execute typed validation recipes, not only shell commands |
| `proof` | should persist evidence manifests, not only final verdict summaries |
| `status` / `inspect work` | should expose blocked harness state and recovery refs when relevant |

---

## 4. Proposed packet: `HarnessSpec`

The first bounded design target is an inspectable repo-local packet.

```text
HarnessSpec
  project_id
  profiles[]
    name
    boot
    readiness_checks[]
    evidence_sources[]
    validation_recipes[]
    recovery_paths[]
  local_constraints[]
  updated_at
```

### Field intent

- `project_id` — binds the harness to a specific repo-local project identity
- `profiles[]` — named runtime/evidence profiles such as `cli`, `web-ui`, `service`, or `fixture`
- `boot` — how the runtime is started inside an isolated workspace
- `readiness_checks[]` — explicit checks that gate evidence collection
- `evidence_sources[]` — typed observable surfaces available to the agent
- `validation_recipes[]` — canonical project-specific assertions and validation paths
- `recovery_paths[]` — explicit next actions when harness execution blocks
- `local_constraints[]` — repo-specific caveats that should remain visible and inspectable

### Suggested storage shape

- machine packet: `.punk/project/harness.json`
- human-facing architecture/spec: `docs/product/HARNESS.md`

The packet should remain repo-local and versioned.

---

## 5. Evidence model

The harness exists to make evidence collection explicit.

Recommended v1 evidence source types:

- `command`
- `artifact`
- `ui_snapshot`
- `ui_flow`
- `log_query`
- `metric_assertion`
- `trace_assertion`

Recommended v1 rule:

> `gate` should gradually move from "run a few shell checks" toward "execute typed validation recipes against declared evidence sources".

This does **not** mean every project needs all evidence types immediately.

It means the model should allow them without redefining primitives later.

---

## 6. Project intelligence relationship

`ProjectOverlay` is the natural shell-facing map for harness information.

Current inspectable harness surface is intentionally derived from live repo state only.

`punk inspect project` and `punk inspect project --json` should expose a `harness_summary` with:

```text
inspect_ready
bootable_per_workspace
ui_legible
logs_legible
metrics_legible
traces_legible
```

In the current slice:

- `inspect_ready` means the repo can already expose at least one derived harness capability
- `bootable_per_workspace` is derived from current bootstrap/check/VCS readiness
- `ui_legible`, `logs_legible`, `metrics_legible`, and `traces_legible` are conservative repo-state signals only

Future `ProjectOverlay` growth can still add fields like:

```text
harness_ref
harness_profiles[]
persisted_harness_capabilities
  bootable_per_workspace
  ui_legible
  logs_legible
  metrics_legible
  traces_legible
```

This keeps one consistent answer to:

> what can this project safely show and verify right now?

Harness data should be inspectable through:

```bash
punk inspect project
punk inspect project --json
```

---

## 7. Gate / proof / ledger relationship

### `gate`

`gate` remains the only path that writes final decision artifacts.

Harness integration should make `gate` stronger, not softer:

- execute typed validation recipes
- record which harness profile was used
- keep recovery explicit when required evidence is unavailable

Current Slice 3 starts smaller:

- keep existing command checks as the only executed recipe type
- persist them as typed `command` evidence entries instead of relying only on flat `check_refs`
- keep `target` / `integrity` lane information, command text, pass/fail status, and stdout/stderr refs together

### `proof`

`Proofpack` should eventually persist:

- harness profile used
- executed validation recipes
- evidence manifest refs
- assertion outcomes
- blocked evidence reasons when verification could not complete cleanly

Current Slice 3 should at least carry forward the same typed command-evidence manifest that `gate` wrote onto `DecisionObject`.

### `Ledger`

The ledger should keep harness-related continuity visible:

- which evidence path was attempted
- what blocked
- which recovery contract or next action is authoritative

This is important because blocked verification should remain inspectable later without relying on old terminal logs.

---

## 8. Stewardship relationship

Harness engineering is not only runtime setup.
It also changes stewardship.

As harness data becomes repo-local and versioned, stewardship should eventually enforce:

- harness/doc freshness
- stale validation recipe detection
- fixture coverage drift
- recurring cleanup of weak or duplicated evidence paths

This is the harness-facing part of garbage collection.

---

## 9. Anti-goals

- do not introduce a second truth object next to `DecisionObject` or `Proofpack`
- do not turn harness behavior into opaque shell heuristics
- do not weaken `gate` into best-effort chatter
- do not import a "minimal merge gates" philosophy as a product rule for `punk`
- do not pretend all projects need UI or observability harnesses on day one
- do not treat runtime rollout ideas as already-implemented behavior

---

## 10. Phased rollout

### Phase 1 — docs and packet design

- define `HarnessSpec`
- define architectural placement
- document anti-goals and rollout boundaries

### Phase 2 — inspectable project harness

- expose derived harness capabilities through `punk inspect project`
- keep the first slice packet-free and derived from current repo state
- keep the model inspectable before adding deep runtime behavior

### Phase 3 — repo-local harness packet + typed evidence in `gate` / `proof`

- add repo-local harness packet
- allow typed validation recipes
- persist evidence manifests and assertion outcomes
- keep blocked evidence paths explicit and durable

### Phase 4 — stewardship and drift control

- stale harness/doc detection
- fixture coverage linkage
- recurring cleanup and quality-grade surfaces

---

## 11. Contributor rule

When proposing harness work, always say:

1. which primitive it relies on
2. whether it changes primitive truth or only a derived mechanism
3. what becomes newly inspectable
4. how blocked recovery remains explicit

If a proposal cannot answer those questions, it is probably too vague.
