use std::fs;
use std::path::Path;

use serde_json::Value;

/// List tasks in failed/ and dead/ for triage.
pub fn list_triage(bus: &Path) -> Vec<TriageEntry> {
    let mut entries = Vec::new();

    for (dir_name, source) in [("failed", "failed"), ("dead", "dead")] {
        let dir = bus.join(dir_name);
        if let Ok(items) = fs::read_dir(&dir) {
            for item in items.flatten() {
                let path = item.path();
                if !path.is_dir() {
                    continue;
                }

                let task_id = path.file_name().unwrap().to_string_lossy().to_string();

                // Try receipt.json, then task.json
                let (project, model, error_excerpt) =
                    if let Some(v) = read_json(&path.join("receipt.json")) {
                        (
                            json_str(&v, "project"),
                            json_str(&v, "model"),
                            json_str(&v, "summary"),
                        )
                    } else if let Some(v) = read_json(&path.join("task.json")) {
                        (
                            json_str(&v, "project"),
                            json_str(&v, "model"),
                            String::new(),
                        )
                    } else {
                        continue;
                    };

                entries.push(TriageEntry {
                    task_id,
                    project,
                    model,
                    source: source.to_string(),
                    error_excerpt,
                });
            }
        }
    }

    entries.sort_by(|a, b| {
        triage_source_rank(&a.source)
            .cmp(&triage_source_rank(&b.source))
            .then_with(|| a.task_id.cmp(&b.task_id))
            .then_with(|| a.project.cmp(&b.project))
            .then_with(|| a.model.cmp(&b.model))
    });

    entries
}

fn triage_source_rank(source: &str) -> u8 {
    match source {
        "dead" => 0,
        "failed" => 1,
        _ => 2,
    }
}

#[derive(Debug)]
pub struct TriageEntry {
    pub task_id: String,
    pub project: String,
    pub model: String,
    pub source: String,
    pub error_excerpt: String,
}

/// Retry a failed/dead task by moving it back to new/p1/.
pub fn retry_task(bus: &Path, task_id: &str) -> Result<(), String> {
    let task_json = find_task_json(bus, task_id)?;
    let dest = bus.join("new/p1").join(format!("{task_id}.json"));
    fs::copy(&task_json, &dest).map_err(|e| format!("copy failed: {e}"))?;

    // Remove from failed/dead
    for dir in &["failed", "dead"] {
        let src = bus.join(dir).join(task_id);
        if src.is_dir() {
            fs::remove_dir_all(&src).ok();
        }
    }
    Ok(())
}

/// Cancel a task (remove from queue or kill running process).
pub fn cancel_task(bus: &Path, task_id: &str) -> Result<(), String> {
    // Check new/ (queued)
    for sub in &["new/p0", "new/p1", "new/p2", "new"] {
        let path = bus.join(sub).join(format!("{task_id}.json"));
        if path.exists() {
            fs::remove_file(&path).map_err(|e| format!("remove failed: {e}"))?;
            return Ok(());
        }
    }

    // Check cur/ (running) — write cancel signal for daemon to process
    let cur_path = bus.join("cur").join(format!("{task_id}.json"));
    if cur_path.exists() {
        let cancel_dir = bus.join(".cancel");
        fs::create_dir_all(&cancel_dir).ok();
        fs::write(cancel_dir.join(task_id), "cancel").map_err(|e| format!("signal failed: {e}"))?;
        return Ok(());
    }

    Err(format!("task '{task_id}' not found in queue or running"))
}

fn find_task_json(bus: &Path, task_id: &str) -> Result<std::path::PathBuf, String> {
    for dir in &["failed", "dead"] {
        let path = bus.join(dir).join(task_id).join("task.json");
        if path.exists() {
            return Ok(path);
        }
    }
    Err(format!("no task.json for '{task_id}' in failed/ or dead/"))
}

fn read_json(path: &Path) -> Option<Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn json_str(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{}-{}-{}", prefix, std::process::id(), nanos));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_receipt(dir: &Path, project: &str, model: &str, summary: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("receipt.json"),
            format!(r#"{{"project":"{project}","model":"{model}","summary":"{summary}"}}"#),
        )
        .unwrap();
    }

    #[test]
    fn list_triage_orders_dead_before_failed_then_task_id() {
        let bus = temp_test_dir("punk-ops-triage");
        write_receipt(
            &bus.join("failed/task-b"),
            "specpunk",
            "codex",
            "failed summary",
        );
        write_receipt(
            &bus.join("dead/task-c"),
            "specpunk",
            "codex",
            "dead summary c",
        );
        write_receipt(
            &bus.join("dead/task-a"),
            "specpunk",
            "claude",
            "dead summary a",
        );

        let entries = list_triage(&bus);
        let pairs: Vec<_> = entries
            .iter()
            .map(|entry| (entry.source.as_str(), entry.task_id.as_str()))
            .collect();

        assert_eq!(
            pairs,
            vec![("dead", "task-a"), ("dead", "task-c"), ("failed", "task-b")]
        );

        let _ = fs::remove_dir_all(&bus);
    }
}
