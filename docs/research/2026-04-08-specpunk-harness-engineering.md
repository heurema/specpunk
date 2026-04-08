# Specpunk Harness Engineering

Date: 2026-04-08
Status: active research track
Priority: cross-cutting

## Research question

How should `specpunk` absorb harness-engineering lessons from agent-first engineering without diluting its correctness substrate?

## Why this matters

`specpunk` already has the right instinct around:

- bounded contracts
- explicit scope
- isolated workspaces
- gate truth
- proof artifacts
- durable ledger projections

But it still lacks a first-class model for the execution environment that agents need in order to validate real behavior reliably.

Today the gap is not primarily "better prompting".
The gap is harness legibility.

## External source

Primary source:

- OpenAI, "Harness engineering: leveraging Codex in an agent-first world", 2026-02-11, https://openai.com/index/harness-engineering/

## Main lessons from the article

### 1. Humans steer; agents execute

The main leverage point moves away from hand-writing code and toward:

- specifying intent
- designing environments
- building feedback loops
- encoding recovery and review paths

### 2. The environment must be legible

The article's strongest operational point is not autonomous coding in itself.
It is that agents were given direct access to:

- bootable app instances per worktree
- UI inspection and navigation
- logs
- metrics
- traces

Once those surfaces became inspectable, validation tasks stopped being vague wishes and became executable assertions.

### 3. Repository knowledge must be structured

A giant `AGENTS.md` failed for predictable reasons:

- context is scarce
- too much guidance becomes non-guidance
- monoliths rot quickly
- giant blobs are hard to verify mechanically

The recommended pattern is:

- short `AGENTS.md` as map / TOC
- structured `docs/` as system of record
- progressive disclosure

### 4. Invariants matter more than style policing

The article argues for:

- strict boundaries
- predictable structure
- structural tests
- custom lints
- remediation-friendly failure messages

This matches `specpunk` well.
What transfers is the discipline of encoded invariants, not a specific stack.

### 5. Entropy must be handled continuously

Agents replicate whatever patterns already exist, including weak ones.
That means cleanup and drift control must become continuous, not occasional.

## What transfers directly to `specpunk`

### A. Docs as system of record

This transfers almost directly.
`specpunk` already points in this direction through:

- `docs/product/ARCHITECTURE.md`
- `docs/product/CLI.md`
- `docs/product/NORTH-ROADMAP.md`
- repo-local bootstrap guidance
- research notes tied to strategic tracks

The next step is not a larger prompt blob.
It is better structured repo-local maps and inspectable packets.

### B. Project legibility through `ProjectOverlay`

`ProjectOverlay` already exists as the best current home for shell-facing project intelligence.
The missing piece is explicit harness capability modeling.

### C. Gate/proof as evidence plane

`specpunk` already has `gate` and `proof`, which means it has a natural home for typed harness evidence.
This is one of the biggest differences from generic agent shells.

### D. Stewardship as continuous cleanup

The article's "garbage collection" idea maps naturally onto the stewardship pillar and the repo-fixture matrix track.

## What should not transfer literally

### 1. Do not import minimal merge-gate philosophy as a product rule

The article describes a high-throughput environment with minimal blocking merge gates.
That is a repo/process tradeoff, not the core reusable architectural lesson.

For `specpunk`, `gate` and `proof` are part of the product's reason to exist.
The transferable lesson is richer, cheaper, more inspectable evidence — not weaker verification.

### 2. Do not copy the "0 manually-written code" constraint

That is an experiment discipline, not an architecture requirement.
It is not the main thing `specpunk` should learn from.

### 3. Do not create new primitives prematurely

The article motivates better scaffolding, but not a second truth model.
In `specpunk`, that means harness should be modeled as a derived mechanism over existing primitives.

## Recommended architecture decision

`specpunk` should adopt harness engineering as a **derived harness/evidence plane**.

That means:

- no new primitive next to `Run` or `Proofpack`
- repo-local inspectable harness metadata
- stronger project legibility through `ProjectOverlay`
- typed evidence paths in `gate`
- richer durable evidence in `Proofpack`
- blocked harness recovery surfaced in the ledger

## Proposed packet

The next design object should be:

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

This packet should remain:

- repo-local
- versioned
- inspectable
- bounded

## Relationship to current strategic tracks

Harness engineering should not replace the current roadmap.
It should sharpen several existing tracks:

- **Identity and layering** — harness is derived, not primitive
- **Work ledger** — harness outcomes and blocked recovery should remain inspectable
- **Project intelligence** — `ProjectOverlay` should surface harness capability state
- **Repo fixture matrix** — fixture coverage becomes part of harness discipline
- **Autonomous loop** — stronger autonomy depends on stronger validation surfaces

## Recommended rollout

### Slice 1

Docs-only architecture slice:

- `docs/product/HARNESS.md`
- this research note
- minimal cross-links in product architecture / roadmap docs

### Slice 2

Inspectable project harness packet:

- repo-local machine packet
- `punk inspect project` harness capability reporting

### Slice 3

Typed evidence execution:

- `gate` runs typed validation recipes
- `proof` persists evidence manifest refs and assertion results

### Slice 4

Stewardship / drift control:

- stale harness detection
- fixture coverage linkage
- recurring cleanup surfaces

## Acceptance evidence

This line of work is succeeding when:

- contributors can explain where harness lives in the architecture
- `inspect project` can answer what evidence surfaces are available
- `gate` can consume more than plain command checks
- blocked verification remains durable and inspectable through ledger/proof surfaces
- repo-local docs become more useful to agents without becoming a giant prompt blob
