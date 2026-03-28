# Prompt: Continue punk-run Implementation

Copy this into a new Claude Code session to continue work.

---

```
cd ~/personal/heurema/specpunk

Read these files in order:
1. docs/product/SESSION-HANDOFF.md — full state, what's done, what's not
2. docs/product/ROADMAP-v2.md — Phase 5 (zero-config) is next
3. docs/product/VISION.md — product spec (28 commands documented)
4. docs/product/ARCHITECTURE.md — technical spec (queue, adapters, goal system)

Current state:
- punk-run is PRODUCTION (dev.punk.daemon launchd, replaced bash supervisor 2026-03-28)
- 21 commits, ~14K LOC, 28 commands, 344 tests, 26 modules in punk-orch
- Binary: ~/.cargo/bin/punk-run (cargo install --git https://github.com/heurema/specpunk punk-run)
- Config: ~/.config/punk/{projects,agents,policy}.toml (currently required, Phase 5 makes optional)
- Bus: ~/vicc/state/bus/ (filesystem protocol: new/cur/done/failed/dead)

CRITICAL DESIGN PRINCIPLES (violating these = wrong):
1. CENTRAL ORCHESTRATOR — user never cd's into projects. punk-run dispatches INTO them.
2. ZERO-CONFIG FIRST — no config required before first successful task. TOML = optional override.
3. CLI SMART, DAEMON DUMB — CLI resolves project names, builds context, triages. Daemon only dispatches resolved absolute paths.
4. DON'T DUPLICATE — punk orchestrates. It doesn't replace Claude Code, arbiter, delve, or loci.
5. SCALE TO N PROJECTS — every feature must work across 5-10 projects from one place.

IDENTITY CHECK (run before writing ANY code):
- Does this feature assume CWD = project? → WRONG
- Does this require config before first success? → WRONG
- Does this make daemon smarter? → WRONG
- Does this duplicate existing tool? → WRONG

Next task: Phase 5 — Zero-Config (ROADMAP-v2.md)
Steps:
5.1 Project resolver: lazy cache + scan roots + ambiguity handling
5.2 Zero-config fallbacks: agents autodetect, built-in policy defaults
5.3 punk-run use/resolve/forget/projects/init commands
5.4 Config load fallback chain: TOML override > cache > autodetect > built-in
5.5 Remove mandatory TOML requirement from daemon + queue

Architecture (ADR-2026-03-28, arbiter consensus):
- Resolution chain: --path > pinned alias > scoped name > registries > cached discoveries > lazy scan > ask user
- Cache: ~/.cache/punk/projects.json (auto-generated, deletable)
- Three levels: L0 (zero files) > L1 (auto-cache) > L2 (TOML override)
- Daemon receives resolved absolute path in task.json. Never resolves names.
- Fuzzy match = suggestions only, never silent dispatch
- New commands: use, resolve, forget, projects (list), init (generate TOML from state)

Key code files:
- punk-orch/src/daemon.rs — main loop, dispatch, context assembly
- punk-orch/src/config.rs — TOML loading (needs fallback chain)
- punk-orch/src/context.rs — unified context (guidance+skills+recall+session)
- punk-orch/src/queue.rs — filesystem bus protocol
- punk-orch/src/triage.rs — auto-triage from prompt keywords
- punk-run/src/main.rs — CLI entry point (28 commands)

Tests: cargo test --all (344 pass, clippy -D warnings clean)
Build: cargo build -p punk-run --release && cp target/release/punk-run ~/.cargo/bin/

After Phase 5, remaining:
- OAuth API fast path (punk-run ask latency: 3s CLI → 200ms API)
- punk recall distillation (raw events → invariants pipeline)
- cargo publish to crates.io
- Close GitHub issues #8, #10, #11 (already resolved in code)
```
