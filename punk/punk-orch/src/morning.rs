use std::fs;
use std::path::Path;

use chrono::{Duration, Utc};
use serde_json::Value;

use crate::bus;
use crate::config;
use crate::goal;

/// Generate a morning briefing from current state.
pub fn briefing(bus_path: &Path, config_dir: &Path) -> String {
    let mut out = String::new();
    let now = Utc::now();

    out.push_str(&format!(
        "## punk morning -- {}\n\n",
        now.format("%Y-%m-%d %H:%M")
    ));

    // Receipt stats (last 24h from index.jsonl)
    let receipts_path = bus_path
        .parent()
        .unwrap_or(bus_path)
        .join("receipts/index.jsonl");
    let (total_24h, success_24h, failed_24h, cost_24h) = receipt_stats_24h(&receipts_path);
    let (total_7d, success_7d, failed_7d, cost_7d) = receipt_stats_window(&receipts_path, 7);

    out.push_str("### Activity\n");
    out.push_str(&format!(
        "  24h: {} tasks ({} ok, {} fail), ${:.2}\n",
        total_24h, success_24h, failed_24h, cost_24h
    ));
    out.push_str(&format!(
        "   7d: {} tasks ({} ok, {} fail), ${:.2}\n\n",
        total_7d, success_7d, failed_7d, cost_7d
    ));

    // Current state
    let state = bus::read_state(bus_path, 5);

    if !state.running.is_empty() {
        out.push_str(&format!("### Running ({})\n", state.running.len()));
        for t in &state.running {
            out.push_str(&format!("  {} ({}, {})\n", t.id, t.project, t.model));
        }
        out.push('\n');
    }

    if !state.queued.is_empty() {
        out.push_str(&format!("### Queued ({})\n", state.queued.len()));
        for t in &state.queued {
            out.push_str(&format!(
                "  {} ({}, {}, {})\n",
                t.id, t.project, t.model, t.priority
            ));
        }
        out.push('\n');
    }

    // Failed + dead letter
    let dead_count = count_dir_entries(&bus_path.join("dead"));
    if !state.failed.is_empty() || dead_count > 0 {
        out.push_str("### Attention\n");
        if !state.failed.is_empty() {
            out.push_str(&format!(
                "  {} failed task(s) pending triage\n",
                state.failed.len()
            ));
            for t in &state.failed {
                out.push_str(&format!("    {} ({}, {})\n", t.id, t.project, t.model));
            }
        }
        if dead_count > 0 {
            out.push_str(&format!(
                "  {} dead-letter task(s) (retries exhausted)\n",
                dead_count
            ));
        }
        out.push('\n');
    }

    let state_dir = bus_path.join("state");
    let replan_needed = count_replan_needed_goals(bus_path);
    let goals = load_goals_summary(&state_dir).map(|mut goals| {
        goals.replan_needed = replan_needed;
        goals
    });
    let goals = goals.or_else(|| {
        if replan_needed > 0 {
            Some(GoalsSummary {
                replan_needed,
                ..GoalsSummary::default()
            })
        } else {
            None
        }
    });
    if let Some(goals) = goals.as_ref() {
        out.push_str("### Goals\n");
        out.push_str(&format!(
            "  {} active, {} blocked, {} done, {} replan-needed\n",
            goals.active, goals.blocked, goals.done, goals.replan_needed
        ));
        if !goals.focus.is_empty() {
            out.push_str(&format!("  focus: {}\n", goals.focus.join(", ")));
        }
        out.push('\n');
    }

    let pipeline = load_pipeline_summary(&state_dir, now);
    if let Some(pipeline) = pipeline.as_ref() {
        out.push_str("### Pipeline\n");
        out.push_str(&format!(
            "  {} active, {} awaiting approval, {} stale\n",
            pipeline.active, pipeline.awaiting_approval, pipeline.stale
        ));
        if !pipeline.focus.is_empty() {
            out.push_str(&format!("  focus: {}\n", pipeline.focus.join(", ")));
        }
        out.push('\n');
    }

    let directives = build_directives(
        state.failed.len(),
        dead_count,
        state.queued.len(),
        replan_needed,
        pipeline.as_ref(),
    );
    if !directives.is_empty() {
        out.push_str("### Directives\n");
        for directive in directives {
            out.push_str(&format!("  - {}\n", directive));
        }
        out.push('\n');
    }

    // Config summary (always works, uses defaults if no TOML)
    let cfg = match config::load_or_default(config_dir) {
        Ok(cfg) => cfg,
        Err(e) => {
            out.push_str("### Config Error\n");
            out.push_str(&format!("  {e}\n\n"));
            return out;
        }
    };
    let active_projects: Vec<_> = cfg.projects.projects.iter().filter(|p| p.active).collect();

    // Checkpoints coming up
    let upcoming: Vec<_> = active_projects
        .iter()
        .filter(|p| !p.checkpoint.is_empty())
        .filter_map(|p| {
            chrono::NaiveDate::parse_from_str(&p.checkpoint, "%Y-%m-%d")
                .ok()
                .map(|d| (&p.id, d))
        })
        .filter(|(_, d)| {
            let days = (*d - now.date_naive()).num_days();
            (0..=14).contains(&days)
        })
        .collect();

    if !upcoming.is_empty() {
        out.push_str("### Upcoming Checkpoints\n");
        for (id, date) in &upcoming {
            let days = (*date - now.date_naive()).num_days();
            out.push_str(&format!("  {} -- {} ({} days)\n", id, date, days));
        }
        out.push('\n');
    }

    // Budget
    let budget_ceiling = cfg.policy.budget.monthly_ceiling_usd;
    if budget_ceiling > 0.0 {
        let pct = (cost_7d / budget_ceiling * 100.0).min(999.0);
        out.push_str(&format!(
            "### Budget\n  ${:.2} / ${:.0} monthly (est {:.0}% at 7d rate)\n\n",
            cost_7d, budget_ceiling, pct
        ));
    }

    out
}

