# Session Handoff: Post-v2 Ship

## Context
Specpunk v2 fully implemented and deployed. 2026-03-27/28 mega session.
Rust daemon replaced bash supervisor in production.

## What Was Built
19 commits, ~14K LOC, 28 commands, 344 tests, 26 modules.

### punk-run CLI (28 commands)
```
status [--project] [-n]    config    daemon [--shadow] [--background]
morning    triage    retry    cancel    doctor    policy-check
queue (full flags)    receipts [--project] [--since]
goal create/list/status/approve/pause/resume/cancel/budget/replan
ask    pipeline list/add/advance/win/lose/stale
diverge    panel    skill list/create    ratchet    graph
context    recall    remember
```

### punk-orch Modules (26)
adapter, budget, bus, config, context, daemon, diverge, doctor, followup,
goal, graph, morning, ops, panel, pipeline, queue, ratchet, recall,
receipt, run, sanitize, session, skill, task, triage

### Key Files
- Binary: `~/.cargo/bin/punk-run`
- Config: `~/.config/punk/{projects,agents,policy}.toml`
- Bus: `~/vicc/state/bus/` (new/cur/done/failed/dead)
- Knowledge: `~/vicc/state/knowledge/events.jsonl`
- Goals: `~/vicc/state/goals/*.json`
- Sessions: `~/vicc/state/sessions/*.json`
- Skills: `~/vicc/state/skills/*.md`
- Receipts: `~/vicc/state/receipts/index.jsonl`
- launchd: `~/Library/LaunchAgents/dev.punk.daemon.plist`
- Schema: `punk/schemas/receipt.v1.schema.json`

## Production State
- `dev.punk.daemon` = Rust daemon (KeepAlive, PID tracked)
- `dev.punk.supervisor` = decommissioned (unloaded 2026-03-28)
- `dev.punk.watchdog` = still running (bash, monitors health)

## What To Do Next
1. **Publish**: `cargo publish` to crates.io for external_installs metric
2. **Close GitHub issues**: #8 (depends_on), #10 (cancel), #11 (heartbeat) — resolved in punk-run
3. **Monitoring**: watch daemon logs for 1 week (`~/vicc/state/collectors/punk-daemon.err`)
4. **Agent guidance files**: create `~/.config/punk/agents/*.md` for each agent role
5. **Skills**: create project-specific skills in `~/vicc/state/skills/`

## Not Done (deferred)
- OAuth API fast path for `punk-run ask` (optimization)
- Parallel tool safety classification
- punk recall distillation (events → invariants)
- cargo publish to crates.io

## How To Read Code
1. `punk-orch/src/daemon.rs` — main loop, dispatch, completion handling
2. `punk-orch/src/context.rs` — unified context assembly (Linear Next pattern)
3. `punk-orch/src/queue.rs` — filesystem bus protocol (slots, locks, heartbeat)
4. `punk-orch/src/run.rs` — failure taxonomy, retry, circuit breaker, smart routing
5. `punk-orch/src/goal.rs` — goal system (plan, steps, eval)
6. `punk-orch/src/recall.rs` — institutional memory (auto-capture + recall)
7. `punk-run/src/main.rs` — CLI entry point (28 commands)
