use std::fs;
use std::path::Path;

use chrono::{Duration, Utc};
use serde_json::Value;

/// Weekly performance metrics computed from receipts.
#[derive(Debug, Clone)]
pub struct WeeklyMetrics {
    pub period: String,
    pub total_tasks: u32,
    pub success_count: u32,
    pub failure_count: u32,
    pub timeout_count: u32,
    pub total_cost_usd: f64,
    pub avg_duration_ms: u64,
    pub success_rate_pct: f64,
}

/// Compute metrics for the last N days from receipts/index.jsonl.
pub fn compute_metrics(bus: &Path, days: i64) -> WeeklyMetrics {
    compute_metrics_window(bus, 0, days)
}

/// Compute metrics for a bounded window: from `start_days_ago` to `end_days_ago`.
pub fn compute_metrics_window(bus: &Path, start_days_ago: i64, end_days_ago: i64) -> WeeklyMetrics {
    let index = bus.parent().unwrap_or(bus).join("receipts/index.jsonl");
    let now = Utc::now();
    let cutoff_recent = (now - Duration::days(start_days_ago)).to_rfc3339();
    let cutoff_old = (now - Duration::days(end_days_ago)).to_rfc3339();
    let period = if start_days_ago == 0 {
        format!("last {}d", end_days_ago)
    } else {
        format!("{}d-{}d ago", start_days_ago, end_days_ago)
    };

    let content = match fs::read_to_string(index) {
        Ok(c) => c,
        Err(_) => {
            return WeeklyMetrics {
                period,
                total_tasks: 0,
                success_count: 0,
                failure_count: 0,
                timeout_count: 0,
                total_cost_usd: 0.0,
                avg_duration_ms: 0,
                success_rate_pct: 0.0,
            };
        }
    };

    let mut total = 0u32;
    let mut success = 0u32;
    let mut failure = 0u32;
    let mut timeout = 0u32;
    let mut cost = 0.0f64;
    let mut duration_sum = 0u64;

    for line in content.lines() {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let ts = v
            .get("created_at")
            .or_else(|| v.get("completed_at"))
            .and_then(|t| t.as_str())
            .unwrap_or("");

        if ts > cutoff_recent.as_str() || ts < cutoff_old.as_str() {
            continue;
        }

        total += 1;
        let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
        match status {
            "success" | "completed" => success += 1,
            "timeout" => timeout += 1,
            _ => failure += 1,
        }
        cost += v.get("cost_usd").and_then(|c| c.as_f64()).unwrap_or(0.0);
        duration_sum += v
            .get("duration_ms")
            .and_then(|d| d.as_u64())
            .or_else(|| {
                v.get("duration_seconds")
                    .and_then(|d| d.as_u64())
                    .map(|s| s * 1000)
            })
            .unwrap_or(0);
    }

    let success_rate = if total > 0 {
        success as f64 / total as f64 * 100.0
    } else {
        0.0
    };
    let avg_duration = if total > 0 {
        duration_sum / total as u64
    } else {
        0
    };

    WeeklyMetrics {
        period,
        total_tasks: total,
        success_count: success,
        failure_count: failure,
        timeout_count: timeout,
        total_cost_usd: cost,
        avg_duration_ms: avg_duration,
        success_rate_pct: success_rate,
    }
}

/// Compare two periods and generate directives.
pub fn compare(current: &WeeklyMetrics, previous: &WeeklyMetrics) -> Vec<String> {
    let mut directives = Vec::new();

    // Success rate degradation
    if previous.success_rate_pct > 0.0 && current.success_rate_pct < previous.success_rate_pct - 10.0
    {
        directives.push(format!(
            "WARN: success rate dropped {:.0}% -> {:.0}%",
            previous.success_rate_pct, current.success_rate_pct
        ));
    }

    // Cost increase
    if previous.total_cost_usd > 0.0
        && current.total_cost_usd > previous.total_cost_usd * 1.5
    {
        directives.push(format!(
            "WARN: cost increased ${:.2} -> ${:.2} (+{:.0}%)",
            previous.total_cost_usd,
            current.total_cost_usd,
            (current.total_cost_usd / previous.total_cost_usd - 1.0) * 100.0
        ));
    }

    // Duration increase
    if previous.avg_duration_ms > 0
        && current.avg_duration_ms > previous.avg_duration_ms * 2
    {
        directives.push(format!(
            "WARN: avg duration doubled {}ms -> {}ms",
            previous.avg_duration_ms, current.avg_duration_ms
        ));
    }

    // Improvement signals
    if current.success_rate_pct > previous.success_rate_pct + 5.0 && previous.total_tasks > 0 {
        directives.push(format!(
            "OK: success rate improved {:.0}% -> {:.0}%",
            previous.success_rate_pct, current.success_rate_pct
        ));
    }

    if current.total_cost_usd < previous.total_cost_usd * 0.8 && previous.total_tasks > 0 {
        directives.push(format!(
            "OK: cost reduced ${:.2} -> ${:.2}",
            previous.total_cost_usd, current.total_cost_usd
        ));
    }

    directives
}

/// Format metrics for display.
pub fn format_metrics(m: &WeeklyMetrics) -> String {
    format!(
        "{}: {} tasks ({} ok, {} fail, {} timeout), ${:.2}, avg {}s, {:.0}% success",
        m.period,
        m.total_tasks,
        m.success_count,
        m.failure_count,
        m.timeout_count,
        m.total_cost_usd,
        m.avg_duration_ms / 1000,
        m.success_rate_pct
    )
}
