use std::collections::HashMap;
use std::fs;
use std::path::Path;

use chrono::{Duration, Utc};
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
