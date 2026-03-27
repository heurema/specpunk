use std::fs;
use std::path::Path;

use chrono::{Duration, Utc};
use serde_json::Value;

use crate::bus;
use crate::config;

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

    // Config summary
    if let Ok(cfg) = config::load(config_dir) {
        let active_projects: Vec<_> = cfg
            .projects
            .projects
            .iter()
            .filter(|p| p.active)
            .collect();

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
    }

    out
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
