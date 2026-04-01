use std::fs;
use std::path::Path;

use chrono::{Duration, Utc};
use serde_json::Value;

use crate::benchmark::BenchmarkSummary;
use crate::eval::EvalSummary;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectiveLevel {
    Ok,
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RatchetVerdict {
    Improving,
    Stable,
    Degrading,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RatchetDirective {
    pub level: DirectiveLevel,
    pub metric: &'static str,
    pub message: String,
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
pub fn compare(current: &WeeklyMetrics, previous: &WeeklyMetrics) -> Vec<RatchetDirective> {
    let mut directives = Vec::new();

    // Success rate degradation
    if previous.success_rate_pct > 0.0
        && current.success_rate_pct < previous.success_rate_pct - 10.0
    {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Warn,
            metric: "success_rate",
            message: format!(
                "success rate dropped {:.0}% -> {:.0}%",
                previous.success_rate_pct, current.success_rate_pct
            ),
        });
    }

    // Cost increase
    if previous.total_cost_usd > 0.0 && current.total_cost_usd > previous.total_cost_usd * 1.5 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Warn,
            metric: "cost",
            message: format!(
                "cost increased ${:.2} -> ${:.2} (+{:.0}%)",
                previous.total_cost_usd,
                current.total_cost_usd,
                (current.total_cost_usd / previous.total_cost_usd - 1.0) * 100.0
            ),
        });
    }

    // Duration increase
    if previous.avg_duration_ms > 0 && current.avg_duration_ms > previous.avg_duration_ms * 2 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Warn,
            metric: "duration",
            message: format!(
                "avg duration doubled {}ms -> {}ms",
                previous.avg_duration_ms, current.avg_duration_ms
            ),
        });
    }

    if previous.total_tasks > 0 && current.timeout_count > previous.timeout_count + 1 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Warn,
            metric: "timeouts",
            message: format!(
                "timeouts increased {} -> {}",
                previous.timeout_count, current.timeout_count
            ),
        });
    }

    // Improvement signals
    if current.success_rate_pct > previous.success_rate_pct + 5.0 && previous.total_tasks > 0 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Ok,
            metric: "success_rate",
            message: format!(
                "success rate improved {:.0}% -> {:.0}%",
                previous.success_rate_pct, current.success_rate_pct
            ),
        });
    }

    if current.total_cost_usd < previous.total_cost_usd * 0.8 && previous.total_tasks > 0 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Ok,
            metric: "cost",
            message: format!(
                "cost reduced ${:.2} -> ${:.2}",
                previous.total_cost_usd, current.total_cost_usd
            ),
        });
    }

    directives
}

pub fn verdict(directives: &[RatchetDirective]) -> RatchetVerdict {
    if directives
        .iter()
        .any(|directive| directive.level == DirectiveLevel::Warn)
    {
        RatchetVerdict::Degrading
    } else if directives
        .iter()
        .any(|directive| directive.level == DirectiveLevel::Ok)
    {
        RatchetVerdict::Improving
    } else {
        RatchetVerdict::Stable
    }
}

pub fn eval_directives(summary: &EvalSummary) -> Vec<RatchetDirective> {
    let mut directives = Vec::new();

    if summary.avg_score < 0.7 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Warn,
            metric: "eval_score",
            message: format!("avg eval score is low at {:.2}", summary.avg_score),
        });
    } else if summary.avg_score >= 0.9 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Ok,
            metric: "eval_score",
            message: format!("avg eval score is strong at {:.2}", summary.avg_score),
        });
    }

    if summary.avg_scope_discipline < 0.95 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Warn,
            metric: "scope_discipline",
            message: format!(
                "scope discipline regressed to {:.2}",
                summary.avg_scope_discipline
            ),
        });
    }

    if summary.avg_integrity_pass_rate < 0.99 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Warn,
            metric: "integrity",
            message: format!(
                "integrity pass rate regressed to {:.2}",
                summary.avg_integrity_pass_rate
            ),
        });
    }

    if summary.avg_docs_parity < 0.95 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Warn,
            metric: "docs_parity",
            message: format!("docs parity regressed to {:.2}", summary.avg_docs_parity),
        });
    }

    if summary.avg_drift_penalty > 0.2 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Warn,
            metric: "drift",
            message: format!(
                "drift penalty is elevated at {:.2}",
                summary.avg_drift_penalty
            ),
        });
    } else if summary.avg_drift_penalty == 0.0 && summary.avg_scope_discipline >= 0.99 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Ok,
            metric: "drift",
            message: "no drift penalty detected in recent evals".to_string(),
        });
    }

    directives
}

pub fn benchmark_directives(summary: &BenchmarkSummary) -> Vec<RatchetDirective> {
    let mut directives = Vec::new();

    if summary.avg_score < 0.7 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Warn,
            metric: "benchmark_score",
            message: format!("avg benchmark score is low at {:.2}", summary.avg_score),
        });
    } else if summary.avg_score >= 0.9 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Ok,
            metric: "benchmark_score",
            message: format!("avg benchmark score is strong at {:.2}", summary.avg_score),
        });
    }

    if summary.fail_count > 0 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Warn,
            metric: "benchmark_failures",
            message: format!("{} benchmark results failed", summary.fail_count),
        });
    }

    if summary.flaky_count > 0 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Warn,
            metric: "benchmark_flaky",
            message: format!("{} benchmark results are flaky", summary.flaky_count),
        });
    }

    if summary.pass_count == summary.total && summary.total > 0 && summary.avg_score >= 0.9 {
        directives.push(RatchetDirective {
            level: DirectiveLevel::Ok,
            metric: "benchmark_pass_rate",
            message: format!("all {} benchmark results passed cleanly", summary.total),
        });
    }

    directives
}

