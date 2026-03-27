use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Lightweight task info extracted from task.json in the bus.
#[derive(Debug)]
pub struct QueuedTask {
    pub id: String,
    pub project: String,
    pub model: String,
    pub category: String,
    pub priority: String,
}

/// Lightweight receipt info — works with both legacy and v1 receipts.
#[derive(Debug)]
pub struct CompletedTask {
    pub id: String,
    pub project: String,
    pub model: String,
    pub status: String,
    pub cost_usd: f64,
    pub duration_s: u64,
    pub exit_code: i32,
}

/// Failed task with no receipt (only task.json present).
#[derive(Debug)]
pub struct FailedTask {
    pub id: String,
    pub project: String,
    pub model: String,
    pub category: String,
}

/// Snapshot of the bus state at a point in time.
#[derive(Debug)]
pub struct BusState {
    pub queued: Vec<QueuedTask>,
    pub running: Vec<QueuedTask>,
    pub done: Vec<CompletedTask>,
    pub failed: Vec<FailedTask>,
}

/// Resolve the bus directory from env or default.
pub fn bus_dir() -> PathBuf {
    std::env::var("PUNK_BUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("vicc/state/bus")
        })
}

/// Read the current bus state. Best-effort: skips unparseable files.
pub fn read_state(bus: &Path, recent_limit: usize) -> BusState {
    BusState {
        queued: read_queued(bus),
        running: read_running(bus),
        done: read_done(bus, recent_limit),
        failed: read_failed(bus),
    }
}

fn read_queued(bus: &Path) -> Vec<QueuedTask> {
    let new_dir = bus.join("new");
    let mut tasks = Vec::new();

    // Check priority subdirs first (p0, p1, p2), then root
    for subdir in &["p0", "p1", "p2"] {
        let dir = new_dir.join(subdir);
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    if let Some(task) = parse_task_file(&path, subdir) {
                        tasks.push(task);
                    }
                }
            }
        }
    }

    // Root-level tasks (no priority subdir = p1)
    if let Ok(entries) = fs::read_dir(&new_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "json") {
                if let Some(task) = parse_task_file(&path, "p1") {
                    tasks.push(task);
                }
            }
        }
    }

    tasks
}

fn read_running(bus: &Path) -> Vec<QueuedTask> {
    let cur_dir = bus.join("cur");
    let mut tasks = Vec::new();

    if let Ok(entries) = fs::read_dir(cur_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Some(task) = parse_task_file(&path, "") {
                    tasks.push(task);
                }
            }
        }
    }

    tasks
}

fn read_done(bus: &Path, limit: usize) -> Vec<CompletedTask> {
    let done_dir = bus.join("done");
    let mut dirs = list_dirs_sorted_recent(&done_dir);
    dirs.truncate(limit);

    dirs.iter()
        .filter_map(|dir| parse_receipt_dir(dir))
        .collect()
}

fn read_failed(bus: &Path) -> Vec<FailedTask> {
    let failed_dir = bus.join("failed");

    list_dirs_sorted_recent(&failed_dir)
        .iter()
        .filter_map(|dir| parse_failed_dir(dir))
        .collect()
}

fn list_dirs_sorted_recent(parent: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();

    // Sort by name descending (task IDs contain timestamps, so lexicographic = chronological)
    dirs.sort();
    dirs.reverse();
    dirs
}

fn parse_task_file(path: &Path, priority_hint: &str) -> Option<QueuedTask> {
    let data = fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&data).ok()?;

    let id = path.file_stem()?.to_str()?.to_string();

    Some(QueuedTask {
        id,
        project: json_str(&v, "project"),
        model: json_str(&v, "model"),
        category: json_str(&v, "category"),
        priority: if !priority_hint.is_empty() {
            priority_hint.to_string()
        } else {
            json_str_or(&v, "priority", "p1")
        },
    })
}

fn parse_receipt_dir(dir: &Path) -> Option<CompletedTask> {
    let receipt_path = dir.join("receipt.json");
    let data = fs::read_to_string(receipt_path).ok()?;
    let v: Value = serde_json::from_str(&data).ok()?;

    let id = dir.file_name()?.to_str()?.to_string();

    // Handle both v1 (duration_ms) and legacy (duration_seconds)
    let duration_s = if let Some(ms) = v.get("duration_ms").and_then(|v| v.as_u64()) {
        ms / 1000
    } else {
        v.get("duration_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    };

    // Handle both v1 (success/failure) and legacy (completed/failed)
    let raw_status = json_str(&v, "status");
    let status = match raw_status.as_str() {
        "completed" => "success".to_string(),
        other => other.to_string(),
    };

    Some(CompletedTask {
        id,
        project: json_str(&v, "project"),
        model: json_str(&v, "model"),
        status,
        cost_usd: v.get("cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0),
        duration_s,
        exit_code: v.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32,
    })
}

fn parse_failed_dir(dir: &Path) -> Option<FailedTask> {
    let id = dir.file_name()?.to_str()?.to_string();

    // Try receipt.json first, fall back to task.json
    let (project, model, category) = if let Ok(data) = fs::read_to_string(dir.join("receipt.json"))
    {
        let v: Value = serde_json::from_str(&data).unwrap_or_default();
        (json_str(&v, "project"), json_str(&v, "model"), json_str(&v, "category"))
    } else if let Ok(data) = fs::read_to_string(dir.join("task.json")) {
        let v: Value = serde_json::from_str(&data).unwrap_or_default();
        (json_str(&v, "project"), json_str(&v, "model"), json_str(&v, "category"))
    } else {
        return None;
    };

    Some(FailedTask {
        id,
        project,
        model,
        category,
    })
}

fn json_str(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn json_str_or(v: &Value, key: &str, default: &str) -> String {
    v.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}
