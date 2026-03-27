# Specpunk Architecture Spec

Executable specification. Complements VISION.md (product direction) with implementable contracts.

---

## 0. Flexibility Principles

Everything changes fast: new models weekly, CLIs break monthly, providers appear and die.
Architecture must make change cheap, not prevent it.

### Core Rule: Hexagonal Traits + Enum Dispatch

Evaluated actors (kameo/ractor), ECS (bevy/hecs), typestate — all overkill or wrong fit.
Chosen: hexagonal traits as ports + enum dispatch for adapters + tokio channels for IPC.

**Ports (traits)** define what the system needs. Adapters implement it:

```rust
// punk-orch/src/ports.rs — the contracts
pub trait Executor: Send + Sync {
    async fn run(&self, task: &Task, config: &AdapterConfig) -> Result<RunResult>;
    fn name(&self) -> &str;
}

pub trait Store: Clone + Send + Sync + 'static {
    async fn enqueue(&self, task: Task) -> Result<()>;
    async fn claim_next(&self, slot_id: SlotId) -> Option<Task>;
    async fn mark_done(&self, id: TaskId, receipt: Receipt) -> Result<()>;
}

// Adding a new provider = one file implementing this trait.
// No changes to daemon, queue, or any other module.
```

**Enum dispatch** for known adapters (compile-time exhaustive match = AI safety net):

```rust
// punk-orch/src/executor.rs
enum ExecutorKind {
    Claude(ClaudeExecutor),
    Codex(CodexExecutor),
    Gemini(GeminiExecutor),
}
// Adding new variant = compiler errors everywhere it's not handled.
// AI agents CAN'T forget to handle a case.
```

**Tokio channels** for internal communication (not actors — overkill for 5 slots):

```rust
// mpsc: task queue -> dispatcher (many writers, one reader)
// watch: current system state (dispatcher -> watchdog, reporter)
// broadcast: events (slot claimed, task done, error) -> all listeners
// oneshot: request-response for punk-run ask/status
```

Same pattern for every pluggable boundary:

| Trait (Port) | What it abstracts | Why it changes |
|-------------|-------------------|----------------|
| `Executor` | Provider invocation (Claude/Codex/Gemini/custom) | New CLIs appear, flags change, auth changes |
| `Store` | Receipt + run persistence (flat file now, SQLite later?) | Scale ceiling, query performance |
| `Bus` | Queue protocol (filesystem now) | Might need IPC, remote dispatch |
| `Planner` | Goal -> plan generation | Prompt engineering, model switching |
| `SessionStore` | Agent memory read/write | Memory architecture evolves |
| `PolicyEngine` | Routing + budget rules | Rules get more complex |
| `Reporter` | Output rendering (terminal table, Mermaid, JSON) | New output formats |

### Enum FSM for Task Lifecycle

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
enum TaskState {
    Queued { enqueued_at: DateTime<Utc> },
    Claimed { slot_id: SlotId, claimed_at: DateTime<Utc> },
    Running { pid: u32, started_at: DateTime<Utc> },
    Done { receipt: Receipt },
    Failed { error: String, attempts: u8 },
}

