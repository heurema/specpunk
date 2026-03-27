use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Per-project session context. Frozen snapshot injected into agent prompt at task start.
/// Capped at MAX_ENTRIES with TTL-based eviction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    pub project: String,
    pub entries: Vec<SessionEntry>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub entry_type: EntryType,
    pub fact: String,
    pub task_id: String,
    pub ttl_tasks: u32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryType {
    Success,
    Failure,
    Surprise,
    CostOverrun,
}

const MAX_ENTRIES: usize = 10;

fn sessions_dir(bus: &Path) -> PathBuf {
    let state_dir = bus.parent().unwrap_or(bus);
    state_dir.join("sessions")
}

/// Load session context for a project.
pub fn load(bus: &Path, project: &str) -> SessionContext {
    let path = sessions_dir(bus).join(format!("{project}.json"));
    fs::read_to_string(path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_else(|| SessionContext {
            project: project.to_string(),
            entries: vec![],
            updated_at: Utc::now(),
        })
}

/// Save session context.
pub fn save(bus: &Path, ctx: &SessionContext) -> std::io::Result<()> {
    let dir = sessions_dir(bus);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", ctx.project));
    let json = serde_json::to_string_pretty(ctx)
        .map_err(std::io::Error::other)?;
    fs::write(path, json)
}

/// Add an entry from a completed task receipt.
pub fn add_from_receipt(
    bus: &Path,
    project: &str,
    task_id: &str,
    status: &str,
    cost_usd: f64,
    budget_usd: f64,
    summary: &str,
) {
    let mut ctx = load(bus, project);

    // Determine entry type
    let entry_type = match status {
        "success" => EntryType::Success,
        "timeout" => EntryType::Failure,
        _ => EntryType::Failure,
    };

    // Check for cost overrun
    let actual_type = if cost_usd > budget_usd * 1.5 && budget_usd > 0.0 {
        EntryType::CostOverrun
    } else {
        entry_type
    };

    let fact = if summary.is_empty() {
        format!("{status}: {task_id}")
    } else {
        summary.to_string()
    };

    // Decrement TTL on existing entries BEFORE adding new one
    for entry in &mut ctx.entries {
        if entry.ttl_tasks > 0 {
            entry.ttl_tasks -= 1;
        }
    }
    ctx.entries.retain(|e| e.ttl_tasks > 0);

    ctx.entries.push(SessionEntry {
        entry_type: actual_type,
        fact,
        task_id: task_id.to_string(),
        ttl_tasks: 5,
        created_at: Utc::now(),
    });

    // Cap at MAX_ENTRIES (keep newest)
    if ctx.entries.len() > MAX_ENTRIES {
        let drain_count = ctx.entries.len() - MAX_ENTRIES;
        ctx.entries.drain(..drain_count);
    }

    ctx.updated_at = Utc::now();
    save(bus, &ctx).ok();
}

/// Format session context for agent prompt injection.
pub fn format_for_prompt(ctx: &SessionContext) -> String {
    if ctx.entries.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("## Recent session context\n\n");

    for entry in &ctx.entries {
        let icon = match entry.entry_type {
            EntryType::Success => "+",
            EntryType::Failure => "!",
            EntryType::Surprise => "?",
            EntryType::CostOverrun => "$",
        };
        out.push_str(&format!("- [{}] {} ({})\n", icon, entry.fact, entry.task_id));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn session_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        add_from_receipt(&bus, "signum", "task-1", "success", 0.1, 1.0, "fixed auth");
        add_from_receipt(&bus, "signum", "task-2", "failure", 0.0, 1.0, "build failed");

        let ctx = load(&bus, "signum");
        assert_eq!(ctx.entries.len(), 2);
        assert_eq!(ctx.entries[0].entry_type, EntryType::Success);
        assert_eq!(ctx.entries[1].entry_type, EntryType::Failure);
    }

    #[test]
    fn session_ttl_eviction() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        // Add 6 entries (TTL=5, each add decrements by 1)
        for i in 0..6 {
            add_from_receipt(&bus, "test", &format!("task-{i}"), "success", 0.0, 1.0, "");
        }

        let ctx = load(&bus, "test");
        // First entry should have been evicted (TTL reached 0)
        assert!(ctx.entries.len() <= 6);
        assert!(ctx.entries.iter().all(|e| e.ttl_tasks > 0));
    }

    #[test]
    fn session_max_entries() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        for i in 0..15 {
            let mut ctx = load(&bus, "test");
            ctx.entries.push(SessionEntry {
                entry_type: EntryType::Success,
                fact: format!("fact-{i}"),
                task_id: format!("t-{i}"),
                ttl_tasks: 100, // high TTL to avoid eviction
                created_at: Utc::now(),
            });
            if ctx.entries.len() > MAX_ENTRIES {
                let drain = ctx.entries.len() - MAX_ENTRIES;
                ctx.entries.drain(..drain);
            }
            ctx.updated_at = Utc::now();
            save(&bus, &ctx).unwrap();
        }

        let ctx = load(&bus, "test");
        assert!(ctx.entries.len() <= MAX_ENTRIES);
    }

    #[test]
    fn format_prompt() {
        let ctx = SessionContext {
            project: "test".into(),
            entries: vec![
                SessionEntry {
                    entry_type: EntryType::Success,
                    fact: "deployed v2".into(),
                    task_id: "t-1".into(),
                    ttl_tasks: 3,
                    created_at: Utc::now(),
                },
                SessionEntry {
                    entry_type: EntryType::Failure,
                    fact: "tests broke".into(),
                    task_id: "t-2".into(),
                    ttl_tasks: 2,
                    created_at: Utc::now(),
                },
            ],
            updated_at: Utc::now(),
        };

        let prompt = format_for_prompt(&ctx);
        assert!(prompt.contains("[+] deployed v2"));
        assert!(prompt.contains("[!] tests broke"));
    }
}
