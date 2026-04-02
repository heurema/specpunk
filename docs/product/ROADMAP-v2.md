# Specpunk v2 Roadmap — Agent Orchestration Platform

Last updated: 2026-03-27
Owner: Vitaly
Status: active

## Mission

Build a CLI-first agent orchestration platform that lets a solo founder set goals
and have AI agents autonomously plan, execute, test, and iterate — across multiple
projects and providers (Claude, Codex, Gemini) — on subscription billing.

## Primary UX Target

- Project bootstrap is an explicit admin action: `punk init [--project <id>] --enable-jj --verify`
- Default autonomous work intake is goal-only: `punk go --fallback-staged "<goal>"`
- Staged/manual work intake remains available: `punk start "<goal>"`
- Users describe the goal; the system decomposes it into contracts/tasks internally
- Target autonomous mode: `punk` should be able to go from goal to verified result without
  mandatory human approve steps or PR confirmations; approval/PR flows remain optional controls,
  not the only path

## North Star Metric

Goals completed per week with < 20% failure rate and < $50/month total spend.

## Reference Implementation

**Paperclip** (github.com/paperclipai/paperclip) is the primary reference.
Closest existing product to our vision. Cloned at `docs/reference-repos/paperclip/`.

Before implementing each step:
1. **Check Paperclip first** — how did they solve this? Read their code, not just docs.
2. **Check cloned references** — gstack (Garry Tan/YC), Hermes (NousResearch), Superpowers (obra) at `docs/reference-repos/`
3. **Check wider ecosystem** — see "Additional references" table below
4. **Adapt, don't copy** — they have PostgreSQL+React, we have flat files+CLI. Take the pattern, skip the stack.

### Additional References (not cloned, study per-phase)

