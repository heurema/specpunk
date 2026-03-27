# Research Plan: Reference Repos Deep Study

**Goal:** Extract architectural decisions, patterns, and lessons from 4 reference repos to define the architecture for specpunk's autonomous agent orchestration system (punk-supervisor successor).

**Reference repos:**
- **paperclip** (paperclipai) — full-featured agent orchestration, TypeScript/Node.js, PostgreSQL, React UI
- **gstack** (garrytan) — sprint process toolkit, Markdown-as-code skills + Playwright browser daemon
- **hermes-agent** (NousResearch) — interactive AI agent CLI, Python, multi-provider, messaging gateway
- **superpowers** (obra) — prompt engineering library, pure Markdown skills, subagent patterns

**Our current state:**
- **punk-supervisor** — bash daemon, filesystem bus, multi-model dispatch (claude/codex/gemini), 5 slots
- **specpunk/punk** — Rust CLI, spec-driven verification (init/plan/check/receipt), v0.1 shipped
- **adjutant** — Rust CLI, business operations pipeline, MVPs 1-13 done
- **cycle-controller** — daily signal generator (directives)

---

## Research Steps

### Step 1: CLI Topology — One Binary vs Unix-Style Many
**Question:** Should we build one monolith CLI (like Paperclip/Hermes) or keep separate unix-style tools (punk + adjutant + supervisor + cycle)?

**What to extract:**
- Paperclip: monorepo with single `paperclipai` binary — how does it handle the scope?
- Hermes: 3 entry points (hermes, hermes-agent, hermes-acp) from one package — why?
- gstack: skills + browse binary + bash scripts — effectively distributed
- superpowers: no CLI at all — pure plugin

**Sub-questions:**
- How do users discover and navigate commands in each?
- What's the cost of a monolith vs unix-style for solo dev maintenance?
- How do they handle cross-cutting concerns (config, auth, state)?

---

### Step 2: Core Abstractions — What Are the Right Nouns?
**Question:** What entities/abstractions should the system have?

**What to extract:**
- Paperclip: Company > Agent > Issue (Initiative > Project > Milestone > Issue > Sub-Issue) > HeartbeatRun
- Hermes: AIAgent > Session > Tool > Toolset > Memory > Skill
- gstack: Skill > Sprint > ReviewLog > BrowseRef
- superpowers: Skill > Spec > Plan > Subagent (Implementer/Reviewer)
- Our current: Project > Task (JSON) > Receipt > Contract > Directive

**Sub-questions:**
- Do we need Company/Organization isolation? (solo founder: probably not now)
- Issue hierarchy depth — flat tasks vs deep hierarchy?
- What's the right task lifecycle? (Paperclip's atomic checkout is interesting)

---

### Step 3: Adapter Pattern — Multi-Model Abstraction
**Question:** How to abstract AI providers cleanly?

**What to extract:**
- Paperclip: adapter registry with `execute()`, `status()`, `cancel()` per adapter. 7+ adapters. Session codecs per adapter.
- Hermes: OpenAI-compatible client as universal abstraction + native Anthropic adapter. Fallback model chain.
- gstack: delegates to Claude Code / Codex CLI / Gemini CLI directly (no abstraction layer)
- Our current: punk-dispatch.sh handles claude/codex/gemini with shell conditionals

**Sub-questions:**
- Paperclip's adapter pattern vs Hermes's "everything is OpenAI-compatible" — which scales better?
- How to handle adapter-specific features (Claude's extended thinking, Codex's sandbox)?
- Session codec pattern — worth it for session continuity across providers?

---

### Step 4: Orchestration — Scheduling & Dispatch
**Question:** How should agents be scheduled and dispatched?

**What to extract:**
- Paperclip: heartbeat timer + routine scheduler + wakeup coalescing. `maxConcurrentRuns` per agent. Atomic DB claim.
- Hermes: synchronous loop (one agent at a time), delegation for parallelism (max 3 children, depth 2)
- gstack: no scheduler — user invokes skills manually, autoplan chains them
- Our current: filesystem bus polling (5s), priority queues (p0/p1/p2), slot-based concurrency (max 5)

**Sub-questions:**
- Timer-based (heartbeat) vs queue-based (bus) vs event-driven — tradeoffs?
- How does Paperclip handle task dependency chains?
- Wakeup coalescing pattern — applicable to us?
- punk-diverge (synchronous scatter/gather) vs Paperclip's parallel heartbeats

---

### Step 5: State & Persistence
**Question:** What storage backend and state model to use?

**What to extract:**
- Paperclip: PostgreSQL (embedded or external), Drizzle ORM, full relational model
- Hermes: SQLite WAL + FTS5, session-scoped, lightweight
- gstack: flat files (JSON, YAML, JSONL), no database
- superpowers: git is the only state
- Our current: flat files (JSON in bus/), audit.jsonl, digest.jsonl

**Sub-questions:**
- At what scale does flat-file break? (Our bus handles ~50 tasks/day fine)
- SQLite as middle ground — Hermes proves it works for sessions + FTS
- Do we need relational queries? (cost tracking, audit, analytics)
- Paperclip's embedded-postgres pattern — overkill for solo founder?

