# Session Handoff: Phase 0 Start

## Context
Specpunk v2 — agent orchestration platform. Full research + architecture done 2026-03-27.
This session starts implementation: Phase 0, Step 0.0.

## Read These First (in order)
1. `docs/product/VISION.md` — what we're building (one-pager, ~800 lines)
2. `docs/product/ARCHITECTURE.md` — how we're building it (executable spec, ~700 lines)
3. `docs/product/ROADMAP-v2.md` — step-by-step plan with reference tables

## Key Decisions (don't re-discuss, already validated by Codex+Gemini quorum)
- **Two binaries**: punk-cli (verify, FROZEN) + punk-run (orchestrate, NEW). Same Cargo workspace.
- **Rust patterns**: hexagonal traits + enum dispatch + tokio channels. No actors, no ECS.
- **Auth**: subscription CLI first, OAuth API best-effort (Claude only), no paid API keys.
- **State**: flat files (JSONL, JSON). No database.
- **Memory**: 5 layers (working/session/receipts/engram/skills). Frozen snapshot pattern.
- **adjutant**: killed, pipeline absorbed as punk-run pipeline (flat JSONL).
- **Goals**: human sets objective -> planner agent -> autonomous execution with eval loop.

## Current Step: Phase 0.0
```
0.0  cargo init punk-orch + cargo init punk-run, add to workspace   [1h]
0.1  receipt.schema.json v1 + validation in bash supervisor          [2-4h]
0.2  receipts/index.jsonl append in bash                             [2h]
0.3  punk-run status command (reads bash supervisor state)           [1d]
0.4  projects.toml + agents.toml + policy.toml + punk-run config    [1d]
```

## Reference Repos (cloned at docs/reference-repos/)
- paperclip — primary reference (Paperclip = closest to our vision)
- gstack, hermes-agent, superpowers — secondary references

## Research Documents
- `docs/research/2026-03-27-reference-repos-research-plan.md`
- `docs/research/2026-03-27-step-01-cli-topology.md`
- `docs/research/2026-03-27-technical-deep-dive-findings.md`
- `docs/research/2026-03-27-memory-and-self-improvement.md`
- `docs/research/2026-03-27-rust-architecture-patterns.md`

## Workspace Structure (target)
```
specpunk/punk/
  Cargo.toml          # workspace: [punk-core, punk-cli, punk-orch, punk-run]
  punk-core/          # FROZEN — verification library (14K LOC)
  punk-cli/           # FROZEN — punk binary (verify commands)
  punk-orch/          # NEW — orchestration library
  punk-run/           # NEW — punk-run binary
```

## Process Rule
Each step: research reference repos -> design -> implement -> test.
Check Paperclip first for each step (see ROADMAP-v2.md reference tables).