impl TaskState {
    fn transition(self, event: TaskEvent) -> Result<Self, InvalidTransition> {
        match (self, event) {
            (Self::Queued { .. }, TaskEvent::Claimed(slot)) => Ok(Self::Claimed { .. }),
            (Self::Claimed { .. }, TaskEvent::Started(pid)) => Ok(Self::Running { .. }),
            (state, event) => Err(InvalidTransition { state, event }),
        }
    }
}
```

Serializable (JSON on disk), exhaustive match, AI-friendly. Not typestate (can't serialize).

### Crate Stack

```toml
tokio = { version = "1.44", features = ["full"] }
command-group = "2.1"      # process group kill (no orphan children)
arc-swap = "1.7"           # lock-free config hot reload
notify = "6.1"             # file watching for config changes
signal-hook = "0.3"        # SIGHUP handler
thiserror = "2"            # structured component errors
anyhow = "2"               # operational daemon errors
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
```

### What We Explicitly Don't Use

| Pattern | Why not |
|---------|---------|
| Actors (kameo/ractor/actix) | 5 slots don't need supervision trees or mailboxes |
| ECS (bevy/hecs) | 5-50 entities, not 5000. Unfamiliar to AI agents |
| Typestate | Can't serialize, AI agents struggle with it |
| Box<dyn Trait> everywhere | Enum dispatch is faster and exhaustive. dyn only at main() boundary if needed |
| libloading / dynamic plugins | Adapters compiled in. Config is TOML, not DLLs |
| Distributed actors | Single machine. No cluster |

### Config-Driven, Not Code-Driven

Behavior lives in TOML files, not Rust match statements.

```
Adding a new agent:     edit agents.toml        (no recompile)
Changing routing rules: edit policy.toml        (no recompile)
Adding a project:       edit projects.toml      (no recompile)
New skill:              drop markdown file      (no recompile)
New personality:        drop markdown file      (no recompile)
```

**Only these require code changes:**
- New adapter (provider CLI) = implement Adapter trait + register
- New receipt field = update schema + struct
- New CLI command = add clap subcommand

### Hot Reload Where Possible

```
policy.toml    -> reloaded on SIGHUP (drain active tasks first)
agents.toml    -> reloaded on SIGHUP
projects.toml  -> reloaded on SIGHUP
skills/*.md    -> discovered per-task (always fresh)
adapters.toml  -> read-only at startup (restart to change)
```

### Schema Versioning

Every persisted format has `schema_version: N`. Readers check version. Migration = read old + write new.

```json
// receipt
{"schema_version": 1, "task_id": "...", ...}

// session context
{"schema_version": 1, "project": "signum", "entries": [...]}

// pipeline event
{"schema_version": 1, "id": 1, "event": "created", ...}
```

New fields = increment version. Old readers skip unknown fields (forward compatible). Missing required fields = error.

### Feature Flags via Policy

Not compile-time feature flags. Runtime behavior switches in policy.toml:

```toml
[features]
goal_system = true          # Phase 3, can disable if unstable
budget_backpressure = false  # Phase 4, not ready yet
skill_self_authoring = false # Phase 4, not ready yet
oauth_api_fast_path = true   # Tier 2 auth, can disable if broken

[features.experimental]
consensus_voting = false     # MassGen pattern, not implemented yet
code_agent = false           # smolagents pattern, future
```

Daemon checks features at dispatch time. No code recompile to enable/disable.

### Easy to Remove

Every module can be deleted without breaking others:

```
Delete pipeline.rs?  -> punk-run pipeline command disappears.
                        Nothing else breaks. No orphan references.

Delete ask.rs?       -> punk-run ask disappears.
                        morning still works (it's a different module).

Delete goal.rs?      -> punk-run goal disappears.
                        Manual punk-run queue still works.
                        Daemon still dispatches tasks.
```

This is enforced by the trait boundaries: modules communicate through traits and state files, not direct function calls across modules.

### Adapter as the Flexibility Boundary

The adapter is the narrowest waist of the system. Everything above it (goals, queue, policy) is ours to change. Everything below it (Claude CLI, Codex CLI, Gemini CLI) is vendor-controlled and unpredictable.

```
Our code (stable, we control):
  goal -> plan -> tasks -> queue -> dispatch
                                      |
                              Adapter trait  <-- THE BOUNDARY
                                      |
Vendor code (unstable, they control):
  claude -p / codex exec / gemini -p
```

When a vendor changes their CLI:
1. Update ONE adapter file
2. Update adapters.toml capability matrix
3. Everything above the adapter is unaffected

When we want to add a completely new dispatch model (e.g., API instead of CLI):
1. Implement new Adapter
2. Register in adapters.toml
3. Policy routes to it based on `invoke = "oauth-api"`

---

## 1. Run/Attempt Entity

A Task can have multiple Runs. A Run is a single attempt to execute a task.

### Schema

```
Task (1) ---> (N) Run ---> (1) Receipt
```

**Task** stays as-is (queued work item). Terminal states: `done`, `failed`, `cancelled`.

**Run** is NEW — tracks each execution attempt:

| Field | Type | Notes |
|-------|------|-------|
| run_id | string | `<task_id>-<attempt>` e.g. `signum-20260327-140000-1` |
| task_id | string | Parent task |
| attempt | int | 1-indexed attempt number |
| retry_of | string? | Previous run_id if this is a retry |
| slot_id | int | Which concurrency slot (0-4) |
| agent | string | Agent ID from agents.toml |
| model | string | Actual model used |
| invoke_tier | enum | `cli`, `oauth-api`, `paid-api` |
| status | enum | `claimed`, `running`, `success`, `failure`, `timeout`, `killed`, `cancelled` |
| error_type | enum? | `transient`, `permanent`, `unknown` (only on failure) |
| termination_reason | string? | `budget_stop`, `timeout`, `adapter_crash`, `provider_429`, `provider_529`, `sigkill`, `user_cancel` |
| claimed_at | datetime | When slot acquired |
| started_at | datetime | When agent process spawned |
| finished_at | datetime | When agent process exited |
| duration_ms | int | Wall-clock time |
| exit_code | int | Process exit code |
| pid | int? | OS process ID (for orphan detection) |
| worktree_path | string? | If isolated worktree |
| prompt_hash | string | SHA-256 of full prompt sent to agent |
| context_hash | string | SHA-256 of session context injected |
| stdout_path | string | Path to captured stdout |
| stderr_path | string | Path to captured stderr |
| heartbeat_at | datetime | Last heartbeat timestamp |

**Receipt** stays as-is but links to run_id instead of task_id:

| Field change | Old | New |
|-------------|-----|-----|
| task_id | required | required |
| run_id | (missing) | **required** |
| attempt | (missing) | **required** |

### Lifecycle

```
Task queued (new/p1/task.json)
  |
  v
Run 1 created (status: claimed)
  |
  v
Agent spawned (status: running, heartbeat starts)
  |
  +---> success --> Receipt written, task -> done/
  |
  +---> failure (transient: 429/529/timeout)
  |       |
  |       v
  |     retry policy check
  |       |
  |       +---> retry allowed --> Run 2 created (retry_of: run-1)
  |       |
  |       +---> retry exhausted --> task -> failed/
  |
  +---> failure (permanent: bad prompt, auth expired)
          |
          v
        task -> failed/ (no retry)
```

### Storage

Runs stored in `state/bus/runs/<task_id>/`:
```
state/bus/runs/signum-20260327-140000/
  run-1.json          # attempt 1
  run-1.stdout        # captured output
  run-1.stderr        # captured errors
  run-2.json          # attempt 2 (retry)
  run-2.stdout
  run-2.stderr
```

Event log per task: `state/bus/runs/<task_id>/events.jsonl`
```json
{"event": "claimed", "run_id": "...-1", "slot": 2, "ts": "..."}
{"event": "started", "run_id": "...-1", "pid": 12345, "ts": "..."}
{"event": "heartbeat", "run_id": "...-1", "ts": "..."}
{"event": "failed", "run_id": "...-1", "error_type": "transient", "reason": "provider_429", "ts": "..."}
{"event": "retry_scheduled", "run_id": "...-2", "retry_of": "...-1", "ts": "..."}
{"event": "claimed", "run_id": "...-2", "slot": 3, "ts": "..."}
{"event": "started", "run_id": "...-2", "pid": 12400, "ts": "..."}
{"event": "success", "run_id": "...-2", "ts": "..."}
{"event": "receipt_written", "run_id": "...-2", "ts": "..."}
```

---

## 2. Queue Protocol Spec

### Claim (atomic)

```
1. Daemon scans new/p{0,1,2}/ for task JSON files (5s poll)
2. For each task (priority order):
   a. Check depends_on: all deps have receipt in done/? If not, skip.
   b. Check project lock: state/bus/.locks/<project> exists? If yes, skip (unless worktree=true).
   c. Check slot availability: count(state/bus/.slots/slot-*) < max_slots?
   d. Acquire slot: mkdir state/bus/.slots/slot-<N>/ (atomic on POSIX)
      - Write slot-<N>/info.json: {task_id, pid, claimed_at}
      - If mkdir fails (slot taken): try next slot
   e. Atomic move: mv new/p1/task.json -> cur/task.json
      - mv is atomic on same filesystem
      - If mv fails (task already claimed): release slot, skip
   f. Create run: write state/bus/runs/<task_id>/run-<attempt>.json
   g. Log event: claimed
```

### Lease + Heartbeat

```
Heartbeat contract:
- Dispatch process writes heartbeat file every 30s:
  state/bus/.heartbeats/<task_id> = {"pid": N, "ts": "ISO", "run_id": "..."}

- Daemon checks heartbeats every 60s:
  for each task in cur/:
    hb = read .heartbeats/<task_id>
    if hb.ts older than 90s:
      if kill -0 hb.pid succeeds:
        # process alive but not heartbeating - warn, give 30s more
        log event: heartbeat_stale
      else:
        # process dead - orphan detected
        log event: orphan_detected
        trigger requeue or fail
```

### Stale Detection

| Condition | Action |
|-----------|--------|
| Heartbeat > 90s, PID alive | Warn. Wait 30s. If still stale: SIGTERM. |
| Heartbeat > 90s, PID dead | Orphan. Write partial receipt (status: killed). Requeue if retryable. |
| Wall-clock > task.timeout_s | SIGTERM. Wait 5s. SIGKILL. Write receipt (status: timeout). |
| No heartbeat file after 30s of claim | Adapter failed to start. Write receipt (status: failure, error_type: permanent, reason: adapter_start_failed). |

### Requeue after Daemon Crash

```
On daemon startup:
  1. Scan cur/ for tasks
  2. For each task in cur/:
     a. Read .heartbeats/<task_id>
     b. If PID alive: adopt (resume heartbeat monitoring)
     c. If PID dead: mark run as failed (reason: daemon_crash_recovery)
     d. Check retry policy: if retryable, create new run, mv back to new/
     e. If not retryable: mv to failed/
  3. Clean orphan slots: for each .slots/slot-*/:
     if info.json task_id not in cur/: rmdir slot
  4. Log event: daemon_recovery with count of adopted/requeued/failed
