# punk Dogfooding

## Summary

`punk` should be built with `punk` itself as soon as the base loop is usable.

But this must be done as **bounded self-hosting**, not blind self-trust.

Current status:
- canonical current-truth matrix: `docs/product/IMPLEMENTATION-STATUS.md`
- active v0 surface today for dogfooding is the existing `punk` CLI path: `init`, `go --fallback-staged`, `start`, `plot / cut / gate`, `status`, and `inspect`
- `punk-council` is **in-tree but inactive**
- `punk-shell`, `punk-skills`, `punk-eval`, and `punk-research` are **planned only** as separate crates
- bounded `punk research ...` is already a current expert/control capability, but it is not the default operator path and does not imply a dedicated `punk-research` crate

Core rule:
- `punk` may operate on `specpunk`
- but `punk` must not become the sole unquestioned authority over its own meta-level changes

Dogfooding is a product strategy, not a license for circular validation.

---

## Why dogfood at all

Building `punk` with `punk` is the fastest way to get:
- real contracts
- real receipts
- real gate decisions
- real failure patterns
- real cleanup obligations
- real skill patches
- real eval evidence

Dogfooding gives better signals than synthetic examples because it exposes:
- scope drift
- stale docs/configs
- cleanup misses
- duplicate v1/v2 paths
- weak contracts
- weak checks
- weak council packets

---

## Dogfooding law

Use `punk` on `specpunk`, but under **split trust**.

### Allowed
`punk` may dogfood itself for:
- feature work in the repo
- contract drafting and refinement
- bounded implementation runs
- deterministic gate checks
- proof generation
- doc cleanup
- replacement/removal obligations
- skill candidate generation
- eval evidence collection
- research packet generation

Current active-v0 note:

- today, dogfooding primarily runs through the existing `punk` shell mechanisms and `plot / cut / gate` surfaces
- later-stage council / skills / eval crates are not current operator defaults

### Not trusted by default
`punk` must not fully self-authorize changes to:
- gate semantics
- promotion policy
- eval scoring rules
- event schema
- artifact ownership model
- other kernel-level trust rules

Meta-level changes require stronger review than ordinary feature work.

---

## Trust domains

### 1. Execution trust
`punk` can write code in itself.

Examples:
- improve `plot`
- add runtime checks
- update docs
- add bounded council code

### 2. Acceptance trust
`gate` can still decide on normal feature work, but changes touching core trust surfaces need extra scrutiny.

For example:
- changes to gate decision logic
- changes to event truth model
- changes to promotion policy
- changes to dogfooding safety rules

These should use stricter review paths, ideally including:
- explicit human review
- fixed eval suites
- optional external or stronger council path

### 3. Improvement trust
Skills, eval, and research may propose improvements, but may not silently promote themselves.

All self-improvement must go through:
- evidence
- baseline comparison
- promotion decision
- rollback path

---

## Self-hosting stages

### Stage A — bounded dogfood
Use the base `plot -> cut -> gate -> proof` loop on `specpunk` itself.

Allowed in this stage:
- ordinary runtime improvements
- doc reconciliation
- bounded refactors
- cleanup obligations

Rules:
- operator remains in the loop
- no auto-promotion
- no silent trust-model changes

### Stage B — later council-assisted self-hosting
After `punk-council` moves from **in-tree but inactive** to an active operator capability, use councils for:
- architecture changes
- risky refactors
- contract hardening
- difficult review situations

Rules:
- council remains advisory
- gate remains final
- no council may self-certify core policy changes alone

### Stage C — later skill-ratchet self-improvement
After `punk-skills` and `punk-eval` move from **planned only** target-shape crates into active capability, allow dogfood runs to generate:
- candidate skill patches
- project overlays
- eval evidence

Rules:
- no promotion from one successful run
- no safety regression allowed
- rollback remains mandatory

### Stage D — bounded research-on-self
Current bounded slice:
- bounded `punk research ...` already allows operator-triggered research packets, artifacts, synthesis, and terminal advisory records on `specpunk`
- this is a current expert/control capability in the active CLI/orch/domain surface
- the dedicated `punk-research` crate and deeper worker execution loops remain **planned only**

Allow controlled research on `punk` itself for:
- architecture questions
- migration risks
- cleanup impact
- skill improvement ideas
- model/protocol comparison

Rules:
- research is bounded
- research is advisory
- research does not directly rewrite policy truth

---

## Meta-change categories

### Ordinary dogfoodable changes
These may use normal dogfooding flow:
- implementation features
- docs updates
- contract improvements
- non-core checks
- cleanup/removal work
- bounded council protocol code

### Sensitive self-hosting changes
These require stricter handling:
- gate semantics
- eval thresholds
- promotion policy
- event schema
- core packet schemas
- artifact truth model
- dogfooding safety rules

### High-trust path for sensitive changes
For sensitive self-hosting changes, require:
- explicit contract with scope called out as meta-level
- stronger review than usual
- fixed before/after eval evidence where possible
- human signoff before treating the change as trusted baseline

---

## How dogfooding feeds the other pillars

### Council
Dogfooding generates real council use cases:
- architecture proposals for real subsystem work
- real review disagreements
- real synthesis needs

### Skills
Dogfooding reveals project-specific competence gaps:
- missing heuristics
- missing cleanup rules
- missing docs-update rules
- repeated failure patterns

### Eval
Dogfooding provides real evidence for:
- task eval
- skill eval
- promotion decisions
- rollback triggers

### Research
Dogfooding creates realistic high-value research prompts:
- why a contract failed
- why a cleanup obligation was missed
- which model/protocol is more reliable
- where migration risk hides in the repo

---

## Default operating rules

1. Prefer real dogfood tasks over toy examples.
2. Keep dogfood tasks bounded and contract-first.
3. Record receipts, decisions, and proof artifacts for dogfood runs.
4. Treat dogfood failures as learning input, not embarrassment.
5. Map reliability failures to the repo fixture matrix in `docs/research/2026-04-03-specpunk-repo-fixture-matrix.md` whenever possible.
6. Never let the system silently self-promote from anecdotal success.
7. Separate ordinary feature trust from meta-level trust.
8. When in doubt, require stronger review for self-referential changes.

## Dogfood -> fixture rule

If a failure discovered through dogfood affects:

- bootstrap
- intake
- project identity
- scope inference
- generated artifact filtering
- blocked/recovery behavior
- event-log compatibility or corruption handling

then the fix should normally add or update a corresponding fixture-class regression.

The default expectation is:

> external dogfood should shrink the unknown space by becoming repeatable fixture coverage.

---

## Test scenarios

1. `punk` completes an ordinary feature in `specpunk` using the normal base loop.
2. A cleanup-heavy self-change detects and removes superseded paths.
3. A candidate skill patch is proposed from a failed dogfood run but not promoted automatically.
4. A sensitive meta-level change is blocked from normal self-certification and requires stronger review.
5. A research-on-self run produces structured advisory output without changing trusted policy by itself.
