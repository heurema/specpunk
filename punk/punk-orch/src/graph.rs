use std::collections::HashMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

/// Generate a cost-per-day ASCII bar chart from receipts.
pub fn cost_chart(bus: &Path, days: i64) -> String {
    let index = bus.parent().unwrap_or(bus).join("receipts/index.jsonl");
    let cutoff = Utc::now() - Duration::days(days);

    let content = match fs::read_to_string(index) {
        Ok(c) => c,
        Err(_) => return "No receipt data.\n".to_string(),
    };

    let mut daily_cost: HashMap<String, f64> = HashMap::new();

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

        if ts.len() < 10 {
            continue;
        }

        let day = &ts[..10]; // YYYY-MM-DD
        if let Ok(d) = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d") {
            if d >= cutoff.date_naive() {
                let cost = v.get("cost_usd").and_then(|c| c.as_f64()).unwrap_or(0.0);
                *daily_cost.entry(day.to_string()).or_default() += cost;
            }
        }
    }

    if daily_cost.is_empty() {
        return format!("No receipts in last {days} days.\n");
    }

    let mut days_sorted: Vec<_> = daily_cost.iter().collect();
    days_sorted.sort_by_key(|(k, _)| (*k).clone());

    let max_cost = daily_cost.values().cloned().fold(0.0f64, f64::max);
    let bar_width = 40;

    let mut out = format!("Cost/day (last {days}d)\n\n");
    for (day, cost) in &days_sorted {
        let bar_len = if max_cost > 0.0 {
            (*cost / max_cost * bar_width as f64) as usize
        } else {
            0
        };
        let bar: String = "#".repeat(bar_len);
        out.push_str(&format!(
            "  {} {:>6} {}\n",
            &day[5..],
            format_usd(**cost),
            bar
        ));
    }

    let total: f64 = daily_cost.values().sum();
    out.push_str(&format!("\n  Total: ${total:.2}\n"));
    out
}

/// Generate task count per project.
pub fn project_chart(bus: &Path, days: i64) -> String {
    let index = bus.parent().unwrap_or(bus).join("receipts/index.jsonl");
    let cutoff = (Utc::now() - Duration::days(days)).to_rfc3339();

    let content = match fs::read_to_string(index) {
        Ok(c) => c,
        Err(_) => return "No receipt data.\n".to_string(),
    };

    let mut project_counts: HashMap<String, u32> = HashMap::new();

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

        if ts < cutoff.as_str() {
            continue;
        }

        let project = v
            .get("project")
            .and_then(|p| p.as_str())
            .unwrap_or("unknown");
        *project_counts.entry(project.to_string()).or_default() += 1;
    }

    if project_counts.is_empty() {
        return format!("No receipts in last {days} days.\n");
    }

    let mut sorted: Vec<_> = project_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    let max_count = sorted.first().map(|(_, c)| *c).unwrap_or(1);
    let bar_width = 40;

    let mut out = format!("Tasks/project (last {days}d)\n\n");
    for (project, count) in &sorted {
        let bar_len = (*count as f64 / max_count as f64 * bar_width as f64) as usize;
        let bar: String = "#".repeat(bar_len);
        out.push_str(&format!("  {:<15} {:>4} {}\n", project, count, bar));
    }

    out
}

fn format_usd(usd: f64) -> String {
    if usd < 0.01 {
        "$0".to_string()
    } else {
        format!("${:.2}", usd)
    }
}

