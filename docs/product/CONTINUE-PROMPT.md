# Prompt: Continue `punk` Rebuild

Copy this into a new coding session to continue the rebuild.

---

```text
cd ~/personal/heurema/specpunk

Read these files in order:
1. README.md
2. docs/product/VISION.md
3. docs/product/ARCHITECTURE.md
4. docs/product/MASTER-PLAN.md
5. docs/product/COUNCIL.md
6. docs/product/SKILLS.md
7. docs/product/EVAL.md
8. docs/product/RESEARCH.md
9. docs/product/DOGFOODING.md
10. docs/product/CLI.md

Current state:
- This repo is in a clean-cut redesign phase.
- Public product surface is `punk`, not `punk-run`.
- No backward compatibility is required.
- Legacy code under `punk/` is source material for extraction, not target architecture.

Target product:
- one CLI: `punk`
- canonical modes: `plot`, `cut`, `gate`
- local-first
- `jj` preferred, `git` fallback
- event log as runtime source of truth
- feature-centric flow, not PR-centric flow
- long-term shape: stewarded multi-agent engineering runtime

Four pillars:
1. Kernel — small stable Rust core with replaceable edges
2. Stewardship — cleanup, docs parity, no drift, coherent project state
3. Council — selective multi-model and multi-role deliberation for high-stakes work
4. Skill/Eval ratchet — project-specific skills that improve through evidence and promotion

Canonical object chain:
Project -> Goal -> Feature -> Contract -> Task -> Run -> Receipt -> DecisionObject -> Proofpack

Critical laws:
1. One CLI: `punk`
2. One vocabulary: `plot / cut / gate`
3. Contract first: `cut` runs against approved contracts
4. Gate writes the final decision
5. Proof before acceptance
6. `jj` preferred, `git` fallback
7. Optimize for clean target architecture, not legacy compatibility
8. Council is advisory; gate is final
9. Skills evolve through curated ratchet, not silent mutation
10. Self-hosting is bounded; meta-level changes require stronger review than ordinary feature work

Current implemented baseline:
- repo-root Cargo workspace
- crates/
  - punk-cli
  - punk-domain
  - punk-events
  - punk-vcs
  - punk-core
  - punk-orch
  - punk-gate
  - punk-proof
  - punk-adapters
- `plot contract`, `plot refine`, `plot approve`
- `cut run`
- `gate run`
- `gate proof`
- `status`
- `inspect --json`
- event log + repo-local artifacts
- hybrid `plot` drafting: deterministic scan + Codex structured draft + validation + one repair pass

Storage split:
- ~/.punk/ = global config, event log, materialized views
- .punk/ = repo-local contracts, runs, decisions, proofs

Near-term implementation order:
1. Keep improving the base `plot -> cut -> gate` loop
2. Add thin `punk-shell` over the same services
3. Add `punk-council` for architecture/contract/review protocols
4. Add skills/eval ratchet subsystem
5. Add bounded deep research mode

Detailed subsystem specs now exist in:
- docs/product/COUNCIL.md
- docs/product/SKILLS.md
- docs/product/EVAL.md
- docs/product/RESEARCH.md

Do not:
- preserve `punk-run` public surface
- reintroduce old daemon-first architecture into v0
- let council write final decisions
- let skills mutate silently from task success alone
- let the system self-certify sensitive meta-level changes without stronger review
- invent second vocabularies like `plan/forge/proof`
```
