use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;
use tokio::time;

use crate::adapter::{Adapter, SpawnedProcess, TaskSpec};
use crate::budget;
use crate::goal::{self, GoalStatus, StepStatus};
use crate::queue::{self, HeartbeatTracker, ProjectLock, SlotManager};
use crate::receipt::{CallStyle, Receipt, ReceiptStatus};
use crate::run::{self, CircuitBreaker, RetryDecision, Run};
use crate::session;

/// Active task being tracked by the daemon.
struct ActiveTask {
    run: Run,
    process: SpawnedProcess,
    staging_dir: PathBuf,
    task_json: Value,
    adapter_name: String,
}

/// Daemon configuration.
pub struct DaemonConfig {
    pub bus_dir: PathBuf,
    pub poll_interval: Duration,
    pub max_slots: u32,
    pub max_attempts: u32,
    pub backoff_base_s: u64,
    pub backoff_multiplier: u64,
    pub backoff_max_s: u64,
    pub shadow: bool,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            bus_dir: crate::bus::bus_dir(),
            poll_interval: Duration::from_secs(5),
            max_slots: 5,
            max_attempts: 3,
            backoff_base_s: 30,
            backoff_multiplier: 2,
            backoff_max_s: 300,
            shadow: false,
        }
    }
}

/// Run the daemon main loop.
pub async fn run(dcfg: DaemonConfig) {
    let bus = &dcfg.bus_dir;
    eprintln!(
        "punk-run daemon: bus={} slots={} shadow={}",
        bus.display(),
        dcfg.max_slots,
        dcfg.shadow
    );

    // Ensure directories exist
    for sub in &[
        "new/p0", "new/p1", "new/p2", "cur", "done", "failed", "dead",
    ] {
        fs::create_dir_all(bus.join(sub)).ok();
    }

    let slots = SlotManager::new(bus, dcfg.max_slots);
    let locks = ProjectLock::new(bus);
    let heartbeats = HeartbeatTracker::new(bus);

    // Crash recovery: reap stale slots, scan cur/ for orphans, then clear locks
    heartbeats.recover_stale_slots(&slots);

    // Adopt orphaned tasks from cur/ (left from previous daemon crash)
    if let Ok(entries) = fs::read_dir(bus.join("cur")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let task_id = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let pid_file = bus.join(".pids").join(format!("{task_id}.pid"));
                let process_alive = fs::read_to_string(&pid_file)
                    .ok()
                    .and_then(|s| s.trim().parse::<i32>().ok())
                    .is_some_and(|pid| unsafe { libc::kill(pid, 0) == 0 });

                if !process_alive {
                    // Orphaned task: process dead, move to failed
                    eprintln!("daemon: crash recovery: orphan {task_id} → failed/");
                    queue::move_to_failed(bus, &task_id);
                    heartbeats.unregister(&task_id);
                    log_event(bus, "orphan_recovered", &format!(",\"task\":\"{task_id}\""));
                } else {
                    eprintln!("daemon: crash recovery: {task_id} still alive (PID), skipping");
                }
            }
        }
    }

    locks.clear_all();

    let mut active: HashMap<String, ActiveTask> = HashMap::new();
    let mut circuits: HashMap<String, CircuitBreaker> = HashMap::new();

    let mut interval = time::interval(dcfg.poll_interval);

    log_event(bus, "daemon_started", "");

    loop {
        interval.tick().await;

        // 0. Process cancel signals (written by punk-run cancel)
        let cancel_dir = bus.join(".cancel");
        if let Ok(entries) = fs::read_dir(&cancel_dir) {
            for entry in entries.flatten() {
                let task_id = entry.file_name().to_string_lossy().to_string();
                if let Some(mut task) = active.remove(&task_id) {
                    // Kill the process
                    if let Some(pid) = task.process.child.id() {
                        unsafe {
                            libc::kill(pid as i32, libc::SIGTERM);
                        }
                    }
                    task.run.mark_failed(
                        130,
                        task.run.duration_ms,
                        crate::run::TerminationReason::UserCancel,
                    );
                    save_run(bus, &task.run);
                    heartbeats.unregister(&task_id);
                    slots.release(task.run.slot_id);
                    let proj = task
                        .task_json
                        .get("project")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    locks.release(proj);
                    move_staging(bus, &task.staging_dir, &task_id, "failed");
                    fs::remove_file(bus.join("cur").join(format!("{task_id}.json"))).ok();
                    log_event(bus, "cancelled", &format!(",\"task\":\"{task_id}\""));
                    eprintln!("daemon: cancelled {task_id}");
                }
                fs::remove_file(entry.path()).ok();
            }
        }

        // 0b. Update heartbeats for still-running tasks (daemon is the heartbeat source)
        for (task_id, _) in active.iter() {
            let hb_path = bus.join(".heartbeats").join(format!("{task_id}.hb"));
            fs::write(&hb_path, Utc::now().to_rfc3339()).ok();
        }

        // 1. Collect completed processes
        collect_completed(
            bus,
            &mut active,
            &slots,
            &locks,
            &heartbeats,
            &mut circuits,
            &dcfg,
        )
        .await;

        // 2. Check heartbeats for stale tasks
        reap_stale(bus, &heartbeats, &mut active, &slots, &locks).await;

        // 3. Process retry queue
        process_retries(bus);

        // 3.5. Evaluate active goals (check step completion, queue next steps)
        evaluate_goals(bus);

        // 4. Budget backpressure check
        let (pressure, spent) = budget::check_pressure(bus, 50.0, 80, 95);
        if pressure != budget::PressureLevel::Normal {
            eprintln!(
                "daemon: budget pressure {:?} (${:.2} spent)",
                pressure, spent
            );
        }

        // 5. Scan and dispatch new tasks
        if !dcfg.shadow {
            dispatch_queued(
                bus,
                &slots,
                &locks,
                &heartbeats,
                &mut active,
                &mut circuits,
                &dcfg,
            )
            .await;
        } else {
            // Shadow mode: log what we would do
            let entries = queue::scan_queue(bus);
            if !entries.is_empty() {
                eprintln!("shadow: {} queued tasks found", entries.len());
                for e in &entries {
                    eprintln!(
                        "shadow: would dispatch {} (project={}, model={})",
                        e.task_id, e.project, e.model
                    );
                }
            }
        }
    }
}