#[derive(Debug, Default)]
struct GoalsSummary {
    active: usize,
    blocked: usize,
    done: usize,
    replan_needed: usize,
    focus: Vec<String>,
}

#[derive(Debug, Default)]
struct PipelineSummary {
    active: usize,
    awaiting_approval: usize,
    stale: usize,
    focus: Vec<String>,
}

fn load_goals_summary(state_dir: &Path) -> Option<GoalsSummary> {
    let value = read_state_value(state_dir, &["goals.json", "goal.json"])?;
    let mut summary = GoalsSummary::default();

    for item in collect_items(&value, &["goals", "items", "entries"]) {
        let status = item_status(item);
        if is_done_status(&status) {
            summary.done += 1;
            continue;
        }

        if is_blocked_status(&status) {
            summary.blocked += 1;
            push_focus(&mut summary.focus, item_label(item));
            continue;
        }

        summary.active += 1;
        push_focus(&mut summary.focus, item_label(item));
    }

    if summary.active == 0 && summary.blocked == 0 && summary.done == 0 {
        None
    } else {
        Some(summary)
    }
}

fn load_pipeline_summary(state_dir: &Path, now: chrono::DateTime<Utc>) -> Option<PipelineSummary> {
    let value = read_state_value(state_dir, &["pipeline.json", "pipelines.json"])?;
    let mut summary = PipelineSummary::default();

    for item in collect_items(&value, &["pipelines", "items", "runs", "entries"]) {
        let status = item_status(item);
        if is_terminal_pipeline_status(&status) {
            continue;
        }

        let label = item_label(item);
        summary.active += 1;

        if is_awaiting_approval_status(&status) {
            summary.awaiting_approval += 1;
            push_focus(&mut summary.focus, label.clone());
        }

        if item_is_stale(item, now) {
            summary.stale += 1;
            push_focus(&mut summary.focus, label);
        }
    }

    if summary.active == 0 && summary.awaiting_approval == 0 && summary.stale == 0 {
        None
    } else {
        Some(summary)
    }
}

fn count_replan_needed_goals(bus_path: &Path) -> usize {
    goal::list_goals(bus_path)
        .into_iter()
        .filter(|goal| goal.status_reason.as_deref() == Some("replan_needed_dead_end"))
        .count()
}

fn build_directives(
    failed_count: usize,
    dead_count: usize,
    queued_count: usize,
    replan_needed: usize,
    pipeline: Option<&PipelineSummary>,
) -> Vec<String> {
    let mut directives = Vec::new();

    if failed_count > 0 || dead_count > 0 {
        directives.push(format!(
            "triage failed/dead-letter tasks first ({} failed, {} dead)",
            failed_count, dead_count
        ));
    }

    if replan_needed > 0 {
        directives.push(format!("replan dead-end goals ({replan_needed})"));
    }

    if let Some(pipeline) = pipeline {
        if pipeline.awaiting_approval > 0 {
            directives.push(format!(
                "review approval queue ({} awaiting approval)",
                pipeline.awaiting_approval
            ));
        }
        if pipeline.stale > 0 {
            directives.push(format!(
                "follow up on stale pipeline items ({} stale)",
                pipeline.stale
            ));
        }
    }

    if directives.is_empty() && queued_count > 0 {
        directives.push(format!(
            "drain queued work by priority ({} queued)",
            queued_count
        ));
    }

    directives
}

