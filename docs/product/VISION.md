# Specpunk Vision

Agent orchestration platform for solo founders running AI agent fleets.
CLI-first. No dashboard. Agents are the operators, humans steer.

---

## What Specpunk Is

Specpunk is a control plane for AI agents. It dispatches tasks to agents running on different models and providers, tracks their work through structured receipts, enforces budgets, and gives the human operator a query interface instead of a dashboard.

Think Paperclip, but:
- **CLI-first** - no web UI, no React, no PostgreSQL
- **Agent-first** - the primary operator is an AI agent, not a human at a dashboard
- **On-demand rendering** - reports, graphs, tables generated when asked, not maintained
- **Built-in verification** - every agent's output can be scope-checked before acceptance
- **Solo founder scale** - one human, multiple AI agents, multiple projects

## What Specpunk Is Not

- Not a coding agent (Claude Code, Codex, Cursor do that)
- Not a SaaS platform (runs locally, single-tenant)
- Not a framework or SDK (it's a finished tool you install and use)
- Not a dashboard you stare at (you ask questions, you get answers)

---

## Core Abstractions

### Goal

The primary interface. Human sets a high-level objective, system autonomously plans and executes.

| Field | Type | Notes |
|-------|------|-------|
| id | string | `signum-checkpoint-20260327` |
| project | string | Project slug |
| objective | string | What to achieve (human language) |
| deadline | date? | Optional target date |
| budget_usd | float | Total budget for this goal |
| spent_usd | float | Running total from receipts |
| status | enum | `planning`, `active`, `paused`, `done`, `failed` |
| plan_version | int | Incremented on re-plan |

Goals are the answer to "I don't want to micromanage." Human says what, system figures out how.

**Lifecycle:**
```
punk goal <project> "objective"
  -> planner agent generates plan (5-15 steps)
  -> human approves plan (one time)
  -> daemon executes steps autonomously
  -> after each step: evaluate, create sub-tasks if needed
  -> re-plan if off track (human approves significant changes)
  -> goal DONE when objective met or all steps complete
```

**Autonomy within a step:** Agents can create sub-tasks within their step without human approval. Fix a bug they found, run tests, refactor a file. All tracked as sub-tasks under the parent step.

### Project

A codebase or initiative that agents work on. Maps to a git repo or a logical unit of work.

| Field | Type | Notes |
|-------|------|-------|
| id | string | Slug: `signum`, `context8`, `mycel` |
| path | path | Filesystem path to the project root |
| stack | string | Language/framework hint for agent context |
| active | bool | Whether agents should work on this |
| budget_usd | float | Monthly spend ceiling for this project |
| checkpoint | date | Next review date |

Projects are defined in a single `projects.toml` file.

### Agent

An AI worker with a specific provider, model, and role. Agents are not persistent processes - they are invoked per-task.

| Field | Type | Notes |
|-------|------|-------|
| id | string | `claude-sonnet`, `codex-auto`, `gemini-flash` |
| provider | enum | `claude`, `codex`, `gemini`, `custom` |
| model | string | Provider-specific model ID |
| role | string | `engineer`, `reviewer`, `researcher`, `scout` |
| budget_usd | float | Per-invocation spend ceiling |
| allowed_tools | string[] | Tool whitelist for this agent |
| adapter_config | table | Provider-specific settings |

Agents are defined in `agents.toml`. The same provider can have multiple agent configurations (e.g., `claude-opus-reviewer` and `claude-sonnet-coder`).

### Task

A unit of work assigned to an agent. Lifecycle: `queued -> claimed -> running -> done | failed`.

| Field | Type | Notes |
|-------|------|-------|
| id | string | `signum-20260327-140000` |
| project | string | Project slug |
| agent | string | Agent ID (or `auto` for routing) |
| prompt | string | What the agent should do |
| category | enum | `codegen`, `research`, `fix`, `review`, `content`, `audit` |
| priority | enum | `p0` (critical), `p1` (normal), `p2` (background) |
| risk_tier | enum | `T1` (auto), `T2` (review result), `T3` (approve before run) |
| budget_usd | float | Max spend for this task |
| timeout_s | int | Max duration |
| depends_on | string[] | Task IDs that must complete first |
| worktree | bool | Run in isolated git worktree |
| template | string | Template name for prompt generation |

Tasks are JSON files in the bus directory. Created by `punk queue`, templates, or programmatically.

### Receipt

The output of every completed task. Structured, versioned, append-only.

| Field | Type | Notes |
|-------|------|-------|
| task_id | string | Links to the task |
| schema_version | int | Currently `1` |
| status | enum | `success`, `failure`, `timeout`, `cancelled` |
| agent | string | Which agent ran it |
| model | string | Actual model used |
| project | string | Project slug |
| category | string | Task category |
| tokens_used | int | Total tokens consumed |
| cost_usd | float | Actual spend |
| duration_ms | int | Wall-clock time |
| exit_code | int | Process exit code |
| artifacts | string[] | Files created/modified |
| errors | string[] | Error messages if any |
| call_style | enum | `tool_use`, `function_declarations`, `plain_text` |
| parent_task_id | string? | For sub-agent chains |
| punk_check_exit | int? | If verification ran: 0=pass, 1=scope violation |
| summary | string | One-line human-readable summary |
| created_at | datetime | When the task completed |

Receipts are the single source of truth. Every query, report, and budget calculation reads receipts.

### Directive

A signal from the system to the human operator. Generated by watchers that monitor state.

| Field | Type | Notes |
|-------|------|-------|
| severity | enum | `crit` (000), `warn` (100), `info` (200) |
| type | string | `review_due`, `budget_alert`, `checkpoint`, `stale_task` |
| message | string | Human-readable description |
| action | string? | Suggested next step |
| created_at | datetime | When generated |

Directives replace the dashboard. Instead of checking a UI, the operator gets a morning briefing.

### Session

Per-project rolling context that gives agents memory across tasks.

| Field | Type | Notes |
|-------|------|-------|
| project | string | Project slug |
| entries | object[] | Last N key facts from receipts |
| entry.type | enum | `success`, `failure`, `surprise`, `cost_overrun` |
| entry.fact | string | What happened |
| entry.task_id | string | Source task |
| entry.ttl_tasks | int | Expires after N more tasks |

Sessions are JSON files, not a database. Capped at 10 entries with TTL-based eviction.

### Pipeline

Lightweight CRM for tracking business opportunities. Flat-file, not a database.

| Field | Type | Notes |
|-------|------|-------|
| id | int | Auto-increment |
| project | string | Project slug |
| contact | string | Person name |
| stage | enum | `lead`, `qualified`, `proposal`, `negotiation`, `won`, `lost` |
| next_step | string | What to do next |
| due | date | When next_step is due |
| value_usd | int? | Estimated deal value |

Storage: `state/pipeline.jsonl` - one line per event (stage change). Current state = last entry per id.

Replaces adjutant's SQLite CRM. Sufficient for 0-50 opportunities. If pipeline grows beyond that, upgrade to SQLite backend.

### Policy

Declarative rules for routing, budgets, and behavior. Single TOML file.

```toml
[defaults]
model = "sonnet"
budget_usd = 1.0
timeout_s = 600
max_slots = 5

[[rules]]
match = { project = "agentfuzz", category = "codegen" }
set = { model = "opus", budget_usd = 3.0 }

[[rules]]
match = { priority = "p0" }
set = { timeout_s = 1800, retry = 2 }

[budget]
monthly_ceiling_usd = 50.0
soft_alert_pct = 80
hard_stop_pct = 95
```

Policy is the single place for all scheduling decisions. Additive overlay: task-level fields take precedence.

---

## System Architecture

Two binaries in one Cargo workspace. Merge into one after orchestration stabilizes.

```
                       human (terminal)
                       /            \
              punk (verify)     punk-run (orchestrate)
              init/plan/check   goal/queue/run/status/ask
                    |                    |
                    |              punk-run daemon
                    |              /    |    \
                    |           slot  slot  slot
                    |            |     |     |
                    |          claude codex gemini
                    |            |     |     |
                    |            v     v     v
                    |          receipt receipt receipt
                    |            \     |     /
                    +------>  receipts/index.jsonl
                 (punk check       |
                  as post-gate)    |
                             state/ directory
                             (flat files, JSONL)
```

**Two binaries:**
- `punk` — existing verification CLI (14K LOC, shipped v0.1, FROZEN)
- `punk-run` — new orchestration CLI (Phase 0+, all new code)

**Workspace layout:**
```
specpunk/punk/
  Cargo.toml          # workspace members
  punk-core/          # FROZEN — verification library
  punk-cli/           # FROZEN — `punk` binary
  punk-orch/          # NEW — orchestration library
  punk-run/           # NEW — `punk-run` binary
```

**Why two binaries:** 5 blocking module name conflicts (session, policy, receipt, status, config) between verification and orchestration. Clean separation = zero legacy risk. Merge planned after receipt v1, state model, and policy engine stabilize (Phase 2+).

### No Database

State lives in flat files:
- `state/bus/` - task lifecycle directories (new/, cur/, done/, failed/)
- `state/receipts/index.jsonl` - aggregated receipt index
- `state/sessions/` - per-project agent context
- `state/cycle/directive.md` - current directives
- `state/audit.jsonl` - event log

SQLite is not needed at this scale. Flat files are grep-able, human-readable, and work with any tool.

### No Web Server

No HTTP API, no REST endpoints, no WebSocket. Agents interact with the system through:
1. **Task files** - agent reads its task spec from a JSON file
2. **Receipt files** - agent writes its receipt on completion
3. **Filesystem bus** - directories as state machine (new/ -> cur/ -> done/)

The bus protocol is the API. Any process that can read/write files can be an agent.

### No Dashboard

Instead of a persistent UI:
- `punk status` - deterministic terminal table (always works, no AI)
- `punk ask "question"` - AI-powered query over state (haiku, fast)
- `punk graph --type cost --since 14d` - on-demand visualization
- `punk morning` - daily briefing compiled from directives + receipts

---

## CLI Surface

```bash
# ============================================
# punk-run (NEW — orchestration CLI)
# ============================================

# === Goals (autonomous cycle — primary interface) ===
punk-run goal <project> "objective"       # create goal, planner generates plan
punk-run goal <project> "obj" --deadline 2026-03-31 --budget 10
punk-run goals                            # list active goals with progress
punk-run goal status <goal-id>            # detailed progress per step
punk-run goal pause/resume/cancel/replan/budget <goal-id>

# === Manual Tasks (one-off, no goal) ===
punk-run queue <project> "prompt"         # create task directly
punk-run queue --template codegen         # from template
punk-run queue --agent claude-opus        # specific agent
punk-run queue --priority p0 --after <task-id>

# === Daemon ===
punk-run daemon                           # start daemon (foreground)
punk-run daemon --background              # start as background service

# === Operations ===
punk-run status                           # terminal table, no AI
punk-run status --project signum          # filtered
punk-run morning                          # daily briefing
punk-run triage                           # interactive dead-letter review
punk-run approve/cancel/retry <task-id>   # task control

# === Pipeline (lightweight CRM) ===
punk-run pipeline                         # list current opportunities
punk-run pipeline add/advance/win/lose/stale

# === Intelligence ===
punk-run ask "what is blocking signum?"   # AI query over state
punk-run graph --type cost --since 14d    # on-demand chart
punk-run receipts --project signum --since 7d

# === Multi-Model ===
punk-run diverge <project> "prompt"       # 3-provider parallel, pick winner
punk-run panel <project> "question"       # ask all providers, compare

# === Skills + Config ===
punk-run skill create/list                # skill management
punk-run config                           # show current config
punk-run policy check --dry-run <task>    # test routing rules
punk-run doctor                           # health check

# ============================================
# punk (EXISTING — verification CLI, FROZEN)
# ============================================

punk init                                 # brownfield scan
punk plan                                 # generate contract
punk check                                # scope gate (called by punk-run post-dispatch)
punk receipt                              # completion proof
punk status                               # verification state
punk close                                # abandon contract
punk config                               # provider config
```

---

## Adapter Model

Minimum contract: **be callable and exit with a code.** That's it.

### Integration Levels

1. **Callable** - specpunk can start the process. Exit code = success/failure.
2. **Receipt-aware** - agent writes `receipt.json` on exit with cost and status.
3. **Fully instrumented** - agent reads session context, writes receipts, respects budget signals.

### Built-in Adapters

| Adapter | Mechanism | Invocation |
|---------|-----------|------------|
| `claude` | Claude Code CLI | `claude -p --model <model> --max-turns N` |
| `codex` | Codex CLI | `codex exec --model <model> --full-auto` |
| `gemini` | Gemini CLI | `gemini -p --model <model>` |
| `process` | Any executable | `<command> --task <task.json>` |

Each adapter handles:
- Process lifecycle (start, monitor, kill)
- Output capture (stdout -> receipt)
- Auth (OAuth tokens, subscription billing)
- Personality injection (system prompt, skills, CWD config)
- Provider quirks (Claude needs `unset CLAUDECODE`, Codex needs sandbox flag)

### Auth Model — Subscription-First, API as Exception

**Core principle:** All agent work runs through CLI tools on subscription billing. API calls (per-token billing) are the absolute last resort.

| Provider | Auth Method | Token Setup | Token Location | Headless Parallel |
|----------|-----------|-------------|----------------|-------------------|
| Claude | OAuth (Max subscription) | `claude setup-token` | `CLAUDE_CODE_OAUTH_TOKEN` env var | 1yr token, supports 5+ parallel |
| Codex | OAuth (ChatGPT Plus/Pro) | `codex auth` | `~/.codex/auth.json` | Auto |
| Gemini | OAuth (Google account) | `gemini login` | `~/.gemini/` | Auto |

**Why subscription-first:**
- $20-200/mo flat rate vs $3-15/hr on API at scale
- 50 tasks/day x 30 days = 1500 agent runs/month. On API this would cost hundreds. On subscription it's fixed.
- All three CLIs support headless mode (`claude -p`, `codex exec`, `gemini -p`) with full capabilities.
- Subscription includes latest models (Opus, GPT-5, Gemini 3) without per-token billing.

**OAuth tokens also work for direct API calls on subscription billing.** This eliminates the CLI spawn overhead (~2-3s) for quick queries while staying on subscription:

| Provider | OAuth -> Direct API | Endpoint | Latency |
|----------|-------------------|----------|---------|
| Claude | `sk-ant-oat*` + Bearer auth + beta headers | `api.anthropic.com` Messages API | ~200ms |
| Codex | JWT from `codex auth` | `chatgpt.com/backend-api/codex` Responses API | ~300ms |
| Gemini | TBD (needs investigation: gcloud OAuth -> Vertex AI?) | TBD | TBD |

**Claude OAuth -> API requirements** (from Hermes production code):
```
auth_token: <oauth-token>
anthropic-beta: claude-code-20250219,oauth-2025-04-20
user-agent: claude-cli/<current-version> (external, cli)
x-app: cli
```
Without these headers, Anthropic intermittently 500s the requests. Version must be current.

**Three invocation tiers (choose per use case):**

| Tier | Method | Latency | When to use |
|------|--------|---------|-------------|
| **CLI** (default) | `claude -p` / `codex exec` / `gemini -p` | 2-3s | All agent tasks (full tool access, skills, context) |
| **OAuth API** | Direct SDK call with subscription token | 200ms | `punk ask`, quick classification, status checks |
| **Paid API** | API key billing | 200ms | Only if subscription unavailable or model not on sub |

**Default: NO paid API keys.** CLI + OAuth API cover 99% of use cases on subscription.

```toml
# agents.toml
[agents.claude-coder]
provider = "claude"
invoke = "cli"          # default: full CLI with tools and skills

[agents.claude-quick]
provider = "claude"
invoke = "oauth-api"    # fast path: direct API via subscription OAuth
model = "haiku"         # cheap model for classification/routing
# Use case: punk ask, receipt summarization, session compaction
```

**Critical constraints:**
- `claude -p` detects `CLAUDECODE` env var and refuses to run inside another Claude Code session. Adapter must `unset CLAUDECODE` before spawning.
- `ANTHROPIC_API_KEY` in env shadows subscription auth. Adapter must `env -u ANTHROPIC_API_KEY`.
- OAuth token refresh in headless is fragile (Hermes #2962, Paperclip #1861). Use long-lived tokens (`claude setup-token` = 1yr).
- macOS `taskgated` kills 3rd+ concurrent `claude` processes with SIGKILL (exit 137). Workaround: `CLAUDE_DEBUG=1`.

### Agent Personality — How to Give Agents Identity

The personality system works differently per provider because headless CLIs have different injection capabilities.

**agents.toml example:**

```toml
[agents.claude-reviewer]
provider = "claude"
model = "sonnet"
role = "reviewer"
system_prompt = "agents/reviewer.md"
skills = ["verification", "code-review"]

[agents.codex-engineer]
provider = "codex"
role = "engineer"
system_prompt = "agents/engineer.md"

[agents.gemini-scout]
provider = "gemini"
model = "gemini-3-flash-preview"
role = "scout"
system_prompt = "agents/scout.md"
```

**Injection per provider:**

| Provider | System Prompt | Skills | CWD Config |
|----------|-------------|--------|-----------|
| Claude | `--append-system-prompt-file agents/reviewer.md` | `--add-dir <tmpdir>` with symlinked skills | CLAUDE.md in worktree |
| Codex | Prepended to prompt: `"You are a {role}.\n{system_prompt}\n---\nTask: {prompt}"` | Not supported | AGENTS.md in worktree |
| Gemini | Prepended to prompt (same pattern as Codex) | Not supported | GEMINI.md copied to worktree |

**Paperclip's approach (reference):** Creates per-run tmpdir at `$TMPDIR/paperclip-skills-XXXXX/.claude/skills/`, symlinks each skill, passes via `--add-dir`. Prompt sent via stdin. 20+ env vars for context (`PAPERCLIP_AGENT_ID`, `_TASK_ID`, `_WORKSPACE_CWD`, etc.). Deleted in `finally` block.

**Our approach:** Same pattern for Claude. For Codex/Gemini, the `system_prompt` file contents are prepended to the task prompt. If the task runs in a worktree, we can also write provider-specific config files (CLAUDE.md, GEMINI.md, AGENTS.md) into the worktree before agent launch.

**Personality files are plain markdown:**
```markdown
# Reviewer Agent

You are a code reviewer. Focus on:
- Security vulnerabilities (OWASP top 10)
- Logic errors and edge cases
- Performance issues
- API contract violations

Do NOT comment on style, formatting, or naming conventions.
Report findings as: [file:line] severity: description
```

---

## Budget & Cost Control

### Three Tiers

1. **Per-task ceiling** - `budget_usd` in task spec. Agent killed if exceeded.
2. **Per-project ceiling** - `budget_usd` in projects.toml. Tasks auto-deprioritized when near limit.
3. **Global monthly ceiling** - in policy.toml. Hard stop at 95%.

### Backpressure

As monthly spend approaches ceiling:
- At 80%: `warn` directive emitted
- At 90%: max concurrent slots reduced from 5 to 2, only p0 and p1 accepted
- At 95%: hard stop, only p0 accepted, human must raise ceiling

Cost data comes from receipts. No external billing API needed.

---

## Human-in-the-Loop

### Risk Tiers

| Tier | Gate | Examples |
|------|------|---------|
| T1 | Auto-approve, auto-run | Research, read-only audit, content draft |
| T2 | Auto-run, review result | Codegen, bug fix (punk check runs after) |
| T3 | Approve before run | Destructive ops, infra changes, releases |

Risk tier is set per-task or inferred from category + policy rules.

### Governance

The human is always in control:
- `punk cancel <id>` - stop any running task
- `punk approve/reject <id>` - gate T3 tasks
- `punk status` - see everything at a glance
- Policy changes are git-committed TOML, not hidden state

No "board" abstraction needed for solo founder. The human IS the board.

---

## Verification Integration

punk's existing verification (init/plan/check/receipt) becomes a post-dispatch gate:

1. Agent completes task, writes code changes
2. If project has `.punk/` directory, daemon runs `punk check --json`
3. Check result recorded in receipt (`punk_check_exit`)
4. If check fails (scope violation), task goes to `failed/` with reason
5. Human reviews via `punk triage`

Verification is optional per-project. Not all tasks need scope checking.

---

## Memory Architecture

Five layers, no database. Flat files + external MCP.

```
L0  Working Memory    task.json + prompt              ephemeral, dies with task
L1  Session Memory    sessions/<project>/context.json  frozen snapshot at task start
L2  Receipts          receipts/index.jsonl             append-only episodic log
L3  Engram            MCP server (external)            long-term semantic facts
L4  Skills            skills/*.md                      procedural memory
```

### L1: Session Context (Hermes frozen snapshot pattern)

- Read ONCE at task start, injected into agent context. Never mutated mid-task.
- Written back AFTER task completes, from receipt data.
- Entries are typed: `success | failure | surprise | cost_overrun`.
- Each entry has `ttl_tasks` (counts down per new task; expired entries auto-evict).
- Negative signal required: receipt validator rejects receipts without at least one session entry.
- Capped at 10 entries. Oldest evicted first after TTL.
- Atomic writes (temp + mv). Scanned for prompt injection before loading.

### L2: Receipt Index (append-only)

- `receipts/index.jsonl` - one line per completed task.
- Schema-validated on write (JSON Schema v1).
- TTL: receipts older than 90 days archived to `receipts/archive/`.
- Queryable by `punk status`, `punk ask`, `punk graph`.

### L3: Engram (external, already exists)

- `mem_save` / `mem_search` via MCP protocol.
- Durable facts, decisions, procedures.
- Not duplicated in specpunk. Engram is the semantic SSoT.

### L4: Skills (self-authored procedural memory)

- Markdown files in `skills/` directory.
- Agents can create/patch skills after complex tasks (Hermes pattern).
- Security scan before injection (10 regex patterns for prompt injection + invisible Unicode).
- Progressive disclosure: index (name + 60-char description) -> full SKILL.md on demand.

### What We Don't Build

- No vector database (Letta proved filesystem + smart agent > vector DB)
- No knowledge graph (receipts + Engram cover our needs)
- No SQLite (flat files sufficient at 50 tasks/day scale)

---

## Self-Improvement

Three loops, ordered by implementation difficulty.

### Loop 1: Receipt-Driven Learning (Phase 0)

```
task completes -> receipt written -> session context updated
     |
     v
next task reads session -> agent sees what worked/failed
```

The receipt's typed session entries (success/failure/surprise) create a natural feedback signal. Negative signal is forced: no receipt is valid without at least one session entry. This means agents MUST report what happened, even if nothing interesting occurred.

### Loop 2: Skill Self-Authoring (Phase 3)

Agents create skills after complex tasks. Trigger heuristic (from Hermes):
- Task used 5+ tool calls
- Errors were overcome during execution
- User corrected the approach mid-task
- Non-trivial workflow was discovered

The agent calls `punk skill create <name>` which:
1. Validates YAML frontmatter (name, description required)
2. Runs security scan (regex for injection + Unicode)
3. Writes atomically (temp + mv)
4. Next task: skill available in agent's context

Agents can also PATCH existing skills when they encounter undocumented failure modes.

### Loop 3: Metric Ratchet (Phase 4, Autoresearch pattern adapted)

```
weekly cycle:
  read receipts from last 7 days
  compute: punk check pass rate, avg cost, failure rate, model routing accuracy
  compare to previous week
  if degraded -> emit warn directive
  if improved -> log what changed
```

This is the Karpathy autoresearch pattern applied to operational metrics instead of model weights. The "artifact" being improved is the system configuration (policy.toml, agents.toml, skill files), and the "metric" is receipt-derived performance data.

### What We Don't Build

- No RL training pipeline (we use frontier models, can't fine-tune)
- No on-policy distillation (NousResearch-specific, requires own model)
- No TDD for prompts (Superpowers pattern - needs test harness, future work)

---

## Migration Path

### Phase 0: Foundation (current)
- Freeze receipt schema v1
- Add schema validation to existing bash supervisor
- Add `receipts/index.jsonl` + `punk status`

### Phase 1: Policy + Config
- Write `projects.toml`, `agents.toml`, `policy.toml`
- `punk policy check --dry-run`
- Migrate hardcoded bash routing to declarative rules

### Phase 2: Typed Daemon
- Rust daemon in shadow mode (parallel with bash)
- Same filesystem bus protocol
- Validate receipts match between Rust and bash
- Cut over after 1-2 weeks

### Phase 3: Goal System + Intelligence
- `punk goal` with planner agent (autonomous cycle)
- Goal evaluation loop in daemon
- `punk ask` with haiku (OAuth API fast path)
- `punk morning` (goals + tasks + pipeline + directives)
- `punk graph` for on-demand reports
- Session context for agent continuity
- `punk pipeline` (flat-file CRM, replaces adjutant)

### Phase 4: Advanced
- Budget backpressure (auto-throttle)
- `punk diverge` / `punk panel` (multi-model)
- Goal re-planning on drift detection
- Skill self-authoring (agents create skills after complex tasks)
- Metric ratchet (weekly performance comparison)

---

## What We Take From Each Reference

| Source | What We Adopt | What We Skip |
|--------|--------------|-------------|
| **Paperclip** | Adapter model (invoke/status/cancel), receipt-as-contract, budget tiers, atomic task checkout, skills tmpdir + `--add-dir` pattern, env var contract for context, session resume by cwd match | PostgreSQL, React UI, Company/Org hierarchy, REST API, WebSocket events |
| **gstack** | Skills as markdown, flat-file state, no-database philosophy, on-demand rendering, generated-from-source docs, eval tier system | Browser daemon, Bun stack, sprint process |
| **Hermes** | Frozen snapshot memory (prompt stability + cache), context compression template (7 sections), skill self-authoring + patching, security scan before injection, parallel tool safety classification, delegation credential isolation | SQLite, messaging gateway, TUI, 50K LOC Python, RL training |
| **Superpowers** | 4-status protocol (DONE/CONCERNS/BLOCKED/NEEDS_CONTEXT), context isolation (subagent gets exactly what it needs), persuasion engineering (Authority+Commitment), 3-fix escalation rule, verification-before-completion gate | Pure-markdown-only approach, no orchestration, no memory |
| **Autoresearch** | Ratchet loop (experiment -> measure -> keep/revert), single metric, git as substrate, NEVER STOP philosophy, autonomous cycle | Model training focus (we don't train models) |
| **Contrarian** | Incremental migration (never big-bang), "9 projects" constraint, filesystem IS the platform, shadow mode before cutover | "Don't rewrite at all" (bash hit its ceiling) |
| **adjutant** (our own, archived) | Pipeline JSONL format, morning briefing concept, business goal framing | SQLite CRM (overkill at $0 revenue), separate binary |

## Lessons from GitHub Issues (6700+ issues across 4 repos)

| Lesson | Source | How We Address It |
|--------|--------|-------------------|
| Transient errors (429/529) must retry, not mark "completed" | Paperclip #1763 | Receipt status distinguishes transient vs permanent failure |
| Session/memory loss on compaction/restart | All 4 repos | Frozen snapshot + receipt-based session, not in-context |
| Unbounded storage growth without TTL/GC | Paperclip #1846, Hermes #3015 | 90-day archive for receipts, TTL on session entries |
| Shared iteration budget parent<->subagent | Hermes #2873 | Budget isolation: each task has independent budget |
| OAuth refresh broken in headless | Hermes #2962 | Long-lived tokens (1yr OAuth via `claude setup-token`) |
| Idle runs blocking concurrency slots | Paperclip #1749 | Hard timeout per task, watchdog kills stale slots |
| Secrets leaked in API responses | Paperclip #1818 | No API. Adapter env vars never written to receipts |
| Subagent abandonment on parallel dispatch | gstack #497 | Slot-based tracking with heartbeat + SIGKILL escalation |
