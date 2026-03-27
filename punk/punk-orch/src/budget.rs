use std::fs;
use std::path::Path;

use serde_json::Value;

/// Budget pressure level based on monthly spend vs ceiling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PressureLevel {
    Normal,
    Soft,  // 80%+ — warn
    Hard,  // 90%+ — reduce slots to 2, p0+p1 only
    Stop,  // 95%+ — p0 only
}

/// Check current budget pressure.
pub fn check_pressure(
    bus: &Path,
    ceiling_usd: f64,
    soft_pct: u32,
    hard_pct: u32,
) -> (PressureLevel, f64) {
    if ceiling_usd <= 0.0 {
        return (PressureLevel::Normal, 0.0);
    }

    let spent = monthly_spend(bus);
    let pct = (spent / ceiling_usd * 100.0) as u32;

    let level = if pct >= 95 {
        PressureLevel::Stop
    } else if pct >= hard_pct {
        PressureLevel::Hard
    } else if pct >= soft_pct {
        PressureLevel::Soft
    } else {
        PressureLevel::Normal
    };

    (level, spent)
}

/// Should this task priority be allowed at the current pressure level?
pub fn priority_allowed(pressure: &PressureLevel, priority: &str) -> bool {
    match pressure {
        PressureLevel::Normal | PressureLevel::Soft => true,
        PressureLevel::Hard => priority == "p0" || priority == "p1",
        PressureLevel::Stop => priority == "p0",
    }
}

/// Max concurrent slots at the current pressure level.
pub fn effective_max_slots(pressure: &PressureLevel, configured_max: u32) -> u32 {
    match pressure {
        PressureLevel::Normal | PressureLevel::Soft => configured_max,
        PressureLevel::Hard => configured_max.min(2),
        PressureLevel::Stop => 1,
    }
}

/// Calculate total spend this month from receipts/index.jsonl.
fn monthly_spend(bus: &Path) -> f64 {
    let index = bus
        .parent()
        .unwrap_or(bus)
        .join("receipts/index.jsonl");

    let content = match fs::read_to_string(index) {
        Ok(c) => c,
        Err(_) => return 0.0,
    };

    // Current month prefix (e.g. "2026-03")
    let now = chrono::Utc::now();
    let month_prefix = now.format("%Y-%m").to_string();

    content
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|v| {
            v.get("created_at")
                .or_else(|| v.get("completed_at"))
                .and_then(|t| t.as_str())
                .is_some_and(|t| t.starts_with(&month_prefix))
        })
        .map(|v| v.get("cost_usd").and_then(|c| c.as_f64()).unwrap_or(0.0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_levels() {
        assert_eq!(check_pressure(Path::new("/nonexistent"), 50.0, 80, 95).0, PressureLevel::Normal);
        assert_eq!(check_pressure(Path::new("/nonexistent"), 0.0, 80, 95).0, PressureLevel::Normal);
    }

    #[test]
    fn priority_filter() {
        assert!(priority_allowed(&PressureLevel::Normal, "p2"));
        assert!(priority_allowed(&PressureLevel::Hard, "p0"));
        assert!(priority_allowed(&PressureLevel::Hard, "p1"));
        assert!(!priority_allowed(&PressureLevel::Hard, "p2"));
        assert!(priority_allowed(&PressureLevel::Stop, "p0"));
        assert!(!priority_allowed(&PressureLevel::Stop, "p1"));
    }

    #[test]
    fn effective_slots() {
        assert_eq!(effective_max_slots(&PressureLevel::Normal, 5), 5);
        assert_eq!(effective_max_slots(&PressureLevel::Hard, 5), 2);
        assert_eq!(effective_max_slots(&PressureLevel::Stop, 5), 1);
    }
}
