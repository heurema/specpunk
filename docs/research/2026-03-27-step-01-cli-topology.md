# Step 1: CLI Topology — One Binary vs Unix-Style Many

## Raw Data

### How each project is structured

| Project | Entry Points | Language | LOC (approx) | Install |
|---------|-------------|----------|---------------|---------|
| **Paperclip** | 1 binary (`paperclipai`) | TypeScript/Node | ~30K+ TS | `npm i -g paperclipai` |
| **Hermes** | 3 binaries from 1 package (`hermes`, `hermes-agent`, `hermes-acp`) | Python | ~50K+ Python | `uv pip install hermes-agent` |
| **gstack** | 1 compiled binary (`browse`) + 15 bash scripts + 28 skill MDs | TS/Bash/MD | ~5K TS + ~2K bash | `setup` script, CC marketplace |
| **Superpowers** | 0 binaries, pure plugin | Markdown + bash hook | ~200 lines code | CC marketplace |
| **punk (specpunk)** | 1 binary (`punk`) | Rust | ~14K Rust | `cargo install` |
| **punk-supervisor** | 12 bash scripts | Bash | ~1.7K bash | launchd plist |
| **adjutant** | 1 binary (`adjutant`) | Rust | ~2K Rust | `cargo install` |

### Command surface area

**Paperclip (1 binary, ~20 commands):**
```
paperclipai onboard|run|doctor|env|configure|db:backup
paperclipai heartbeat run
paperclipai auth bootstrap-ceo
paperclipai context|company|issue|agent|approval|activity|dashboard|worktree|plugin
```
Everything under one roof. Server + CLI + client commands in one binary.

**Hermes (3 binaries, ~15 commands + ~20 slash commands):**
```
hermes [chat]|setup|model|tools|config|cron|honcho|doctor|sessions|acp|update
hermes-agent (standalone runner, fire CLI)
hermes-acp (ACP protocol server)
```
Main interactive CLI + headless runner + editor protocol adapter. Separated by use case.

**gstack (distributed):**
```
browse <50+ subcommands>    # compiled binary
gstack-config|gstack-update-check|gstack-telemetry-*|gstack-repo-mode|...  # bash scripts
/review|/ship|/qa|/browse|/codex|/autoplan|...  # skill invocations
```
No central CLI. Each component is independent. Skills are invoked by the host AI platform.

**Our current system (fragmented):**
```
punk init|plan|check|receipt|status|close|config  # Rust binary (verification)
punk-supervisor.sh    # daemon (poll + dispatch)
punk-dispatch.sh      # per-task runner
punk-task.sh          # task creator (template-based)
punk-create-task.sh   # low-level task writer
punk-diverge.sh       # 3-model parallel
punk-watchdog.sh      # health monitor
punk-triage.sh        # interactive review
punk-approve|cancel|retry|quality-scan.sh
adjutant init|morning|pipeline|act|review|decide  # Rust binary (business ops)
cycle-controller.sh   # daily signal
```

---

## Analysis: 4 Topology Models

### Model A: Monolith (Paperclip pattern)
One binary does everything: server, scheduler, CLI client, config, diagnostics.

**Pros:**
- Single install, single version, single update path
- Shared state (DB connection) without IPC
- Discoverable: `paperclipai --help` shows everything
- Paperclip proves it works at scale (7 adapters, plugins, React UI)

