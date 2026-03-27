use std::fs;
use std::path::{Path, PathBuf};

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

    entries
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

    // Check cur/ (running) — kill process, clean up, move to failed
    let cur_path = bus.join("cur").join(format!("{task_id}.json"));
    if cur_path.exists() {
        // Kill process if PID file exists
        let pid_file = bus.join(".pids").join(format!("{task_id}.pid"));
        if let Ok(pid_str) = fs::read_to_string(&pid_file) {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
            }
        }

        // Clean up tracking files
        fs::remove_file(bus.join(".heartbeats").join(format!("{task_id}.hb"))).ok();
        fs::remove_file(&pid_file).ok();

        // Release slot
        let slots_dir = bus.join(".slots");
        if let Ok(entries) = fs::read_dir(&slots_dir) {
            for entry in entries.flatten() {
                let slot_dir = entry.path();
                if slot_dir.join(task_id).exists() {
                    fs::remove_dir_all(&slot_dir).ok();
                    break;
                }
            }
        }

        // Release project lock
        let task_data = fs::read_to_string(&cur_path).ok();
        if let Some(ref data) = task_data {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(project) = v.get("project").and_then(|v| v.as_str()) {
                    fs::remove_file(bus.join(".locks").join(project)).ok();
                }
            }
        }

        // Move staging dir to failed/ (preserves stdout/stderr for diagnostics)
        let staging = PathBuf::from(format!("/tmp/punk-stage-{task_id}"));
        let dest = bus.join("failed").join(task_id);
        if staging.is_dir() {
            fs::rename(&staging, &dest).ok();
        } else {
            fs::create_dir_all(&dest).ok();
            fs::rename(&cur_path, dest.join("task.json"))
                .map_err(|e| format!("move failed: {e}"))?;
        }
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
