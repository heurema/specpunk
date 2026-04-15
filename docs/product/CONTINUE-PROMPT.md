# Prompt: Continue `punk` Rebuild

Copy this into a new coding session to continue the rebuild.

---

```text
cd ~/personal/heurema/specpunk

Read these files in order:
1. README.md
2. docs/product/CURRENT-ROADMAP.md
3. docs/product/ADR-provider-alignment.md
4. docs/product/ARCHITECTURE.md
5. docs/product/CLI.md
6. docs/product/VISION.md
7. docs/product/IMPLEMENTATION-STATUS.md
8. docs/product/REPO-STATUS.md
9. docs/product/ACTION-PLAN.md
10. docs/product/NORTH-ROADMAP.md
11. docs/product/MASTER-PLAN.md

Only then open subsystem specs if your slice actually touches them:
- docs/product/COUNCIL.md
- docs/product/SKILLS.md
- docs/product/EVAL.md
- docs/product/RESEARCH.md
- docs/product/DOGFOODING.md

Current repo truth:
- Public product surface is `punk`.
- One vocabulary: `plot / cut / gate`.
- No backward compatibility is required.
- Legacy code under `punk/` is source material for extraction, not target architecture.

Status vocabulary:
- active v0 surface
- in-tree but inactive
- planned only

Current crates:
- active v0 surface: `punk-cli`, `punk-domain`, `punk-events`, `punk-vcs`, `punk-core`, `punk-orch`, `punk-gate`, `punk-proof`, `punk-adapters`
- in-tree but inactive: `punk-council`
- planned only as separate crates: `punk-shell`, `punk-skills`, `punk-eval`, `punk-research`

Current operator/default path:
- `punk init --enable-jj --verify`
- `punk go --fallback-staged "<goal>"`
- `punk status [id]`
- `punk inspect project`
- `punk inspect work [id]`

Current expert/control surfaces:
- `punk start "<goal>"`
- `punk plot ...`
- `punk cut run ...`
- `punk gate ...`
- `punk research start|artifact|synthesize|complete|escalate`

Important distinctions:
- `punk go --fallback-staged` already exists today as a shell mechanism.
- The standalone `Goal` primitive is still deferred from the current v0 domain/runtime.
- `punk research ...` already exists today as a bounded capability in `punk-cli` + `punk-orch` + `punk-domain`.
- The dedicated `punk-research` crate is still planned only.
- `punk-council` is buildable in-tree but inactive and not part of the current default operator path.
- The future interactive `punk-shell` crate does not exist yet.

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

Do not:
- preserve `punk-run` public surface
- describe planned-only crates as current operator surface
- describe in-tree but inactive council as active default behavior
- describe `punk go` as proof that the standalone `Goal` primitive already exists
- describe current `punk research ...` commands as proof that `punk-research` already exists as a crate
- reintroduce old daemon-first architecture into v0
- let council write final decisions
- let skills mutate silently from task success alone
- invent second vocabularies like `plan/forge/proof`
```