**Cons:**
- Scope creep — binary grows unbounded (Hermes's cli.py is 327KB, run_agent.py is 382KB)
- Compile time grows with scope (Rust: significant)
- Can't run parts independently (e.g., watchdog separate from scheduler)
- Deployment: can't upgrade scheduler without restarting everything

**Best for:** Products with a web UI, team usage, SaaS deployment.

### Model B: Multi-Entry Single Package (Hermes pattern)
One package, multiple binaries. Shared library, different entry points.

**Pros:**
- Code sharing without IPC
- Each binary optimized for its use case (interactive vs headless vs protocol)
- Install once, get all tools
- Natural separation of concerns

**Cons:**
- Still a monolith at the package level — all deps bundled
- Hermes's 3 entry points share massive modules (run_agent.py used by all)
- Version coupling: can't update hermes-agent without updating hermes

**Best for:** Tools with distinct runtime modes (interactive, daemon, protocol adapter).

### Model C: Toolkit (gstack pattern)
Skills as markdown + compiled binary for infrastructure + bash for glue.

**Pros:**
- Each component independently replaceable
- Skills are pure text — no compilation, instant iteration
- Browse daemon lifecycle independent of skill execution
- Minimal binary (gstack compiles only the browser daemon)

**Cons:**
- No unified `--help` — users must know what exists
- Cross-component state sharing via files (fragile)
- bash glue is hard to test and maintain long-term
- gstack works because Claude Code is the orchestrator — without it, there's no CLI

**Best for:** Claude Code plugins, tools that extend an existing AI platform.

### Model D: Unix Pipeline (current punk pattern)
Many independent scripts + one compiled binary. Compose via filesystem.

**Pros:**
- Each tool does one thing well (punk-supervisor polls, punk-dispatch runs, punk checks)
- Can replace any piece without touching others
- Daemon (supervisor) and CLI (punk) have independent lifecycles
- Low coupling: if punk-dispatch.sh breaks, supervisor still runs other tasks

**Cons:**
- 12 scripts is a lot to discover and maintain
- Shared state via filesystem requires careful protocol (race conditions, atomicity)
- No type safety across script boundaries (JSON parsing in bash)
- Testing bash is painful
- Users can't easily see what the system can do

**Best for:** Infrastructure daemons, composition of heterogeneous tools.

---

## Tradeoff Matrix

| Criterion | Monolith | Multi-Entry | Toolkit | Unix Pipeline |
|-----------|----------|-------------|---------|---------------|
| Discoverability | +++  | ++  | +   | +   |
| Independence of components | +   | ++  | +++ | +++ |
| Code sharing | +++ | +++ | +   | -   |
| Testability | ++  | ++  | +   | +   |
| Iteration speed (skills) | +   | +   | +++ | ++  |
| Iteration speed (infra) | +   | ++  | ++  | +++ |
| Solo dev maintenance | ++  | ++  | +++ | +   |
| Can run as daemon | ++  | +++ | ++  | +++ |
| Type safety | +++ | +++ | +   | -   |
| Compile time (Rust) | -   | --  | +++ | ++  |

---

## Key Observations

1. **Paperclip is a product, we're building infrastructure.** Paperclip serves teams with a web UI. We serve a solo founder with a terminal. Different topology needs.

2. **gstack proves you don't need a monolith.** Garry Tan's system is wildly effective with just a browser binary + skills. The AI platform (Claude Code) IS the orchestrator.

3. **Hermes's 3-binary model is elegant.** `hermes` (interactive), `hermes-agent` (headless), `hermes-acp` (protocol) — same code, different entry points. This maps naturally to our needs.

4. **Our bash scripts are the real orchestrator.** punk-supervisor (1699 LOC bash) does scheduling, dispatch, monitoring, retry — this is the critical infrastructure that needs type safety and reliability.

5. **adjutant is a separate concern.** Business ops (pipeline, outreach) has zero overlap with code verification (punk check) or task dispatch (supervisor). Separate binary is correct.

---

## Preliminary Decision

### Recommended: Model B+ (Multi-Entry + Toolkit Hybrid)

**Rust workspace with 3 binaries + skills as markdown:**

```
specpunk/
  punk-core/        # shared library (contracts, state, adapters, dispatch)
  punk-cli/         # `punk` — verification CLI (init, plan, check, receipt)
  punk-run/         # `punk-run` — orchestrator daemon (replaces bash supervisor)
  punk-ops/         # `punk-ops` — operational commands (triage, approve, cancel, diverge, watchdog)
  skills/           # markdown skills (stay as-is, invoked by CC)
  adjutant/         # stays separate — different domain
```

**Why this split:**
- `punk` (verification) = deterministic, fast, no daemon — stays clean
- `punk-run` (daemon) = the scheduler/dispatcher, needs reliability, type safety, atomicity — replaces 1699 LOC bash
- `punk-ops` (operational) = human-facing commands for triage/approve/cancel — interactive, optional
- Skills = markdown, zero compile time, instant iteration
- adjutant = separate domain (business ops), separate binary, separate release cycle

**What this gives us:**
- Type safety for the critical path (dispatch, slot management, heartbeat)
- Independent daemon lifecycle (punk-run can restart without affecting punk or punk-ops)
- Shared punk-core library (adapters, state types, bus protocol)
- Skills still iterate at the speed of text
- Each binary has focused scope — no 327KB God files

**What we lose vs monolith:**
- Need IPC or shared filesystem for punk-ops to talk to punk-run
- Three binaries to install/update (but same workspace, one `cargo install`)

**Open questions for Step 2:**
- Does punk-core export adapter types that punk-run uses? (yes, likely)
- Should punk-ops be a separate binary or subcommands of punk-run?
- Where does adjutant's `decide` (arbiter) live — adjutant or punk-ops?