async fn dispatch_queued(
    bus: &Path,
    slots: &SlotManager,
    locks: &ProjectLock,
    heartbeats: &HeartbeatTracker,
    active: &mut HashMap<String, ActiveTask>,
    circuits: &mut HashMap<String, CircuitBreaker>,
    _dcfg: &DaemonConfig,
) {
    let entries = queue::scan_queue(bus);

    for entry in entries {
        // Slot available?
        let slot_id = match slots.acquire() {
            Some(s) => s,
            None => break, // all slots full
        };

        // Dependency check
        if !queue::deps_ready(bus, &entry) {
            slots.release(slot_id);
            continue;
        }

        // Circuit breaker check
        let provider = &entry.model;
        let cb = circuits
            .entry(provider.clone())
            .or_insert_with(|| CircuitBreaker::new(provider));
        cb.check_cooldown();
        if !cb.allows() {
            slots.release(slot_id);
            continue;
        }

        // Project lock (skip if worktree task)
        if !entry.worktree && !locks.try_acquire(&entry.project, &entry.task_id) {
            slots.release(slot_id);
            continue;
        }

        // Claim task (atomic rename)
        let claimed_path = match queue::claim_task(bus, &entry) {
            Some(p) => p,
            None => {
                slots.release(slot_id);
                if !entry.worktree {
                    locks.release(&entry.project);
                }
                continue;
            }
        };

        slots.record_owner(slot_id, &entry.task_id);
        log_event(
            bus,
            "claimed",
            &format!(",\"task\":\"{}\",\"slot\":{slot_id}", entry.task_id),
        );

        // Read full task JSON
        let task_json: Value = match fs::read_to_string(&claimed_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(v) => v,
            None => {
                eprintln!("daemon: failed to read claimed task {}", entry.task_id);
                queue::move_to_failed(bus, &entry.task_id);
                slots.release(slot_id);
                locks.release(&entry.project);
                continue;
            }
        };

        // Resolve agent → provider/model via agents.toml
        let config_dir = crate::config::config_dir();
        let (provider, agent_model, agent_id) = if let Ok(cfg) = crate::config::load(&config_dir) {
            if let Some(agent) = cfg.agents.agents.get(&entry.model) {
                // Task specified an agent ID (e.g. "claude-reviewer")
                (
                    agent.provider.clone(),
                    agent.model.clone(),
                    entry.model.clone(),
                )
            } else {
                // Fallback: treat entry.model as raw provider name
                (entry.model.clone(), String::new(), entry.model.clone())
            }
        } else {
            (entry.model.clone(), String::new(), entry.model.clone())
        };

        let adapter = match Adapter::from_provider(&provider) {
            Some(a) => a,
            None => {
                eprintln!("daemon: unknown provider '{provider}' (agent: {agent_id})");
                queue::move_to_failed(bus, &entry.task_id);
                slots.release(slot_id);
                locks.release(&entry.project);
                continue;
            }
        };

        // Count attempts from runs dir
        let attempt = count_attempts(bus, &entry.task_id) + 1;

        // Create run entity
        let mut run_entity = Run::new(&entry.task_id, attempt, slot_id, &agent_id, &provider);

        // Build TaskSpec
        let project_path = task_json
            .get("project_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .replace('~', &dirs::home_dir().unwrap_or_default().to_string_lossy());

        let raw_prompt = task_json
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Auto-triage: infer category if not specified
        let task_category = task_json
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let effective_category = if task_category.is_empty() {
            crate::triage::infer_category(&raw_prompt)
        } else {
            task_category
        };

        // Model resolution: agent config > task field > smart routing
        let explicit_model = if !agent_model.is_empty() {
            agent_model.clone()
        } else {
            task_json
                .get("claude_model")
                .and_then(|v| v.as_str())
                .or_else(|| task_json.get("model_override").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string()
        };
        let routed_model = if explicit_model.is_empty() {
            run::route_model(&raw_prompt, "sonnet")
        } else {
            run::route_model(&raw_prompt, &explicit_model)
        };

        // Unified context injection (Linear Next pattern):
        // agent guidance + skills + recall + session + project stats
        let context_pack = crate::context::ContextPack::build(
            bus,
            &entry.project,
            effective_category,
            &agent_id,
            &config_dir,
        );
        let context_prefix = context_pack.format();
        let prompt_with_context = if context_prefix.is_empty() {
            raw_prompt
        } else {
            format!("{context_prefix}{raw_prompt}")
        };

        let task_spec = TaskSpec {
            task_id: entry.task_id.clone(),
            project: entry.project.clone(),
            project_path: PathBuf::from(&project_path),
            prompt: prompt_with_context,
            model: routed_model,
            timeout_s: task_json.get("timeout_seconds").and_then(|v| v.as_u64()).unwrap_or(600),
            budget_usd: task_json.get("max_budget_usd").and_then(|v| v.as_f64()),
            allowed_tools: task_json.get("allowedTools").and_then(|v| v.as_str())
                .unwrap_or("Read,Write,Edit,Bash(*)").to_string(),
            disallowed_tools: task_json.get("disallowedTools").and_then(|v| v.as_str())
                .unwrap_or("Bash(git push *),Bash(git remote *),Bash(rm -rf *),Bash(sudo *),Bash(curl *),Bash(wget *)").to_string(),
        };

        // Create staging dir
        let staging_dir = PathBuf::from(format!("/tmp/punk-stage-{}-{attempt}", entry.task_id));
        fs::create_dir_all(&staging_dir).ok();
        fs::copy(&claimed_path, staging_dir.join("task.json")).ok();

        // Spawn adapter
        match adapter.spawn(&task_spec, &staging_dir).await {
            Ok(process) => {
                let pid = process.pid;
                run_entity.mark_started(pid);
                heartbeats.register(&entry.task_id, pid);

                // Save run entity
                save_run(bus, &run_entity);

                log_event(
                    bus,
                    "started",
                    &format!(
                        ",\"task\":\"{}\",\"pid\":{pid},\"run\":\"{}\"",
                        entry.task_id, run_entity.run_id
                    ),
                );

                active.insert(
                    entry.task_id.clone(),
                    ActiveTask {
                        run: run_entity,
                        process,
                        staging_dir,
                        task_json,
                        adapter_name: adapter.name().to_string(),
                    },
                );
            }
            Err(e) => {
                eprintln!("daemon: spawn failed for {}: {e}", entry.task_id);
                run_entity.mark_failed(1, 0, crate::run::TerminationReason::AdapterCrash);
                save_run(bus, &run_entity);
                queue::move_to_failed(bus, &entry.task_id);
                slots.release(slot_id);
                locks.release(&entry.project);
                heartbeats.unregister(&entry.task_id);
            }
        }
    }
}

async fn collect_completed(
    bus: &Path,
    active: &mut HashMap<String, ActiveTask>,
    slots: &SlotManager,
    locks: &ProjectLock,
    heartbeats: &HeartbeatTracker,
    circuits: &mut HashMap<String, CircuitBreaker>,
    dcfg: &DaemonConfig,
) {
    let mut to_collect = Vec::new();
    for (task_id, task) in active.iter_mut() {
        if let Ok(Some(_status)) = task.process.child.try_wait() {
            to_collect.push(task_id.clone());
        }
    }

    for task_id in to_collect {
        if let Some(mut task) = active.remove(&task_id) {
            let result = task.process.wait().await;
            let project = task
                .task_json
                .get("project")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let category = task
                .task_json
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("codegen");

            if result.exit_code == 0 {
                // Success
                task.run.mark_success(result.duration_ms);

                let cb = circuits
                    .entry(task.adapter_name.clone())
                    .or_insert_with(|| CircuitBreaker::new(&task.adapter_name));
                cb.record_success();

                // Write v1 receipt
                let receipt = Receipt {
                    schema_version: 1,
                    task_id: task_id.clone(),
                    status: ReceiptStatus::Success,
                    agent: task.run.agent.clone(),
                    model: task.run.model.clone(),
                    project: project.to_string(),
                    category: category.to_string(),
                    call_style: Some(CallStyle::ToolUse),
                    tokens_used: 0,
                    cost_usd: extract_cost(&result.stdout_path),
                    duration_ms: result.duration_ms,
                    exit_code: 0,
                    artifacts: vec![],
                    errors: vec![],
                    summary: String::new(),
                    created_at: Utc::now(),
                    parent_task_id: None,
                    punk_check_exit: None,
                };

                write_receipt(&task.staging_dir, &receipt);
                append_receipt_index(bus, &receipt);
                save_run(bus, &task.run);
                move_staging(bus, &task.staging_dir, &task_id, "done");

                log_event(
                    bus,
                    "completed",
                    &format!(
                        ",\"task\":\"{task_id}\",\"duration\":{}",
                        result.duration_ms
                    ),
                );

                // Update session context
                session::add_from_receipt(
                    bus,
                    project,
                    &task_id,
                    "success",
                    receipt.cost_usd,
                    0.0,
                    &receipt.summary,
                );

                // Skill auto-authoring trigger (Hermes pattern):
                // complex task (long duration + high cost) may warrant a skill
                if result.duration_ms > 120_000 && receipt.cost_usd > 0.50 {
                    let skill_prompt = format!(
                        "Based on task '{}' (project={}, {}ms, ${:.2}), \
                         create a reusable skill if the task discovered a non-trivial workflow. \
                         Write it to state/skills/<name>.md",
                        task_id, project, result.duration_ms, receipt.cost_usd
                    );
                    eprintln!(
                        "daemon: skill authoring candidate: {task_id} ({}ms, ${:.2})",
                        result.duration_ms, receipt.cost_usd
                    );
                    // Queue a skill-authoring meta-task
                    let skill_task = serde_json::json!({
                        "project": project,
                        "project_path": task.task_json.get("project_path").and_then(|v| v.as_str()).unwrap_or(""),
                        "model": "claude",
                        "prompt": skill_prompt,
                        "category": "content",
                        "timeout_seconds": 120,
                        "max_budget_usd": 0.10,
                        "parent_task_id": task_id,
                    });
                    let skill_task_id = format!("{task_id}-skill");
                    let skill_path = bus.join("new/p2").join(format!("{skill_task_id}.json"));
                    if let Ok(data) = serde_json::to_string_pretty(&skill_task) {
                        fs::write(skill_path, data).ok();
                        log_event(
                            bus,
                            "skill_authoring_queued",
                            &format!(",\"task\":\"{task_id}\",\"trigger\":\"complex_task\""),
                        );
                    }
                }

                // Follow-up task extraction (Linear Next pattern)
                let followups =
                    crate::followup::extract_and_queue(bus, &task_id, project, &result.stdout_path);
                if !followups.is_empty() {
                    eprintln!(
                        "daemon: {} follow-up(s) queued from {task_id}: {}",
                        followups.len(),
                        followups.join(", ")
                    );
                    log_event(
                        bus,
                        "followups_queued",
                        &format!(",\"task\":\"{task_id}\",\"count\":{}", followups.len()),
                    );
                }
            } else {
                // Failure — classify and maybe retry
                let stderr = fs::read_to_string(&result.stderr_path).unwrap_or_default();
                let reason = run::classify_failure(result.exit_code, &stderr);

                task.run
                    .mark_failed(result.exit_code, result.duration_ms, reason.clone());

                let cb = circuits
                    .entry(task.adapter_name.clone())
                    .or_insert_with(|| CircuitBreaker::new(&task.adapter_name));
                cb.record_failure();

                save_run(bus, &task.run);

                let decision = run::should_retry(
                    &reason,
                    task.run.attempt,
                    dcfg.max_attempts,
                    dcfg.backoff_base_s,
                    dcfg.backoff_multiplier,
                    dcfg.backoff_max_s,
                );

                match decision {
                    RetryDecision::Retry { delay_s } => {
                        // 429 → fallback to different provider (Hermes pattern)
                        let mut retry_task = task.task_json.clone();
                        if reason == run::TerminationReason::Provider429
                            || reason == run::TerminationReason::Provider529
                        {
                            if let Some(fallback) = run::fallback_provider(&task.adapter_name) {
                                eprintln!(
                                    "daemon: {} rate-limited on {}, falling back to {fallback}",
                                    task_id, task.adapter_name
                                );
                                retry_task["model"] = serde_json::json!(fallback);
                                log_event(bus, "provider_fallback", &format!(",\"task\":\"{task_id}\",\"from\":\"{}\",\"to\":\"{fallback}\"", task.adapter_name));
                            }
                        }

                        eprintln!(
                            "daemon: {} failed ({reason:?}), retry in {delay_s}s (attempt {})",
                            task_id, task.run.attempt
                        );
                        // Schedule delayed requeue
                        let requeue_at = Utc::now().timestamp() as u64 + delay_s;
                        let requeue_path = bus.join("new/p1").join(format!("{task_id}.json"));
                        // Store task data in staging for delayed write
                        let retry_staging = bus.join("runs").join(&task_id);
                        fs::create_dir_all(&retry_staging).ok();
                        let retry_ok = serde_json::to_string_pretty(&retry_task)
                            .ok()
                            .and_then(|task_data| {
                                fs::write(retry_staging.join("retry-pending.json"), &task_data).ok()
                            })
                            .and_then(|_| {
                                fs::write(
                                    retry_staging.join("retry-meta.json"),
                                    format!(
                                        "{{\"requeue_at\":{requeue_at},\"requeue_path\":\"{}\"}}",
                                        requeue_path.display()
                                    ),
                                )
                                .ok()
                            })
                            .is_some();
                        if !retry_ok {
                            eprintln!(
                                "daemon: failed to schedule retry for {task_id}, moving to failed/"
                            );
                            move_staging(bus, &task.staging_dir, &task_id, "failed");
                        }
                        log_event(
                            bus,
                            "retry_scheduled",
                            &format!(
                                ",\"task\":\"{task_id}\",\"delay\":{delay_s},\"attempt\":{}",
                                task.run.attempt + 1
                            ),
                        );
                    }
                    RetryDecision::Exhausted | RetryDecision::NotRetryable => {
                        // Write failure receipt
                        let receipt = Receipt {
                            schema_version: 1,
                            task_id: task_id.clone(),
                            status: if task.run.status == crate::run::RunStatus::Timeout {
                                ReceiptStatus::Timeout
                            } else {
                                ReceiptStatus::Failure
                            },
                            agent: task.run.agent.clone(),
                            model: task.run.model.clone(),
                            project: project.to_string(),
                            category: category.to_string(),
                            call_style: None,
                            tokens_used: 0,
                            cost_usd: 0.0,
                            duration_ms: result.duration_ms,
                            exit_code: result.exit_code,
                            artifacts: vec![],
                            errors: vec![stderr
                                .lines()
                                .take(5)
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>()
                                .join("\n")],
                            summary: format!("{reason:?}"),
                            created_at: Utc::now(),
                            parent_task_id: None,
                            punk_check_exit: None,
                        };

                        write_receipt(&task.staging_dir, &receipt);
                        append_receipt_index(bus, &receipt);

                        let dest = if matches!(decision, RetryDecision::Exhausted) {
                            "dead"
                        } else {
                            "failed"
                        };
                        move_staging(bus, &task.staging_dir, &task_id, dest);

                        log_event(
                            bus,
                            "failed",
                            &format!(
                                ",\"task\":\"{task_id}\",\"reason\":\"{reason:?}\",\"attempts\":{}",
                                task.run.attempt
                            ),
                        );

                        session::add_from_receipt(
                            bus,
                            project,
                            &task_id,
                            "failure",
                            0.0,
                            0.0,
                            &format!("{reason:?}"),
                        );

                        // Auto-capture to knowledge store (punk recall)
                        crate::recall::capture_from_failure(
                            bus,
                            &task_id,
                            project,
                            &format!("{reason:?}"),
                            &stderr.lines().take(3).collect::<Vec<_>>().join("\n"),
                            &receipt.artifacts,
                        );
                    }
                }
            }

            // Cleanup
            heartbeats.unregister(&task_id);
            slots.release(task.run.slot_id);
            let project = task
                .task_json
                .get("project")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            locks.release(project);

            // Remove from cur/
            fs::remove_file(bus.join("cur").join(format!("{task_id}.json"))).ok();
        }
    }
}

async fn reap_stale(
    bus: &Path,
    heartbeats: &HeartbeatTracker,
    active: &mut HashMap<String, ActiveTask>,
    slots: &SlotManager,
    locks: &ProjectLock,
) {
    let stale = heartbeats.find_stale(&bus.join("cur"));
    for s in stale {
        eprintln!(
            "daemon: stale task {} (age={}s, timeout={}s)",
            s.task_id, s.age_s, s.timeout_s
        );

        // Kill the process
        if let Some(pid) = s.pid {
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
            // Give 2s for graceful shutdown, then SIGKILL
            tokio::time::sleep(Duration::from_secs(2)).await;
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }

        // Remove from active tracking and persist final state
        if let Some(mut task) = active.remove(&s.task_id) {
            task.run.mark_timeout(s.age_s * 1000);
            save_run(bus, &task.run);

            let project = task
                .task_json
                .get("project")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let category = task
                .task_json
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("codegen");

            // Write timeout receipt
            let receipt = Receipt {
                schema_version: 1,
                task_id: s.task_id.clone(),
                status: ReceiptStatus::Timeout,
                agent: task.run.agent.clone(),
                model: task.run.model.clone(),
                project: project.to_string(),
                category: category.to_string(),
                call_style: None,
                tokens_used: 0,
                cost_usd: 0.0,
                duration_ms: s.age_s * 1000,
                exit_code: 124,
                artifacts: vec![],
                errors: vec![format!(
                    "timeout after {}s (limit: {}s)",
                    s.age_s, s.timeout_s
                )],
                summary: "timeout".to_string(),
                created_at: Utc::now(),
                parent_task_id: None,
                punk_check_exit: None,
            };
            write_receipt(&task.staging_dir, &receipt);
            append_receipt_index(bus, &receipt);
            move_staging(bus, &task.staging_dir, &s.task_id, "failed");

            slots.release(task.run.slot_id);
            locks.release(project);
        } else {
            queue::move_to_failed(bus, &s.task_id);
        }

        // Remove from cur/ (prevents stuck-in-cur state)
        fs::remove_file(bus.join("cur").join(format!("{}.json", s.task_id))).ok();
        heartbeats.unregister(&s.task_id);
        log_event(
            bus,
            "timeout",
            &format!(",\"task\":\"{}\",\"age\":{}", s.task_id, s.age_s),
        );

        // Auto-capture timeout to knowledge store
        crate::recall::capture_from_failure(
            bus,
            &s.task_id,
            "unknown",
            "Timeout",
            &format!("Task stale after {}s (limit: {}s)", s.age_s, s.timeout_s),
            &[],
        );
    }
}

fn process_retries(bus: &Path) {
    let runs_dir = bus.join("runs");
    let entries = match fs::read_dir(&runs_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let now = Utc::now().timestamp() as u64;

    for entry in entries.flatten() {
        let task_dir = entry.path();
        let meta_path = task_dir.join("retry-meta.json");
        let pending_path = task_dir.join("retry-pending.json");

        if !meta_path.exists() || !pending_path.exists() {
            continue;
        }

        // Read retry metadata
        let meta: serde_json::Value = match fs::read_to_string(&meta_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(v) => v,
            None => continue,
        };

        let requeue_at = meta.get("requeue_at").and_then(|v| v.as_u64()).unwrap_or(0);
        if now < requeue_at {
            continue; // not yet time
        }

        // Time to requeue
        let requeue_path = meta
            .get("requeue_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if requeue_path.is_empty() {
            continue;
        }

        let requeue_ok = fs::read_to_string(&pending_path)
            .ok()
            .and_then(|task_data| fs::write(requeue_path, task_data).ok())
            .is_some();

        if !requeue_ok {
            eprintln!("daemon: retry requeue failed, keeping retry files");
            continue;
        }

        // Cleanup retry files only after confirmed requeue
        fs::remove_file(&meta_path).ok();
        fs::remove_file(&pending_path).ok();

        let task_id = task_dir.file_name().unwrap_or_default().to_string_lossy();
        eprintln!("daemon: requeued {task_id} after backoff");
    }
}

/// Check active goals: mark completed steps, queue next ready steps, detect completion.
fn evaluate_goals(bus: &Path) {
    let goals = goal::list_goals(bus);

    for mut g in goals {
        if g.status != GoalStatus::Active {
            continue;
        }

        let mut changed = false;
        let mut status_changed = false;

        {
            let plan = match g.plan.as_mut() {
                Some(p) => p,
                None => continue,
            };

            // Sync queued/running steps from bus state.
            for step in &mut plan.steps {
                let Some(task_id) = step.task_id.as_ref() else {
                    continue;
                };
                if matches!(
                    step.status,
                    StepStatus::Done
                        | StepStatus::Blocked
                        | StepStatus::Failed
                        | StepStatus::Skipped
                ) {
                    continue;
                }

                let Some(next_status) = goal::task_step_status(bus, task_id) else {
                    continue;
                };
                if next_status == step.status {
                    continue;
                }

                if matches!(next_status, StepStatus::Done | StepStatus::Failed) {
                    g.spent_usd += task_receipt_cost(bus, task_id);
                }
                step.status = next_status;
                changed = true;
            }

            // Pending steps whose dependencies can no longer succeed should become blocked.
            let step_statuses = plan
                .steps
                .iter()
                .map(|step| (step.step, step.status.clone()))
                .collect();
            for step in &mut plan.steps {
                if step.status != StepStatus::Pending {
                    continue;
                }
                if goal::step_dependencies_blocked(step, &step_statuses) {
                    step.status = StepStatus::Blocked;
                    changed = true;
                }
            }
        }

        let should_queue_ready = g.plan.as_ref().is_some_and(|plan| {
            let step_statuses = plan
                .steps
                .iter()
                .map(|step| (step.step, step.status.clone()))
                .collect();
            plan.steps.iter().any(|step| {
                step.status == StepStatus::Pending
                    && goal::step_dependencies_met(step, &step_statuses)
            })
        });

        if should_queue_ready {
            match goal::queue_ready_steps(bus, &mut g) {
                Ok(queued) => {
                    if !queued.is_empty() {
                        changed = true;
                        eprintln!(
                            "daemon: goal {} queued {} step(s): {}",
                            g.id,
                            queued.len(),
                            queued.join(", ")
                        );
                    }
                }
                Err(e) => {
                    g.status = GoalStatus::Failed;
                    status_changed = true;
                    eprintln!("daemon: goal {} queue error: {}", g.id, e);
                    log_event(
                        bus,
                        "goal_failed",
                        &format!(",\"goal\":\"{}\",\"reason\":\"queue_error\"", g.id),
                    );
                }
            }
        }

        if let Some(ref plan) = g.plan {
            let all_done = plan
                .steps
                .iter()
                .all(|s| matches!(s.status, StepStatus::Done | StepStatus::Skipped));

            if all_done {
                if g.status != GoalStatus::Done {
                    g.status = GoalStatus::Done;
                    g.completed_at = Some(Utc::now());
                    status_changed = true;
                    eprintln!("daemon: goal {} completed", g.id);
                    log_event(bus, "goal_completed", &format!(",\"goal\":\"{}\"", g.id));
                }
            } else {
                let any_terminal_failure = plan
                    .steps
                    .iter()
                    .any(|s| matches!(s.status, StepStatus::Blocked | StepStatus::Failed));
                let any_inflight = plan
                    .steps
                    .iter()
                    .any(|s| matches!(s.status, StepStatus::Queued | StepStatus::Running));
                let any_pending = plan.steps.iter().any(|s| s.status == StepStatus::Pending);
                if any_terminal_failure
                    && !any_inflight
                    && !any_pending
                    && g.status != GoalStatus::Failed
                {
                    g.status = GoalStatus::Failed;
                    status_changed = true;
                    eprintln!("daemon: goal {} failed (blocked or failed steps)", g.id);
                    log_event(bus, "goal_failed", &format!(",\"goal\":\"{}\"", g.id));
                }
            }
        }

        if changed || status_changed {
            goal::save_goal(bus, &g).ok();
        }
    }
}

fn task_receipt_cost(bus: &Path, task_id: &str) -> f64 {
    let receipt_path = bus.join("done").join(task_id).join("receipt.json");
    fs::read_to_string(receipt_path)
        .ok()
        .and_then(|data| serde_json::from_str::<serde_json::Value>(&data).ok())
        .and_then(|value| value.get("cost_usd").and_then(|v| v.as_f64()))
        .unwrap_or(0.0)
}

// --- Helpers ---

fn log_event(bus: &Path, event: &str, extra: &str) {
    let line = format!(
        "{{\"event\":\"{event}\",\"ts\":\"{}\"{extra}}}\n",
        Utc::now().to_rfc3339()
    );
    let audit = bus.join("audit.jsonl");
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(audit)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()))
        .ok();
}

fn save_run(bus: &Path, run: &Run) {
    let runs_dir = bus.join("runs").join(&run.task_id);
    fs::create_dir_all(&runs_dir).ok();
    let path = runs_dir.join(format!("run-{}.json", run.attempt));
    if let Ok(json) = serde_json::to_string_pretty(run) {
        fs::write(path, json).ok();
    }
}

fn count_attempts(bus: &Path, task_id: &str) -> u32 {
    let runs_dir = bus.join("runs").join(task_id);
    fs::read_dir(runs_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.path()
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("run-"))
        })
        .count() as u32
}