```

### Semantics: At-Least-Once

Specpunk provides at-least-once execution. Duplicate execution is possible if:
- Daemon crashes after claim but before agent starts
- Network partition between daemon and agent

Mitigation: worktree isolation (each run in fresh worktree = idempotent). For non-worktree tasks, the agent is responsible for idempotency via the task prompt.

---

## 3. Auth: Official vs Unofficial

### Auth Tier Classification

| Tier | Description | Stability | When |
|------|-------------|-----------|------|
| **Tier 1: CLI** | Official CLI tools with subscription auth | Stable (vendor-supported) | Default for all tasks |
| **Tier 2: OAuth API** | Subscription OAuth tokens used for direct API calls | **Unofficial, best-effort** | Only for `punk ask` fast path |
| **Tier 3: Paid API** | Standard API keys with per-token billing | Stable (vendor-supported) | Explicit opt-in per agent |

### Tier 2 Caveats (MUST be in docs)

```
WARNING: OAuth API access is an undocumented capability.
- Anthropic: works via Hermes production (sk-ant-oat* + claude-code headers)
- OpenAI/Codex: chatgpt.com/backend-api/codex is an internal endpoint
- Google/Gemini: not yet investigated

Risks:
- Provider can revoke access at any time without notice
- Endpoint URLs and required headers may change with CLI version updates
- ToS may not explicitly permit automated API use via subscription tokens
- No SLA, no rate limit guarantees, no support channel

