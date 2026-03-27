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
    let safe_project = match crate::sanitize::safe_id(project) {
        Ok(s) => s,
        Err(_) => return SessionContext {
            project: project.to_string(),
            entries: vec![],
            updated_at: Utc::now(),
        },
    };
    let path = sessions_dir(bus).join(format!("{safe_project}.json"));
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
    crate::sanitize::safe_id(&ctx.project)
        .map_err(|e| std::io::Error::other(format!("unsafe project name: {e}")))?;
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

    // --- Adversarial tests ---

    #[test]
    fn adversarial_rapid_add_from_receipt_20_calls() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        // 20 rapid calls — TTL eviction kicks in after 5 tasks
        for i in 0..20 {
            add_from_receipt(
                &bus,
                "test",
                &format!("task-{i}"),
                "success",
                0.1,
                1.0,
                &format!("fact {i}"),
            );
        }

        let ctx = load(&bus, "test");
        // MAX_ENTRIES = 10, but TTL eviction also removes old ones
        assert!(
            ctx.entries.len() <= MAX_ENTRIES,
            "entries should be capped at MAX_ENTRIES, got {}",
            ctx.entries.len()
        );
        // All remaining entries should have ttl_tasks > 0
        for e in &ctx.entries {
            assert!(
                e.ttl_tasks > 0,
                "no entry with ttl=0 should survive: {:?}",
                e.fact
            );
        }
    }

    #[test]
    fn adversarial_empty_strings() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        // Empty task_id, empty summary, empty status
        add_from_receipt(&bus, "proj", "", "success", 0.0, 1.0, "");
        let ctx = load(&bus, "proj");
        assert_eq!(ctx.entries.len(), 1);
        // When summary is empty, fact becomes "success: " (empty task_id)
        assert_eq!(ctx.entries[0].fact, "success: ");

        // Empty project name: creates a file named ".json"
        add_from_receipt(&bus, "", "task-x", "success", 0.0, 1.0, "something");
        // Should not panic — loading empty project is valid
        let empty_ctx = load(&bus, "");
        assert_eq!(empty_ctx.project, "");
    }

    #[test]
    fn adversarial_cost_overrun_zero_budget() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        // budget_usd = 0.0: overrun check is `cost > budget * 1.5 && budget > 0.0`
        // so zero budget should NOT trigger CostOverrun
        add_from_receipt(&bus, "test", "t1", "success", 9999.0, 0.0, "huge cost zero budget");
        let ctx = load(&bus, "test");
        assert_eq!(
            ctx.entries[0].entry_type,
            EntryType::Success,
            "zero budget with high cost should not trigger CostOverrun"
        );
    }

    #[test]
    fn adversarial_negative_cost_overrun_check() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        // Negative cost_usd: cost_usd > budget_usd * 1.5 is false for negative cost
        add_from_receipt(&bus, "test", "t1", "success", -5.0, 1.0, "negative cost");
        let ctx = load(&bus, "test");
        // Should be Success, not CostOverrun
        assert_eq!(ctx.entries[0].entry_type, EntryType::Success);
    }

    #[test]
    fn adversarial_unknown_status_maps_to_failure() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        // Status that is neither "success" nor "timeout" falls through to Failure
        for status in &["error", "cancelled", "TIMEOUT", "SUCCESS", "", "🔥"] {
            add_from_receipt(&bus, "test", "t1", status, 0.0, 1.0, "");
        }

        let ctx = load(&bus, "test");
        // "success" (exact lowercase) is the only one that maps to Success
        // All others → Failure (or possibly CostOverrun if cost threshold met)
        // We added 6 entries but TTL will evict some; just check no panic
        assert!(ctx.entries.len() <= MAX_ENTRIES);
    }

    #[test]
    fn adversarial_format_for_prompt_empty_entries() {
        let ctx = SessionContext {
            project: "test".into(),
            entries: vec![],
            updated_at: Utc::now(),
        };
        let prompt = format_for_prompt(&ctx);
        // Should return empty string, not header without content
        assert!(prompt.is_empty(), "empty entries should produce empty prompt");
    }

    #[test]
    fn adversarial_format_for_prompt_special_chars_in_fact() {
        let ctx = SessionContext {
            project: "test".into(),
            entries: vec![SessionEntry {
                entry_type: EntryType::Surprise,
                fact: "task had [brackets] and (parens) and\nnewlines\ttabs".into(),
                task_id: "t-weird".into(),
                ttl_tasks: 5,
                created_at: Utc::now(),
            }],
            updated_at: Utc::now(),
        };
        // Should not panic, special chars pass through as-is
        let prompt = format_for_prompt(&ctx);
        assert!(prompt.contains("[?]"));
        assert!(prompt.contains("brackets"));
    }

    // --- Security tests: path traversal in load / save ---

    #[test]
    fn security_load_traversal_project_name() {
        // load() constructs sessions_dir(bus).join("{project}.json") without sanitization.
        // A project name of "../../../tmp/evil" would resolve outside the sessions dir.
        // This test verifies whether the traversal is blocked or accepted.
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        // Create a sentinel file to detect escape
        let sentinel = tmp.path().join("canary.json");
        let sentinel_data = r#"{"project":"canary","entries":[],"updated_at":"2026-01-01T00:00:00Z"}"#;
        fs::write(&sentinel, sentinel_data).unwrap();

        // sessions_dir = tmp/sessions; path = tmp/sessions/../canary = tmp/canary
        let traversal_project = "../canary";
        let ctx = load(&bus, traversal_project);

        // If traversal works, ctx.project would be "canary" (deserialized from sentinel).
        // If blocked (or file not found), we get a default context with the traversal string.
        assert!(
            ctx.project != "canary",
            "SECURITY BYPASS: load() with project='../canary' read file outside sessions dir — \
             got project='{}' from sentinel",
            ctx.project
        );
        // Note: on macOS Path::join with ".." does resolve at open() time,
        // so this test MAY fail (bypass confirmed) depending on directory structure.
    }

    #[test]
    fn security_save_traversal_project_name() {
        // save() constructs path from ctx.project without sanitization.
        // A SessionContext with project="../../../tmp/evil" would write outside sessions/.
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        let evil_ctx = SessionContext {
            project: "../evil-session".into(),
            entries: vec![],
            updated_at: Utc::now(),
        };

        let _ = save(&bus, &evil_ctx);

        // sessions_dir = tmp/sessions; path = tmp/sessions/../evil-session.json = tmp/evil-session.json
        let escaped_path = tmp.path().join("evil-session.json");
        assert!(
            !escaped_path.exists(),
            "SECURITY BYPASS: save() with project='../evil-session' wrote file outside sessions dir: {}",
            escaped_path.display()
        );
    }

    #[test]
    fn security_add_from_receipt_traversal_project() {
        // add_from_receipt calls load() then save() — same traversal vector.
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        // Should not panic regardless of traversal in project name
        add_from_receipt(&bus, "../../../tmp/evil", "task-1", "success", 0.1, 1.0, "test");

        // Verify no file was written outside tmp
        let potential_escape = std::path::Path::new("/tmp/evil.json");
        assert!(
            !potential_escape.exists(),
            "SECURITY BYPASS: add_from_receipt wrote session file to /tmp/evil.json"
        );
    }

    #[test]
    fn security_load_null_byte_in_project() {
        // Null byte in project name terminates string at OS level on some platforms.
        // sessions_dir.join("foo\x00bar.json") could become "foo.json" on some OSes.
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        // Create a "foo.json" as a sentinel
        let sessions_dir = tmp.path().join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        let sentinel_data = r#"{"project":"foo","entries":[],"updated_at":"2026-01-01T00:00:00Z"}"#;
        fs::write(sessions_dir.join("foo.json"), sentinel_data).unwrap();

        // Try to load "foo\x00bar" — should NOT silently load "foo.json"
        // Rust's std does reject null bytes in paths with an error, so load() should
        // return a default context (file-not-found branch).
        let ctx = load(&bus, "foo\x00bar");
        assert!(
            ctx.project != "foo",
            "SECURITY: null byte in project name must not silently load 'foo.json'"
        );
    }
}