---

### Step 6: Memory Architecture
**Question:** How should agents remember things across sessions?

**What to extract:**
- Paperclip: task/comment system IS memory. Planned 2-layer memory API not yet built.
- Hermes: MEMORY.md + USER.md frozen snapshot at session start. Skills as procedural memory. Honcho for user modeling.
- gstack: no memory — reads project files (CLAUDE.md, DESIGN.md) as context
- superpowers: no memory — git is memory, skills re-injected every session
- Our current: Engram (3-layer: handbook L1, engram MCP L2, auto-memory L3)

**Sub-questions:**
- Hermes's "frozen snapshot" approach — prevents mid-session memory drift, preserves prompt cache. Apply to punk?
- Should agents build shared memory or isolated per-agent memory?
- Context compression algorithms — Hermes's 50% threshold + structured summary
- Skills as procedural memory — how Hermes auto-creates skills after complex tasks

---

### Step 7: Reliability & Error Handling
**Question:** How to make autonomous agents reliable?

**What to extract:**
- Paperclip: orphan reaping, budget hard stop, process loss retry (1x), PID recycling awareness
- Hermes: _SafeWriter for daemon mode, message sanitizer, tool call deduplication, fuzzy name repair
- gstack: crash-and-restart philosophy, health endpoint, version mismatch auto-restart, AI-readable errors
- superpowers: 3-fix escalation rule, SUBAGENT-STOP guards
- Our current: watchdog, heartbeat monitoring, SIGKILL escalation, slot cleanup, retry for transient errors

**Sub-questions:**
- Hermes's tool call deduplication and fuzzy repair — do we need this?
- gstack's "don't self-heal, crash and let CLI restart" vs Paperclip's orphan reaping
- Budget enforcement: Paperclip's hard/soft ceilings vs our simple max_budget_usd
- superpowers's 3-fix escalation — integrate into punk check?

---

### Step 8: Human-in-the-Loop & Governance
**Question:** How to balance autonomy with human control?

**What to extract:**
- Paperclip: approval gates for agent hires + CEO strategy. Board principal. Budget pause.
- Hermes: dangerous command approval (20+ patterns + unicode normalization + LLM fallback)
- gstack: autoplan presents only "taste decisions" to user
- superpowers: spec compliance reviewer as verification gate
- Our current: risk tiers (T1/T2/T3), pending/ gate for T2/T3, quality-rules.md injection

**Sub-questions:**
- Paperclip's board approval model — too complex for solo founder?
- "Taste decisions only" pattern (gstack) — which decisions can be fully automated?
- Risk tier → approval requirement mapping
- How to surface results for human review efficiently?

---

### Step 9: Multi-Model Strategy & Smart Routing
**Question:** How to use multiple AI models effectively?

**What to extract:**
- Paperclip: one adapter per agent, no auto-routing between models
- Hermes: smart_model_routing (cheap for simple, expensive for complex), MoA (4 models → aggregator), fallback chain
- gstack: /codex skill for second opinion, cross-model diff analysis
- superpowers: model selection policy in subagent dispatch (cheap/standard/most-capable)
- Our current: per-task model selection, punk-diverge (3 models parallel), arbiter (panel/quorum modes)

**Sub-questions:**
- Hermes's smart routing heuristic (160 chars, 28 words threshold) — too simple?
- MoA pattern — when is 4-model parallel worth the cost?
- gstack's cross-model diff analysis — integrate into punk-diverge scoring?
- Automatic vs manual model selection

---

### Step 10: Issues, Bugs, User Pain Points
**Question:** What problems have users reported? What can we learn from their mistakes?

**What to extract:**
- Paperclip GitHub issues: adapter failures, session bugs, budget edge cases
- gstack TODOS.md: P0-P4 prioritized issues, known bugs
- Hermes: AGENTS.md pitfalls, test bug annotations, config loader duplication
- superpowers: platform fragmentation, no enforcement mechanism, sequential bottleneck

**Sub-questions:**
- Common failure modes across all 4 repos
- What users want most (from issues)
- What's hardest to get right (from bug density)
- Anti-patterns to avoid

---

### Step 11: Synthesis — Architecture Blueprint
**Question:** Given all findings, what's the target architecture?

**Deliverable:** Architecture decision document with:
- CLI topology decision (with tradeoffs table)
- Core abstractions and their relationships
- State model choice
- Orchestration model
- Multi-model strategy
- Reliability guarantees
- Implementation phases

---

## Execution Protocol

1. We go through steps sequentially
2. Each step: deep dive into the specific aspect across all 4 repos
3. Extract findings into a structured comparison
4. Discuss tradeoffs and make preliminary decisions
5. Document in `docs/research/` per step
6. Step 11 synthesizes everything into the architecture blueprint

## Files Created
- Reference repos: `docs/reference-repos/{paperclip,gstack,hermes-agent,superpowers}/`
- Per-step research: `docs/research/2026-03-27-step-N-<topic>.md`
- Final blueprint: `docs/research/2026-03-27-architecture-blueprint.md`