Fallback order on Tier 2 failure:
1. Retry with Tier 1 (CLI) — always available
2. If urgent (p0): fall back to Tier 3 (paid API) if configured
3. If neither works: fail with actionable error message
```

### Token Health Check

`punk doctor` checks all configured auth:

```
punk doctor

Auth Health:
| Provider | Tier 1 (CLI) | Tier 2 (OAuth API) | Tier 3 (Paid API) |
|----------|-------------|-------------------|------------------|
| Claude   | ok (1yr)    | ok (tested)       | not configured   |
| Codex    | ok          | untested          | not configured   |
| Gemini   | ok          | not available      | not configured   |

Warnings:
- Claude OAuth token expires 2027-03-15 (353 days remaining)
- Codex Tier 2 not tested — run `punk doctor --test-oauth` to verify
- Gemini Tier 2 not available — no known OAuth -> API path
```

### Fallback Order (per invocation tier)

```toml
# policy.toml
[auth.fallback]
# When oauth-api fails, try these in order:
oauth_api_fallback = ["cli", "paid_api"]

# When cli fails (auth expired):
cli_fallback = ["paid_api"]

# Circuit breaker: after 3 consecutive failures on a tier, skip it for 5 minutes
circuit_breaker_threshold = 3
circuit_breaker_cooldown_s = 300
```

---

## 4. Budget Model: Estimated, Not Guaranteed

### Rename: cost_usd -> estimated_cost_usd

Budget enforcement is **best-effort** on subscription tiers.

| Billing Tier | Cost Accuracy | Enforcement |
|-------------|---------------|-------------|
| CLI (subscription) | **Estimated** from token count heuristic | Best-effort: kill after estimated spend exceeds ceiling |
| OAuth API | **Estimated** from response usage field (if returned) | Best-effort |
| Paid API | **Actual** from provider billing response | Hard guarantee |

### How estimation works

```
For CLI runs:
  1. Agent exits. Daemon parses stdout for usage hints:
     - Claude: --output-format stream-json includes token counts
     - Codex: output may include usage summary
     - Gemini: no structured usage output
  2. If usage available: estimate = input_tokens * $/1K + output_tokens * $/1K
  3. If no usage: estimate from duration heuristic ($/min based on historical avg)
  4. Write to receipt: estimated_cost_usd (not cost_usd)
```

### Budget fields renamed

```
Receipt:
  estimated_cost_usd: float    # was cost_usd
  cost_accuracy: enum          # "actual" | "estimated_tokens" | "estimated_duration" | "unknown"
  tokens_input: int?           # if available from provider
  tokens_output: int?          # if available from provider

Policy:
  monthly_ceiling_usd: 50.0    # best-effort enforcement
  enforcement: "best-effort"   # explicit, not implied