| Project | Builder | Stars | Unique pattern for us |
|---------|---------|-------|-----------------------|
| [emdash](https://github.com/generalaction/emdash) | YC W26 | 2.9K | Closest competitor: 23+ CLI providers, git worktrees, local-first SQLite, SSH remote |
| [composio/agent-orchestrator](https://github.com/ComposioHQ/agent-orchestrator) | Composio (YC) | 4.9K | 8 swappable abstractions (runtime/tracker/SCM/notifier), auto-fix CI |
| [overstory](https://github.com/jayminwest/overstory) | Jaymin West | 1.1K | SQLite WAL as agent IPC bus (1-5ms), 11 runtime adapters |
| [aider](https://github.com/Aider-AI/aider) | Paul Gauthier | 40K | Architect mode (plan LLM -> exec LLM), git-first, repo map |
| [open-swe](https://github.com/langchain-ai/open-swe) | Harrison Chase / LangChain | 8.6K | Middleware safety hooks, Slack/Linear triggers |
| [goose](https://github.com/block/goose) | Block (Jack Dorsey) | 33K | Rust, MCP support, Linux Foundation governance |
| [OpenHands](https://github.com/OpenHands/OpenHands) | All Hands AI ($18.8M) | 65K | CodeAct: agent writes+executes Python instead of tool calls |
| [deer-flow](https://github.com/bytedance/deer-flow) | ByteDance | 37K | Skills as Markdown in git, versioned, subagents + sandbox |
| [smolagents](https://github.com/huggingface/smolagents) | HuggingFace | 25K | Code-first agents: Python blocks, not JSON tool calls |
| [simonw/llm](https://github.com/simonw/llm) | Simon Willison | 11.5K | Plugin arch: each provider = pip package. Mature extensibility |
| [OpenCode](https://github.com/sst/opencode) | SST (Jay+Dax Raad) | 131K | Client/server split: TUI is one client, server remotely accessible |
| [MassGen](https://github.com/massgen/MassGen) | Berkeley | 892 | Consensus voting: agents solve in parallel, observe, vote |

**Per-phase study guide:**
- Phase 1 (daemon): clone overstory (SQLite bus), composio (swappable layers), study goose Rust adapters
- Phase 2 (ops): study aider architect mode, open-swe middleware hooks
- Phase 3 (goals): study deer-flow skills-in-git, emdash local+remote dispatch
- Phase 4 (multi-model): study MassGen consensus, smolagents CodeAgent, simonw/llm plugin arch

Key reference files per phase:

### Phase 0: Receipt Schema + Status
| What to research | Paperclip | Others |
|-----------------|-----------|--------|
| Receipt/run data model | `packages/shared/src/types/heartbeat.ts` (HeartbeatRun schema) | Hermes `hermes_state.py` (SQLite session schema, what fields matter) |
| Cost tracking | `server/src/services/budgets.ts` (budget enforcement) | Hermes `agent/usage_pricing.py` + `agent/insights.py` (cost estimation per model) |
| Config structure | `packages/shared/src/config-schema.ts` (Zod validation) | gstack `bin/gstack-config` (YAML key-value, simple) |

### Phase 1: Typed Daemon
| What to research | Paperclip | Others |
|-----------------|-----------|--------|
| Dispatch + claim | `server/src/services/heartbeat.ts:1654` (claimQueuedRun, atomic CAS) | gstack `browse/src/server.ts` (file-based mutex lock, O_CREAT\|O_EXCL) |
| Heartbeat + orphan reaping | `server/src/services/heartbeat.ts:1732` (reapOrphanedRuns, PID liveness) | Hermes `run_agent.py:108` (_SafeWriter for daemon stdout) |
| Adapter invocation | `packages/adapters/claude-local/src/server/execute.ts` (env vars, session resume, skills tmpdir) | Hermes `agent/anthropic_adapter.py` (OAuth token -> Bearer auth + headers for subscription API) |
| Process lifecycle | `packages/adapter-utils/` (runChildProcess, spawn, kill) | gstack `browse/src/cli.ts:107` (PID kill with Windows workaround) |
| Retry + error handling | `server/src/services/heartbeat.ts:1780` (processLossRetryCount, auto-retry once) | Hermes `run_agent.py:6577` (429 -> fallback model immediately, backoff for others) |
| Wakeup coalescing | `server/src/services/heartbeat.ts:3076` (mergeCoalescedContextSnapshot) | — |

### Phase 2: Operations
| What to research | Paperclip | Others |
|-----------------|-----------|--------|
| CLI structure | `cli/src/index.ts` (Commander.js, 20+ commands) | Hermes `hermes_cli/main.py` (argparse, slash commands) |
| Doctor/health check | `cli/src/checks/` (8 check types: config, db, llm, port, secrets) | gstack `bin/gstack-update-check` (version check + snooze) |
| Dangerous command approval | — | Hermes `tools/approval.py` (22 regex patterns + Unicode NFKC + smart LLM judge) |
| Telemetry | — | gstack `bin/gstack-telemetry-log` (JSONL local + optional Supabase sync, never fatal) |

### Phase 3: Goal System
| What to research | Paperclip | Others |
|-----------------|-----------|--------|
| Task hierarchy | `doc/SPEC.md` section 5 (Initiative > Project > Milestone > Issue > Sub-issue) | Superpowers `skills/subagent-driven-development/SKILL.md` (controller > implementer > reviewer chain) |
| Inter-agent delegation | `doc/SPEC.md` section 3 (task acceptance rules, escalation protocol) | Hermes `tools/delegate_tool.py` (MAX_DEPTH=2, blocked tools, credential isolation) |
| Session continuity | `server/src/services/heartbeat.ts:888` (session compaction, handoff markdown) | Hermes `tools/memory_tool.py` (frozen snapshot, atomic write, injection scan) |
| Subagent status protocol | — | Superpowers `implementer-prompt.md` (DONE/DONE_WITH_CONCERNS/BLOCKED/NEEDS_CONTEXT) |
| Context compression | — | Hermes `agent/context_compressor.py` (5-phase, 50% threshold, 7-section summary template) |
| Verification gate | — | Superpowers `skills/verification-before-completion/SKILL.md` (gate function, red flags) |
| Re-planning | — | Autoresearch `program.md` (ratchet loop: modify -> evaluate -> keep/revert, NEVER STOP) |

### Phase 4: Multi-Model + Self-Improvement
| What to research | Paperclip | Others |
|-----------------|-----------|--------|
| Multi-adapter dispatch | `packages/adapters/` (7 adapters, each with execute/status/cancel) | gstack `/codex/SKILL.md` (cross-model diff analysis after review) |
| Smart routing | — | Hermes `agent/smart_model_routing.py` (160 chars, 28 words, 44 keyword blacklist) |
| Skill self-authoring | — | Hermes `tools/skill_manager_tool.py` (create/edit/patch, security scan, atomic write) |
| Skill quality | — | Superpowers `skills/writing-skills/SKILL.md` (TDD for prompts, pressure scenarios) |
| Persuasion engineering | — | Superpowers `skills/writing-skills/persuasion-principles.md` (Cialdini 7 principles, 33%->72%) |
| Mixture of agents | — | Hermes `tools/mixture_of_agents_tool.py` (4 models parallel -> aggregator) |
| Eval system | — | gstack `test/helpers/eval-store.ts` (tier system, diff-based selection, planted-bug detection) |
| RL from traces | — | Hermes `batch_runner.py` + `trajectory_compressor.py` + `environments/agentic_opd_env.py` (reference only, we don't train models) |

## Process Rule

Each roadmap step follows: **research -> design -> implement -> test**.
Never jump to implementation from the step title alone.

---

## Phase 0: Foundation

**Goal:** Bootstrap punk-orch + punk-run crates. Prove receipt-based architecture on top of existing bash supervisor.
**Duration:** ~1 week

| Step | What | Effort | Blocks |
|------|------|--------|--------|
| 0.0 | `cargo init punk-orch` + `cargo init punk-run`, add to workspace | 1h | Everything |
| 0.1 | receipt.schema.json v1 + validation in bash supervisor | 2-4h | 0.2 |
| 0.2 | receipts/index.jsonl append in bash | 2h | 0.3 |
| 0.3 | `punk-run status` command (reads bash supervisor state, read-only) | 1d | morning |
| 0.4 | TOML config (optional override) + `punk-run config` + built-in defaults | 1d | Phase 1 |

**Architecture note:** punk-core/punk-cli = FROZEN (verification). punk-orch/punk-run = all new code. Same workspace, separate namespaces. No module conflicts.

**Done when:** `punk-run status` shows real tasks from live bash supervisor.

---

## Phase 1: Typed Daemon

**Goal:** Rust daemon replaces bash supervisor. Same bus, same receipts, type safety.
**Duration:** ~2 weeks

| Step | What | Effort | Blocks |
|------|------|--------|--------|
| 1.1 | Queue protocol: claim, heartbeat, stale detection, fair share | 3d | 1.2 |
| 1.2 | Adapter layer: Claude/Codex/Gemini/process, personality, skills | 3d | 1.3 |
| 1.3 | Run/Attempt entity, failure taxonomy, retry, circuit breaker, dead-letter | 2d | 1.4 |
| 1.4 | Shadow mode, compare with bash, daemon recovery, watchdog, cutover | 3d | Phase 2 |

**All code in punk-orch/ and punk-run/. punk-core/punk-cli untouched.**

**Done when:** bash supervisor decommissioned, `punk-run daemon` handling all tasks for 1 week without incident.

---

## Phase 2: Operations Layer

**Goal:** Human sees everything, controls everything, no micromanagement needed.
**Duration:** ~1 week

| Step | What | Effort | Blocks |
|------|------|--------|--------|
| 2.1 | `punk morning` (briefing: receipts + goals + pipeline + directives) | 1d | — |
| 2.2 | `punk triage` (dead-letter interactive review) | 4h | — |
| 2.3 | `punk approve/cancel/retry` (T3 gate, task control) | 4h | — |
| 2.4 | `punk pipeline` (flat JSONL CRM: add, advance, win, lose, stale) | 4h | — |
| 2.5 | `punk ask` (AI query with provenance, deterministic fallback) | 1d | — |
| 2.6 | `punk doctor` (health check: providers, auth, queue) | 4h | — |

**All commands are `punk-run <cmd>`, not `punk <cmd>`.**

**Done when:** Complete daily workflow: `punk-run morning` -> `punk-run status` -> `punk-run triage` -> `punk-run ask` -> `punk-run pipeline`.

---

## Phase 3: Goal System

**Goal:** Human sets objective, system autonomously plans and executes.
**Duration:** ~1.5 weeks

| Step | What | Effort | Blocks |
|------|------|--------|--------|
| 3.1 | Planner agent: `punk goal`, plan generation, human approval | 2d | 3.2 |
| 3.2 | Goal evaluation loop in daemon: step tracking, sub-task creation, re-plan triggers | 2d | 3.4 |
| 3.3 | Session context: frozen snapshot, typed entries, TTL, security scan | 1d | — |
| 3.4 | Goal CLI: goals, goal status, pause/resume/cancel/replan/budget | 1d | — |

**Done when:** `punk-run goal signum "prepare checkpoint"` runs autonomously from plan to completion.

**Merge milestone:** After Phase 3 stabilizes, consider merging punk-cli commands into punk-run. Rename punk-run -> punk. Archive punk-cli as punk-verify.

---

## Phase 4: Multi-Model + Self-Improvement

**Goal:** Full multi-provider capabilities and system that learns from its own output.
**Duration:** ~1.5 weeks

| Step | What | Effort | Blocks |
|------|------|--------|--------|
| 4.1 | `punk diverge` (3-provider parallel, decision matrix) | 2d | — |
| 4.2 | `punk panel` (ask all providers, compare) | 4h | — |
| 4.3 | Budget backpressure (auto-throttle at 80/90/95%) | 4h | — |
| 4.4 | Skill self-authoring (agent creates skills after complex tasks) | 1d | — |
| 4.5 | Metric ratchet (weekly performance comparison, directive on degradation) | 4h | — |
| 4.6 | `punk graph` (on-demand cost/gantt charts) | 1d | — |

**Done when:** System self-improves: skills created, metrics tracked, budget enforced, multi-model diverge produces better solutions than single-model.

---

## Total: ~7 weeks

Each step is independently testable. No big-bang.
Phase 0: new crates + receipt schema on existing bash supervisor — zero risk.
Phase 1: punk-run daemon in shadow mode before cutover.
Phase 3: goals = the differentiator vs Paperclip.
Phase 4: polish and optimization.

**Binary strategy:** punk-run is the new product. punk (verify) stays frozen.

---

## Phase 5: Zero-Config & Polish

**Goal:** Remove mandatory TOML setup. First task dispatches without any config files.
**Status:** designed (2026-03-28, arbiter panel consensus: Codex + Gemini + Claude)

| Step | What | Effort | Blocks |
|------|------|--------|--------|
| 5.1 | Project resolver: lazy cache + scan roots + ambiguity handling | 1d | 5.2 |
| 5.2 | Zero-config fallbacks: agents autodetect, built-in policy defaults | 4h | 5.3 |
| 5.3 | `punk-run use/resolve/forget/projects/init` commands | 4h | — |
| 5.4 | Config load fallback chain: TOML override > cache > autodetect > built-in | 4h | — |
| 5.5 | Remove mandatory TOML requirement from daemon + queue | 2h | — |

**Architecture decision (ADR-2026-03-28):**
- projects.toml = OPTIONAL override, not mandatory. Cache = `~/.cache/punk/projects.json`
- agents.toml = OPTIONAL override. Agents autodetected from PATH.
- policy.toml = OPTIONAL override. Built-in safe defaults in code.
- Resolution chain: CLI resolves project name → absolute path → writes to task.json. Daemon only dispatches resolved paths.
- Three levels: L0 (zero files), L1 (auto-cache), L2 (TOML override)
- Identity check: punk = central orchestrator, NOT per-project tool. Never assume CWD = project.

**Done when:** `cargo install punk-run && punk-run queue signum "fix bug"` works without any config files.

---

## Design Principles (applies to ALL phases)

1. **Central orchestrator** — user never cd's into projects. punk-run dispatches INTO them.
2. **Zero-config first** — no config required before first successful task. TOML = optional override.
3. **CLI smart, daemon dumb** — CLI resolves names, builds context, triages. Daemon only dispatches resolved tasks.
4. **Don't duplicate** — punk orchestrates. It doesn't replace Claude Code, arbiter, delve, or loci.
5. **Scale to N projects** — every feature must work across 5-10 projects from one place.
Merge into one binary only after Phase 3 stabilizes + product narrative clear.

---

## Documents

| Document | Purpose |
|----------|---------|
| `VISION.md` | Product north star, core abstractions, CLI surface |
| `ARCHITECTURE.md` | Executable spec: queue protocol, auth, budget, failure taxonomy, goal system |
| `ROADMAP-v2.md` | This file: granular implementation steps |
| `docs/research/2026-03-27-*.md` | Research: reference repos, technical findings, memory, self-improvement |