fn read_state_value(state_dir: &Path, file_names: &[&str]) -> Option<Value> {
    file_names.iter().find_map(|name| {
        let path = state_dir.join(name);
        let raw = fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    })
}

fn collect_items<'a>(value: &'a Value, array_keys: &[&str]) -> Vec<&'a Value> {
    if let Some(items) = value.as_array() {
        return items.iter().collect();
    }

    let Some(obj) = value.as_object() else {
        return Vec::new();
    };

    for key in array_keys {
        if let Some(items) = obj.get(*key).and_then(Value::as_array) {
            return items.iter().collect();
        }
        if let Some(items) = obj.get(*key).and_then(Value::as_object) {
            return items.values().collect();
        }
    }

    obj.values().filter(|value| value.is_object()).collect()
}

fn item_status(item: &Value) -> String {
    item.get("status")
        .or_else(|| item.get("state"))
        .or_else(|| item.get("phase"))
        .and_then(Value::as_str)
        .unwrap_or("active")
        .to_ascii_lowercase()
}

fn item_label(item: &Value) -> String {
    item.get("title")
        .or_else(|| item.get("name"))
        .or_else(|| item.get("id"))
        .or_else(|| item.get("goal"))
        .or_else(|| item.get("pipeline"))
        .and_then(Value::as_str)
        .unwrap_or("unnamed")
        .to_string()
}

fn push_focus(focus: &mut Vec<String>, label: String) {
    if focus.len() >= 3 || focus.iter().any(|existing| existing == &label) {
        return;
    }
    focus.push(label);
}

fn is_done_status(status: &str) -> bool {
    matches!(
        status,
        "done" | "complete" | "completed" | "closed" | "resolved"
    )
}

fn is_blocked_status(status: &str) -> bool {
    status.contains("blocked")
}

fn is_terminal_pipeline_status(status: &str) -> bool {
    matches!(
        status,
        "done" | "complete" | "completed" | "failed" | "cancelled" | "canceled" | "aborted"
    )
}

fn is_awaiting_approval_status(status: &str) -> bool {
    matches!(
        status,
        "awaiting_approval" | "awaiting-approval" | "approval_needed" | "pending_approval"
    ) || (status.contains("approval") && (status.contains("await") || status.contains("pending")))
}