```

### Enforcement semantics

- **Pre-run check**: before dispatching, sum `estimated_cost_usd` of receipts this month. If > 80%: warn. If > 95%: block new tasks (except p0).
- **Mid-run kill**: NOT implemented for subscription CLI (no realtime telemetry). Only for paid API path where streaming usage is available.
- **Post-run accounting**: receipt's estimated_cost_usd updates the running total.

---

## 5. Adapter Capability Matrix

Machine-readable file: `adapters.toml` (generated, not hand-written).

```toml
[adapters.claude]
provider = "claude"
cli_binary = "claude"
system_prompt = "file"           # --append-system-prompt-file
skills = "dir"                   # --add-dir with symlinks
cwd_config = "CLAUDE.md"         # auto-loaded from CWD
tool_whitelist = true            # --allowedTools
sandbox = "seatbelt"             # macOS sandbox (--sandbox not exposed in headless)
cancel = "sigterm"               # SIGTERM -> grace -> SIGKILL
resume = "session_id"            # --resume <id> (if cwd matches)
usage_metrics = "stream_json"    # --output-format stream-json
output_capture = "stdout"        # streamed to stdout
parallel_auth = "oauth_token"    # CLAUDE_CODE_OAUTH_TOKEN
env_sanitize = ["CLAUDECODE", "ANTHROPIC_API_KEY"]  # must unset

[adapters.codex]
provider = "codex"
cli_binary = "codex"
system_prompt = "prompt_prefix"  # prepended to prompt text
skills = "none"                  # not supported
cwd_config = "AGENTS.md"         # read from CWD
tool_whitelist = false           # not supported in headless
sandbox = "built_in"             # codex has own sandbox
cancel = "sigterm"
resume = "none"                  # no session resume
usage_metrics = "exit_summary"   # sometimes in output, not reliable
output_capture = "file"          # --output-last-message <file>
parallel_auth = "auto"           # uses ~/.codex/ credentials
env_sanitize = []

[adapters.gemini]
provider = "gemini"
cli_binary = "gemini"
system_prompt = "prompt_prefix"  # prepended to prompt text
skills = "none"
cwd_config = "GEMINI.md"         # auto-loaded from CWD
tool_whitelist = false
sandbox = "none"                 # --approval-mode + worktree isolation
cancel = "sigterm"
resume = "session"               # -r latest (cwd-scoped)
usage_metrics = "none"           # no structured usage output
output_capture = "stdout"        # -o text
parallel_auth = "auto"           # uses ~/.gemini/ credentials
env_sanitize = []

[adapters.process]
provider = "custom"
cli_binary = "configurable"
system_prompt = "env_var"        # PUNK_SYSTEM_PROMPT env var
skills = "none"
cwd_config = "none"
tool_whitelist = false
sandbox = "none"
cancel = "sigterm"
resume = "none"
usage_metrics = "receipt"        # agent writes own receipt
output_capture = "stdout"
parallel_auth = "configurable"
env_sanitize = []
```

### Policy routing uses capabilities

```toml
# policy.toml
[[rules]]
match = { category = "review", requires = ["system_prompt"] }
# Only agents with system_prompt capability can do reviews
# claude: ok (file), codex: ok (prefix), gemini: ok (prefix)

[[rules]]
match = { category = "codegen", requires = ["skills", "tool_whitelist"] }
# Only claude adapter supports both
set = { provider = "claude" }
```

---

## 6. Failure Taxonomy + Retry Policy

### Error Classification

| error_type | termination_reason | Retryable | Action |
|-----------|-------------------|-----------|--------|
| transient | provider_429 | Yes | Backoff: 30s, 60s, 120s |
| transient | provider_529 | Yes | Backoff: 30s, 60s |
| transient | provider_overloaded | Yes | Backoff: 60s |
| transient | timeout | Yes (once) | Same timeout, fresh run |
| transient | adapter_crash | Yes (once) | Fresh run |
| transient | daemon_crash_recovery | Yes | Fresh run |
| permanent | auth_expired | No | Directive: "run `claude setup-token`" |
| permanent | budget_exceeded | No | Directive: "raise budget or reprioritize" |
| permanent | prompt_too_large | No | Directive: "reduce prompt size" |
| permanent | adapter_not_found | No | Directive: "install provider CLI" |
| permanent | project_not_found | No | Task -> failed/ |
| permanent | user_cancel | No | Task -> cancelled/ |
| unknown | exit_nonzero | Maybe | Check stderr, classify, maybe retry once |

### Retry Policy

```toml
# policy.toml
[retry]
max_attempts = 3                # total attempts (1 original + 2 retries)
backoff_base_s = 30             # first retry after 30s
backoff_multiplier = 2          # 30s, 60s, 120s
backoff_max_s = 300             # cap at 5 min
jitter = true                   # +/- 20% random jitter

# Per error_type overrides:
[retry.overrides.timeout]
max_attempts = 2                # only 1 retry for timeouts