pub fn format_directive(directive: &RatchetDirective) -> String {
    let prefix = match directive.level {
        DirectiveLevel::Ok => "OK",
        DirectiveLevel::Warn => "WARN",
    };
    format!("{prefix} [{}] {}", directive.metric, directive.message)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(
        total_tasks: u32,
        success_count: u32,
        failure_count: u32,
        timeout_count: u32,
        total_cost_usd: f64,
        avg_duration_ms: u64,
        success_rate_pct: f64,
    ) -> WeeklyMetrics {
        WeeklyMetrics {
            period: "test".into(),
            total_tasks,
            success_count,
            failure_count,
            timeout_count,
            total_cost_usd,
            avg_duration_ms,
            success_rate_pct,
        }
    }

    #[test]
    fn compare_emits_warn_directives_for_clear_regressions() {
        let current = metrics(10, 4, 4, 2, 30.0, 6_000, 40.0);
        let previous = metrics(10, 8, 2, 0, 10.0, 2_000, 80.0);

        let directives = compare(&current, &previous);
        assert!(directives.iter().any(|d| d.metric == "success_rate"));
        assert!(directives.iter().any(|d| d.metric == "cost"));
        assert!(directives.iter().any(|d| d.metric == "duration"));
        assert!(directives.iter().any(|d| d.metric == "timeouts"));
        assert_eq!(verdict(&directives), RatchetVerdict::Degrading);
    }

    #[test]
    fn compare_emits_ok_directives_for_clear_improvements() {
        let current = metrics(10, 9, 1, 0, 8.0, 1_000, 90.0);
        let previous = metrics(10, 7, 3, 1, 12.0, 2_000, 70.0);

        let directives = compare(&current, &previous);
        assert!(directives.iter().any(|d| d.level == DirectiveLevel::Ok));
        assert_eq!(verdict(&directives), RatchetVerdict::Improving);
    }

    fn sample_eval_summary(
        avg_score: f64,
        avg_scope_discipline: f64,
        avg_integrity_pass_rate: f64,
        avg_docs_parity: f64,
        avg_drift_penalty: f64,
    ) -> EvalSummary {
        EvalSummary {
            total: 3,
            accept_count: 2,
            reject_count: 1,
            avg_score,
            avg_contract_satisfaction: 0.8,
            avg_scope_discipline,
            avg_target_pass_rate: 0.7,
            avg_integrity_pass_rate,
            avg_cleanup_completion: 0.8,
            avg_docs_parity,
            avg_drift_penalty,
            projects: vec![],
            weakest_tasks: vec![],
        }
    }

    #[test]
    fn eval_directives_warn_on_low_eval_health() {
        let summary = sample_eval_summary(0.55, 0.7, 0.8, 0.6, 0.5);

        let directives = eval_directives(&summary);
        assert!(directives.iter().any(|d| d.metric == "eval_score"));
        assert!(directives.iter().any(|d| d.metric == "scope_discipline"));
        assert!(directives.iter().any(|d| d.metric == "integrity"));
        assert!(directives.iter().any(|d| d.metric == "docs_parity"));
        assert!(directives.iter().any(|d| d.metric == "drift"));
        assert_eq!(verdict(&directives), RatchetVerdict::Degrading);
    }

    #[test]
    fn eval_directives_emit_ok_for_clean_recent_evals() {
        let summary = sample_eval_summary(0.95, 1.0, 1.0, 1.0, 0.0);

        let directives = eval_directives(&summary);
        assert!(directives
            .iter()
            .any(|d| d.metric == "eval_score" && d.level == DirectiveLevel::Ok));
        assert!(directives
            .iter()
            .any(|d| d.metric == "drift" && d.level == DirectiveLevel::Ok));
        assert_eq!(verdict(&directives), RatchetVerdict::Improving);
    }

    fn sample_benchmark_summary(
        total: usize,
        pass_count: usize,
        fail_count: usize,
        flaky_count: usize,
        avg_score: f64,
    ) -> BenchmarkSummary {
        BenchmarkSummary {
            total,
            pass_count,
            fail_count,
            flaky_count,
            avg_score,
            projects: vec![],
            suites: vec![],
            weakest: vec![],
        }
    }

    #[test]
    fn benchmark_directives_warn_on_failures_and_flakes() {
        let summary = sample_benchmark_summary(4, 1, 2, 1, 0.55);

        let directives = benchmark_directives(&summary);
        assert!(directives.iter().any(|d| d.metric == "benchmark_score"));
        assert!(directives.iter().any(|d| d.metric == "benchmark_failures"));
        assert!(directives.iter().any(|d| d.metric == "benchmark_flaky"));
        assert_eq!(verdict(&directives), RatchetVerdict::Degrading);
    }

    #[test]
    fn benchmark_directives_emit_ok_for_clean_high_scores() {
        let summary = sample_benchmark_summary(3, 3, 0, 0, 0.94);

        let directives = benchmark_directives(&summary);
        assert!(directives
            .iter()
            .any(|d| d.metric == "benchmark_score" && d.level == DirectiveLevel::Ok));
        assert!(directives
            .iter()
            .any(|d| d.metric == "benchmark_pass_rate" && d.level == DirectiveLevel::Ok));
        assert_eq!(verdict(&directives), RatchetVerdict::Improving);
    }
}