fn write_receipt(staging_dir: &Path, receipt: &Receipt) {
    let path = staging_dir.join("receipt.json");
    if let Ok(json) = serde_json::to_string_pretty(receipt) {
        fs::write(path, json).ok();
    }
}

fn append_receipt_index(bus: &Path, receipt: &Receipt) {
    let receipts_dir = bus.parent().unwrap_or(bus).join("receipts");
    fs::create_dir_all(&receipts_dir).ok();
    let index = receipts_dir.join("index.jsonl");
    if let Ok(json) = serde_json::to_string(receipt) {
        let line = format!("{json}\n");
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(index)
            .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()))
            .ok();
    }
}

fn move_staging(bus: &Path, staging_dir: &Path, task_id: &str, dest: &str) {
    let dest_dir = bus.join(dest).join(task_id);
    fs::create_dir_all(bus.join(dest)).ok();
    fs::rename(staging_dir, &dest_dir).ok();
}

fn extract_cost(stdout_path: &Path) -> f64 {
    fs::read_to_string(stdout_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| {
            v.get("cost_usd")
                .or_else(|| v.get("result").and_then(|r| r.get("cost_usd")))
                .and_then(|c| c.as_f64())
        })
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goal::{self, Goal, Plan, Step};
    use tempfile::TempDir;

    fn active_goal(steps: Vec<Step>) -> Goal {
        Goal {
            id: "goal-stage3".into(),
            project: "test".into(),
            objective: "stabilize".into(),
            deadline: None,
            budget_usd: 5.0,
            spent_usd: 0.0,
            status: GoalStatus::Active,
            plan: Some(Plan {
                version: 1,
                created_by: "test".into(),
                approved_at: None,
                steps,
            }),
            created_at: Utc::now(),
            completed_at: None,
        }
    }

    fn write_task_json(path: &Path) {
        let task = serde_json::json!({
            "project": "test",
            "model": "claude",
            "category": "fix"
        });
        fs::write(path, serde_json::to_string_pretty(&task).unwrap()).unwrap();
    }

    #[test]
    fn evaluate_goals_queues_ready_pending_steps_without_receipt_change() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(bus.join("new/p1")).unwrap();

        let goal = active_goal(vec![
            Step {
                step: 1,
                category: "research".into(),
                prompt: "done".into(),
                agent: "claude-sonnet".into(),
                est_cost_usd: 0.1,
                depends_on: vec![],
                status: StepStatus::Done,
                task_id: Some("goal-stage3-step1".into()),
                sub_tasks: vec![],
            },
            Step {
                step: 2,
                category: "fix".into(),
                prompt: "next".into(),
                agent: "claude-sonnet".into(),
                est_cost_usd: 0.1,
                depends_on: vec![1],
                status: StepStatus::Pending,
                task_id: None,
                sub_tasks: vec![],
            },
        ]);
        goal::save_goal(&bus, &goal).unwrap();

        evaluate_goals(&bus);

        let loaded = goal::load_goal(&bus, "goal-stage3").unwrap();
        let step = &loaded.plan.unwrap().steps[1];
        assert_eq!(step.status, StepStatus::Queued);
        assert_eq!(step.task_id.as_deref(), Some("goal-stage3-step2"));
        assert!(bus.join("new/p1/goal-stage3-step2.json").exists());
    }

    #[test]
    fn evaluate_goals_blocks_dependents_and_fails_goal_when_progress_is_impossible() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(bus.join("new/p1")).unwrap();

        let goal = active_goal(vec![
            Step {
                step: 1,
                category: "fix".into(),
                prompt: "failed".into(),
                agent: "claude-sonnet".into(),
                est_cost_usd: 0.1,
                depends_on: vec![],
                status: StepStatus::Failed,
                task_id: Some("goal-stage3-step1".into()),
                sub_tasks: vec![],
            },
            Step {
                step: 2,
                category: "review".into(),
                prompt: "blocked downstream".into(),
                agent: "claude-sonnet".into(),
                est_cost_usd: 0.1,
                depends_on: vec![1],
                status: StepStatus::Pending,
                task_id: None,
                sub_tasks: vec![],
            },
        ]);
        goal::save_goal(&bus, &goal).unwrap();

        evaluate_goals(&bus);

        let loaded = goal::load_goal(&bus, "goal-stage3").unwrap();
        assert_eq!(loaded.status, GoalStatus::Failed);
        let plan = loaded.plan.unwrap();
        assert_eq!(plan.steps[1].status, StepStatus::Blocked);
    }

    #[test]
    fn evaluate_goals_promotes_queued_steps_to_running_from_cur() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(bus.join("cur")).unwrap();
        write_task_json(&bus.join("cur/goal-stage3-step1.json"));

        let goal = active_goal(vec![Step {
            step: 1,
            category: "fix".into(),
            prompt: "running".into(),
            agent: "claude-sonnet".into(),
            est_cost_usd: 0.1,
            depends_on: vec![],
            status: StepStatus::Queued,
            task_id: Some("goal-stage3-step1".into()),
            sub_tasks: vec![],
        }]);
        goal::save_goal(&bus, &goal).unwrap();

        evaluate_goals(&bus);

        let loaded = goal::load_goal(&bus, "goal-stage3").unwrap();
        let plan = loaded.plan.unwrap();
        assert_eq!(plan.steps[0].status, StepStatus::Running);
        assert_eq!(loaded.status, GoalStatus::Active);
    }
}