[retry.overrides.auth_expired]
max_attempts = 1                # never retry auth failures
```

### Circuit Breaker (per provider)

```
State machine: CLOSED -> OPEN -> HALF_OPEN -> CLOSED

CLOSED (normal):
  Track consecutive failures per provider.
  If failures >= 3: transition to OPEN.

OPEN (provider unhealthy):
  All tasks for this provider -> queued with alternate provider (if available).
  After cooldown (5 min): transition to HALF_OPEN.

HALF_OPEN (testing):
  Allow 1 task through.
  If succeeds: transition to CLOSED, reset counter.
  If fails: transition to OPEN, restart cooldown.

Storage: state/bus/.circuit/<provider>.json
  {"state": "open", "failures": 3, "opened_at": "...", "cooldown_until": "..."}
```

### Dead-Letter Queue

Tasks that fail all retries AND are not retryable go to `state/bus/dead/`:
```
state/bus/dead/<task_id>/
  task.json           # original task
  runs/               # all run attempts
  diagnosis.json      # {error_type, termination_reason, attempts, last_stderr_excerpt}
```

`punk triage` interactively reviews dead-letter tasks. Options: retry with different agent, retry with higher budget, archive, delete.

---

## 7. Prompt/Context Manifest

Every run records exactly what the agent saw, for audit and reproducibility.

### Manifest Schema

Written to `state/bus/runs/<task_id>/run-<N>.manifest.json`:

```json
{
  "run_id": "signum-20260327-140000-1",
  "prompt_hash": "sha256:abc123...",
  "context_hash": "sha256:def456...",
  "manifest": {
    "system_prompt": {
      "source": "agents/reviewer.md",
      "hash": "sha256:...",
      "injection": "append-system-prompt-file"
    },
    "session_context": {
      "source": "state/sessions/signum/context.json",
      "hash": "sha256:...",
      "entries_count": 7,
      "injection": "prompt_prefix"
    },
    "skills": [
      {"name": "verification", "hash": "sha256:...", "injection": "add-dir"},
      {"name": "code-review", "hash": "sha256:...", "injection": "add-dir"}
    ],
    "cwd_config": {
      "file": "CLAUDE.md",
      "hash": "sha256:...",
      "injection": "auto"
    },
    "task_prompt": {
      "hash": "sha256:...",
      "char_count": 1234,
      "injection": "stdin"
    },
    "env_vars": [
      "PUNK_TASK_ID=signum-20260327-140000",
      "PUNK_RUN_ID=signum-20260327-140000-1",
      "PUNK_PROJECT=signum",
      "PUNK_RECEIPT_PATH=state/bus/runs/.../receipt.json",
      "PUNK_SESSION_PATH=state/sessions/signum/context.json",
      "PUNK_RISK_TIER=T2",
      "PUNK_BUDGET_USD=1.0",
      "PUNK_WORKTREE_PATH=/tmp/punk-wt-..."
    ]
  }
}
```

### Env Var Contract (guaranteed per run)

| Variable | Always | Description |
|----------|--------|-------------|
| PUNK_TASK_ID | yes | Task identifier |
| PUNK_RUN_ID | yes | Run identifier (task_id + attempt) |
| PUNK_PROJECT | yes | Project slug |
| PUNK_PROJECT_PATH | yes | Absolute path to project root |
| PUNK_RECEIPT_PATH | yes | Where agent should write receipt |
| PUNK_SESSION_PATH | if exists | Path to session context.json |
| PUNK_RISK_TIER | yes | T1/T2/T3 |
| PUNK_BUDGET_USD | yes | Max estimated spend for this run |
| PUNK_WORKTREE_PATH | if worktree | Path to isolated worktree |
| PUNK_CATEGORY | yes | Task category |
| PUNK_TIMEOUT_S | yes | Wall-clock timeout |

Secrets are NEVER in env vars or manifests. Provider auth tokens are injected by the adapter layer, not exposed to the agent.

---

## 8. Provenance Contract for `punk ask`

### Design Principle

`punk ask` = AI synthesis over structured data. NOT a replacement for `punk status`.

```
punk status  = deterministic, always correct, no AI
punk ask     = synthesized, may be wrong, cites sources
```

### Provenance Rules

1. **Every claim must cite a source**: receipt ID, task ID, or file path.
2. **Data freshness always shown**: "Based on N receipts from last 24h. Oldest: 3h ago."
3. **Deterministic questions use deterministic answers**: "how many tasks failed today?" -> count from index.jsonl, not LLM synthesis.
4. **Synthesized answers marked**: "Based on receipt analysis:" vs "Exact count:"
5. **Uncertainty explicit**: "I found no receipts matching this query" rather than fabricating.

### Implementation

```
punk ask "what is blocking signum?"