fn item_is_stale(item: &Value, now: chrono::DateTime<Utc>) -> bool {
    let timestamp = item
        .get("updated_at")
        .or_else(|| item.get("updatedAt"))
        .or_else(|| item.get("timestamp"))
        .or_else(|| item.get("ts"))
        .and_then(Value::as_str);

    let Some(timestamp) = timestamp else {
        return false;
    };

    let Ok(updated_at) = chrono::DateTime::parse_from_rfc3339(timestamp) else {
        return false;
    };

    now.signed_duration_since(updated_at.with_timezone(&Utc)) >= Duration::days(3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goal::{Goal, GoalStatus};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn briefing_includes_goal_and_pipeline_sections() {
        let root = test_root("sections");
        let bus_path = root.join("bus");
        let config_dir = root.join("config");
        let state_dir = bus_path.join("state");
        let receipts_dir = root.join("receipts");

        fs::create_dir_all(&state_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(&receipts_dir).unwrap();
        fs::create_dir_all(root.join("goals")).unwrap();
        fs::write(receipts_dir.join("index.jsonl"), "").unwrap();
        fs::write(
            state_dir.join("goals.json"),
            r#"{
                "goals": [
                    {"id": "g1", "title": "Ship council sync", "status": "active"},
                    {"id": "g2", "title": "Fix flaky gate", "status": "blocked"},
                    {"id": "g3", "title": "Close old audit", "status": "done"}
                ]
            }"#,
        )
        .unwrap();

        let stale = (Utc::now() - Duration::days(5)).to_rfc3339();
        fs::write(
            state_dir.join("pipeline.json"),
            format!(
                r#"{{
                    "pipelines": [
                        {{"id": "p1", "name": "prove/spec", "status": "running", "updated_at": "{}"}},
                        {{"id": "p2", "name": "approve/release", "status": "awaiting_approval", "updated_at": "{}"}}
                    ]
                }}"#,
                stale,
                Utc::now().to_rfc3339()
            ),
        )
        .unwrap();
        fs::write(
            root.join("goals/replan.json"),
            serde_json::to_string_pretty(&Goal {
                id: "replan".into(),
                project: "specpunk".into(),
                objective: "Recover dead-end".into(),
                deadline: None,
                budget_usd: 5.0,
                spent_usd: 1.0,
                status: GoalStatus::Failed,
                status_reason: Some("replan_needed_dead_end".into()),
                plan: None,
                created_at: Utc::now(),
                completed_at: None,
            })
            .unwrap(),
        )
        .unwrap();

        let output = briefing(&bus_path, &config_dir);

        assert!(output.contains("### Goals"));
        assert!(output.contains("1 active, 1 blocked, 1 done, 1 replan-needed"));
        assert!(output.contains("focus: Ship council sync, Fix flaky gate"));
        assert!(output.contains("### Pipeline"));
        assert!(output.contains("2 active, 1 awaiting approval, 1 stale"));
        assert!(output.contains("focus: prove/spec, approve/release"));

        cleanup(root);
    }

    #[test]
    fn briefing_prioritizes_triage_approval_and_stale_follow_up() {
        let root = test_root("directives");
        let bus_path = root.join("bus");
        let config_dir = root.join("config");
        let state_dir = bus_path.join("state");
        let dead_dir = bus_path.join("dead");
        let receipts_dir = root.join("receipts");

        fs::create_dir_all(&state_dir).unwrap();
        fs::create_dir_all(&dead_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(&receipts_dir).unwrap();
        fs::create_dir_all(root.join("goals")).unwrap();
        fs::write(receipts_dir.join("index.jsonl"), "").unwrap();
        fs::create_dir_all(dead_dir.join("task-1")).unwrap();
        fs::write(
            root.join("goals/replan.json"),
            serde_json::to_string_pretty(&Goal {
                id: "replan".into(),
                project: "specpunk".into(),
                objective: "Recover dead-end".into(),
                deadline: None,
                budget_usd: 5.0,
                spent_usd: 1.0,
                status: GoalStatus::Failed,
                status_reason: Some("replan_needed_dead_end".into()),
                plan: None,
                created_at: Utc::now(),
                completed_at: None,
            })
            .unwrap(),
        )
        .unwrap();

        let stale = (Utc::now() - Duration::days(4)).to_rfc3339();
        fs::write(
            state_dir.join("pipeline.json"),
            format!(
                r#"{{
                    "pipelines": [
                        {{"id": "p1", "name": "approval", "status": "awaiting_approval", "updated_at": "{}"}}
                    ]
                }}"#,
                stale
            ),
        )
        .unwrap();

        let output = briefing(&bus_path, &config_dir);
        let triage_idx = output
            .find("triage failed/dead-letter tasks first")
            .unwrap();
        let replan_idx = output.find("replan dead-end goals").unwrap();
        let approval_idx = output.find("review approval queue").unwrap();
        let stale_idx = output.find("follow up on stale pipeline items").unwrap();

        assert!(output.contains("### Directives"));
        assert!(triage_idx < replan_idx);
        assert!(replan_idx < approval_idx);
        assert!(approval_idx < stale_idx);

        cleanup(root);
    }

    fn test_root(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("punk-morning-{name}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn cleanup(path: PathBuf) {
        let _ = fs::remove_dir_all(path);
    }
}

fn receipt_stats_24h(index_path: &Path) -> (u32, u32, u32, f64) {
    receipt_stats_window(index_path, 1)
}

fn receipt_stats_window(index_path: &Path, days: i64) -> (u32, u32, u32, f64) {
    let cutoff = Utc::now() - Duration::days(days);
    let cutoff_str = cutoff.to_rfc3339();

    let content = match fs::read_to_string(index_path) {
        Ok(c) => c,
        Err(_) => return (0, 0, 0, 0.0),
    };

    let mut total = 0u32;
    let mut success = 0u32;
    let mut failed = 0u32;
    let mut cost = 0.0f64;

    for line in content.lines() {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Filter by time
        let ts = v
            .get("created_at")
            .or_else(|| v.get("completed_at"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if ts < cutoff_str.as_str() {
            continue;
        }

        total += 1;
        let status = v.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if status == "success" || status == "completed" {
            success += 1;
        } else {
            failed += 1;
        }
        cost += v.get("cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
    }

    (total, success, failed, cost)
}

fn count_dir_entries(dir: &Path) -> usize {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .count()
}