pub fn gantt_chart(bus: &Path, days: i64) -> String {
    let index = bus.parent().unwrap_or(bus).join("receipts/index.jsonl");
    let cutoff = Utc::now() - Duration::days(days);

    let content = match fs::read_to_string(index) {
        Ok(c) => c,
        Err(_) => return "No receipt data.\n".to_string(),
    };

    let mut spans = Vec::new();
    for line in content.lines() {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(span) = parse_receipt_span(&v) else {
            continue;
        };
        if span.end < cutoff {
            continue;
        }
        spans.push(span);
    }

    if spans.is_empty() {
        return format!("No receipts in last {days} days.\n");
    }

    spans.sort_by_key(|span| span.start);
    if spans.len() > 10 {
        let start = spans.len() - 10;
        spans = spans[start..].to_vec();
    }

    let window_start = spans.iter().map(|span| span.start).min().unwrap();
    let window_end = spans.iter().map(|span| span.end).max().unwrap();
    let total_ms = (window_end - window_start).num_milliseconds().max(1) as f64;
    let width = 40usize;

    let mut out = format!("Run timeline (last {days}d)\n\n");
    for span in &spans {
        let offset_ms = (span.start - window_start).num_milliseconds().max(0) as f64;
        let duration_ms = (span.end - span.start).num_milliseconds().max(1) as f64;
        let offset = ((offset_ms / total_ms) * width as f64).floor() as usize;
        let bar_len = ((duration_ms / total_ms) * width as f64).ceil() as usize;
        let marker = match span.status.as_str() {
            "success" | "completed" => '#',
            "timeout" => '!',
            _ => 'x',
        };

        let mut row = vec![' '; width];
        let start_idx = offset.min(width.saturating_sub(1));
        let end_idx = (start_idx + bar_len.max(1)).min(width);
        for slot in &mut row[start_idx..end_idx] {
            *slot = marker;
        }
        let bar: String = row.into_iter().collect();

        out.push_str(&format!(
            "  {:<12} {} {:<9} {:>5}s {}\n",
            trim_label(&span.label, 12),
            bar,
            span.status,
            ((span.end - span.start).num_seconds()).max(0),
            span.start.format("%m-%d %H:%M")
        ));
    }

    out.push_str(&format!(
        "\n  Window: {} -> {}\n",
        window_start.format("%m-%d %H:%M"),
        window_end.format("%m-%d %H:%M")
    ));
    out
}

#[derive(Clone)]
struct ReceiptSpan {
    label: String,
    status: String,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

fn parse_receipt_span(v: &Value) -> Option<ReceiptSpan> {
    let created_at = v
        .get("created_at")
        .and_then(|value| value.as_str())
        .and_then(parse_ts)?;

    let completed_at = v
        .get("completed_at")
        .and_then(|value| value.as_str())
        .and_then(parse_ts)
        .or_else(|| {
            v.get("duration_ms")
                .and_then(|value| value.as_u64())
                .map(|duration_ms| created_at + Duration::milliseconds(duration_ms as i64))
        })
        .or_else(|| {
            v.get("duration_seconds")
                .and_then(|value| value.as_u64())
                .map(|duration_s| created_at + Duration::seconds(duration_s as i64))
        })
        .unwrap_or(created_at);

    let project = v.get("project").and_then(|value| value.as_str()).unwrap_or("unknown");
    let task_id = v.get("task_id").and_then(|value| value.as_str()).unwrap_or("task");
    let label = format!("{project}:{task_id}");
    let status = v
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string();

    Some(ReceiptSpan {
        label,
        status,
        start: created_at,
        end: completed_at.max(created_at),
    })
}

fn parse_ts(raw: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn trim_label(label: &str, max_len: usize) -> String {
    if label.chars().count() <= max_len {
        return label.to_string();
    }
    let keep = max_len.saturating_sub(1);
    let mut trimmed = label.chars().take(keep).collect::<String>();
    trimmed.push('…');
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn gantt_chart_reports_empty_window() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();
        let receipts = tmp.path().join("receipts");
        fs::create_dir_all(&receipts).unwrap();
        fs::write(receipts.join("index.jsonl"), "").unwrap();

        let out = gantt_chart(&bus, 7);
        assert!(out.contains("No receipts in last 7 days."));
    }

    #[test]
    fn gantt_chart_renders_recent_runs() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();
        let receipts = tmp.path().join("receipts");
        fs::create_dir_all(&receipts).unwrap();

        let now = Utc::now();
        let lines = [serde_json::json!({
                "project": "punk",
                "task_id": "task-a",
                "status": "success",
                "created_at": (now - Duration::minutes(30)).to_rfc3339(),
                "completed_at": (now - Duration::minutes(20)).to_rfc3339(),
                "duration_ms": 600_000u64
            })
            .to_string(),
            serde_json::json!({
                "project": "punk",
                "task_id": "task-b",
                "status": "timeout",
                "created_at": (now - Duration::minutes(15)).to_rfc3339(),
                "completed_at": (now - Duration::minutes(5)).to_rfc3339(),
                "duration_ms": 600_000u64
            })
            .to_string()];
        fs::write(receipts.join("index.jsonl"), lines.join("\n") + "\n").unwrap();

        let out = gantt_chart(&bus, 7);
        assert!(out.contains("Run timeline"));
        assert!(out.contains("punk:task-a"));
        assert!(out.contains("success"));
        assert!(out.contains("timeout"));
        assert!(out.contains("Window:"));
    }
}