1. Daemon reads:
   - receipts/index.jsonl (last 7 days, filtered by project=signum)
   - state/sessions/signum/context.json
   - state/cycle/directive.md
   - state/bus/cur/ (currently running tasks)
   - state/bus/dead/ (dead-letter tasks)

2. Builds context preamble (deterministic, no AI):
   """
   Data snapshot (2026-03-27T14:30:00Z):
   - Receipts (7d): 23 total, 19 success, 3 failure, 1 timeout
   - Failed tasks: [signum-20260326-fix-auth (permanent: prompt_too_large),
                     signum-20260327-update-deps (transient: provider_429, retried ok)]
   - Dead letter: 1 task (signum-20260325-refactor, 3 attempts exhausted)
   - Currently running: 0
   - Session context: 7 entries (2 failures, 1 surprise)
   - Directives: checkpoint in 4d (2026-03-31)
   """

3. Sends to LLM (haiku via OAuth API or CLI):
   "Based ONLY on the data above, answer: what is blocking signum?
    Rules: cite task IDs, don't invent data not in the snapshot, say 'unknown' if data insufficient."

4. Output:
   """
   ## signum status

   **Based on:** 23 receipts (last 7d), 1 dead-letter task

   **Blocking:**
   - Dead-letter task `signum-20260325-refactor` exhausted 3 attempts
     (last failure: adapter_crash). Needs manual triage via `punk triage`.
   - Checkpoint in 4 days (2026-03-31) — no qualified_user_conversations progress.

   **Recent issues:**
   - `signum-20260326-fix-auth` failed permanently (prompt too large).

   Data freshness: 23 receipts, newest 2h ago. Run `punk status --project signum` for exact counts.
   """
```

### Hallucination Guard

- **Context-only answering**: system prompt explicitly says "answer ONLY from the provided data"
- **No web search**: `punk ask` never calls WebSearch/WebFetch
- **Deterministic fallback**: if question is purely quantitative ("how many failed?"), skip LLM, compute from index.jsonl directly
- **Citation required**: every fact in the answer must reference a task_id, receipt, or data source
- **Freshness warning**: if newest receipt is >6h old, prepend warning

---

## 9. Goal System (Autonomous Cycle)

Human sets objective, system plans and executes autonomously.

### Entity Model

```
Goal (1) --> Plan (versioned) --> Steps (ordered) --> Tasks (N per step)
                                                        |
                                                   Sub-Tasks (agent-created)
```

### Goal Storage

`state/goals/<goal-id>.json`:

```json
{
  "id": "signum-checkpoint-20260327",
  "project": "signum",
  "objective": "Get 5 qualified user conversations by March 31",
  "deadline": "2026-03-31",
  "budget_usd": 6.0,
  "spent_usd": 3.20,
  "status": "active",
  "plan": {
    "version": 1,
    "created_by": "claude-opus (planner)",
    "approved_at": "2026-03-27T14:05:00Z",
    "steps": [
      {
        "step": 1,
        "category": "research",
        "prompt": "Audit current onboarding flow, identify where users drop off",
        "agent": "codex-auto",
        "est_cost_usd": 0.50,
        "depends_on": [],
        "status": "done",
        "task_id": "signum-20260327-140500"
      },
      {
        "step": 2,
        "category": "fix",
        "prompt": "Fix bugs #12, #15, #17 found in onboarding",
        "agent": "claude-sonnet",
        "est_cost_usd": 2.00,
        "depends_on": [1],
        "status": "running",
        "task_id": "signum-20260327-141200",
        "sub_tasks": ["signum-20260327-141500"]
      }
    ]
  }
}
```

### Planner Agent

When `punk goal <project> "objective"` is invoked:

1. **Planner task** created (category: plan, agent: opus-class model via OAuth API)
2. Planner reads:
   - Project codebase (CLAUDE.md, README, key source files)
   - Session context (what happened recently)
   - Recent receipts (what worked/failed)
   - agent-brief.md (current metrics)
   - adapters.toml (what agents can do)
3. Planner generates plan:
   - 5-15 steps, each with: prompt, category, agent, est_cost, dependencies, success_criteria
   - Total estimated cost and time
4. Plan presented to human for approval
5. On approve: goal status -> active, first steps queued

### Goal Daemon Loop (every 30s)

```
for each goal where status == "active":
  for each step in plan.steps:
    if step.status == "done":
      continue
    if step.status == "running":
      task = lookup(step.task_id)
      if task in done/:
        receipt = read receipt
        if receipt.status == "success":
          step.status = "done"
          # Check for concerns
          if receipt.concerns:
            create sub-task to address concerns
          # Queue next steps whose deps are now met
          queue_ready_steps(goal)
        elif receipt.status == "failure":
          # Retry handled by task-level retry policy
          # If exhausted: step.status = "blocked"
          step.status = "blocked"
          emit directive: "Goal step blocked"
      continue
    if step.status == "pending":
      if all(plan.steps[dep].status == "done" for dep in step.depends_on):
        # Dependencies met, queue this step
        task = create_task(step, goal)
        step.task_id = task.id
        step.status = "running"

  # Check re-plan triggers
  if goal.spent_usd > goal.budget_usd * 0.8 and progress < 50%:
    emit directive: "Goal over budget, consider replan"
  if any step blocked for > 24h:
    emit directive: "Goal blocked, needs attention"
  if all steps done:
    goal.status = "done"
    emit directive: "Goal completed"
