# Rust Architecture Patterns for Agent Orchestrator Daemon

Date: 2026-03-27
Decision: Hexagonal traits + enum dispatch + tokio channels. No actors, no ECS.

## Evaluated Patterns

| Pattern | Verdict | Reason |
|---------|---------|--------|
| Actors (kameo/ractor/actix) | **Skip** | Overkill for 5 slots. tokio tasks + channels sufficient |
| ECS (bevy/hecs) | **Skip** | Overkill, unfamiliar to AI agents, can't serialize easily |
| Hexagonal / ports-and-adapters | **Use** | Traits as ports, clean separation, new adapter = one impl |
| Enum dispatch | **Use** | Exhaustive match = compile errors on new variant. AI safety net |
| tokio channels | **Use** | mpsc(queue) + watch(state) + broadcast(events) + oneshot(rpc) |
| Enum FSM | **Use** | Task lifecycle as enum, serializable, AI-friendly |
| arc-swap | **Use** | Lock-free config reload on SIGHUP |
| process groups | **Use** | command-group + nix for killing entire process tree |
| Typestate | **Skip** | Can't serialize, AI agents struggle with it |
| Box<dyn Trait> | **Later** | When we need runtime-loaded plugins. Enum dispatch first |

## Crate Stack

```toml
tokio = { version = "1.44", features = ["full"] }
command-group = "2.1"
arc-swap = "1.7"
notify = "6.1"
signal-hook = "0.3"
thiserror = "2"
anyhow = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

## Key Patterns

### Hexagonal: Traits as Ports
```rust
pub trait Executor: Send + Sync {
    async fn run(&self, task: &Task, config: &AdapterConfig) -> Result<RunResult>;
    fn name(&self) -> &str;
}

pub trait Store: Clone + Send + Sync + 'static {
    async fn enqueue(&self, task: Task) -> Result<()>;
    async fn claim_next(&self, slot_id: SlotId) -> Option<Task>;
    async fn mark_done(&self, id: TaskId, receipt: Receipt) -> Result<()>;
}
```
New adapter = implement trait. Domain logic untouched.

### Enum Dispatch for Adapters
```rust
enum ExecutorKind {
    Claude(ClaudeExecutor),
    Codex(CodexExecutor),
    Gemini(GeminiExecutor),
}
```
Adding new provider = add variant + impl. Compiler catches all unhandled matches.

### Enum FSM for Task Lifecycle
```rust
enum TaskState {
    Queued { enqueued_at: DateTime<Utc> },
    Claimed { slot_id: SlotId, claimed_at: DateTime<Utc> },
    Running { pid: u32, started_at: DateTime<Utc> },
    Done { receipt: Receipt },
    Failed { error: String, attempts: u8 },
}

impl TaskState {
    fn transition(self, event: TaskEvent) -> Result<Self, InvalidTransition> {
        match (self, event) { ... }
    }
}
```

### arc-swap for Config Hot Reload
```rust
static CONFIG: Lazy<ArcSwap<Config>> = Lazy::new(|| ArcSwap::from_pointee(Config::default()));
// Read: wait-free. Reload: store(Arc::new(new_config))
```

### Process Groups for Clean Kill
```rust
use command_group::CommandGroup;
let child = Command::new("claude").process_group(0).spawn()?;
// Kill entire tree: kill(-pgid, SIGTERM)
```

## Why This Works for AI-Written Code

1. Exhaustive match = compiler catches missing cases on new enum variant
2. Trait bounds = compiler verifies all methods implemented
3. No unsafe = agents can't corrupt memory
4. cargo check = fast feedback loop (seconds, not minutes)
5. Structured errors (thiserror) = clear error propagation
6. Hexagonal boundaries = new adapter is ONE file, no ripple effects

## What We Don't Need

- Supervision trees (ractor) - 5 slots don't need Erlang-style supervision
- Message routing (actor mailboxes) - tokio channels are simpler and sufficient
- Data-oriented design (ECS) - entities are 5-50, not 5000
- Dynamic loading (libloading) - adapters are compiled in, config is TOML
- Distributed actors - single machine, no cluster