```

### Sub-Task Creation (agent-initiated)

During step execution, agents can create sub-tasks via receipt fields:

```json
// receipt.json
{
  "status": "success",
  "follow_up_tasks": [
    {"prompt": "Fix test regression in auth module", "category": "fix", "priority": "p1"},
    {"prompt": "Run e2e tests after auth fix", "category": "audit", "priority": "p1"}
  ]
}
```

Daemon reads `follow_up_tasks` from receipt and queues them as sub-tasks of the current step. Sub-tasks must complete before the step is marked "done."

### Re-Planning Triggers

| Trigger | Action |
|---------|--------|
| Budget > 80% consumed, < 50% steps done | Directive + offer replan |
| Step blocked > 24h after retry exhausted | Directive + offer replan |
| 2+ consecutive step failures | Auto-pause, ask human |
| Deadline < 2 days away, > 30% steps pending | Directive: "unlikely to meet deadline" |

Re-plan = new planner run with all receipts as context. Human approves if >30% of plan changes.

### Human Controls

```bash
punk goals                    # see all goals with progress bars
punk goal status <id>         # detailed step-by-step view
punk goal pause <id>          # stop autonomous execution
punk goal replan <id>         # force new plan
punk goal cancel <id>         # abort
punk goal budget <id> <usd>   # adjust
```

Human is never surprised. All autonomous actions are visible in `punk morning` and `punk status`.

---

## 10. Pipeline (Lightweight CRM)

Replaces adjutant. Flat-file storage, no SQLite.

### Storage

`state/pipeline.jsonl` - append-only events:
```jsonl
{"id":1,"event":"created","project":"signum","contact":"John CTO","stage":"lead","next_step":"Demo call","due":"2026-04-01","value_usd":5000,"ts":"2026-03-27T10:00:00Z"}
{"id":1,"event":"advanced","stage":"qualified","next_step":"Send proposal","due":"2026-04-05","ts":"2026-03-30T14:00:00Z"}
{"id":1,"event":"won","reason":"Great demo, immediate need","ts":"2026-04-03T16:00:00Z"}
```

Current state per opportunity = last event for that id.

### Commands

```bash
punk pipeline                          # table of active opportunities
punk pipeline add signum "John" --stage lead --next "Demo" --due 2026-04-01
punk pipeline advance 1                # lead -> qualified
punk pipeline win 1 --reason "..."     # mark won
punk pipeline lose 1 --reason "..."    # mark lost
punk pipeline stale                    # overdue next_steps
```

### Integration with Goals

Business goals can reference pipeline:
```bash
punk goal signum "Convert John CTO from lead to won"
```

Planner sees pipeline state and generates steps: prepare demo, send proposal, follow up.

### Scale Ceiling

Flat JSONL works for 0-50 opportunities. If pipeline grows, upgrade to SQLite backend (adjutant's schema is the reference).

---

## Resolved Open Questions

| Question | Resolution |
|----------|-----------|
| Gemini OAuth -> API | Not possible. `gemini login` uses internal endpoint. Use CLI (Tier 1) or free API key (Tier 1.5). |
| Worktree lifecycle | `/tmp/punk-wt-<task_id>`, detached HEAD, stale GC 5min, no auto-merge. |
| Secret management | Project `.env` > macOS keychain > env inheritance. Redaction on output. |
| adjutant integration | Killed. Pipeline absorbed as flat JSONL. Goals replace business ops. |
| Multi-project slots | Per-project quota (default 2) + fair share + p0 reserved slot. |

## Remaining Open Questions

1. **Sandbox standardization**: Claude has Seatbelt, Codex built-in, Gemini nothing. Standardize?
2. **Resume semantics**: Claude `--resume <session_id>` on transient failures. Worth it?
3. **Self-authored skill review gate**: security scan sufficient, or human approval needed?
4. **Session compaction cost**: who pays for LLM summarization calls?
5. **Flat-file scale ceiling**: when to upgrade pipeline/receipts to SQLite?
