use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

/// Slot-based concurrency control using mkdir atomicity.
pub struct SlotManager {
    slots_dir: PathBuf,
    max_slots: u32,
}

impl SlotManager {
    pub fn new(bus: &Path, max_slots: u32) -> Self {
        let slots_dir = bus.join(".slots");
        fs::create_dir_all(&slots_dir).ok();
        Self { slots_dir, max_slots }
    }

    /// Acquire a slot. Returns slot ID (1..=max_slots) or None.
    /// Uses mkdir atomicity: succeeds only if directory doesn't exist.
    pub fn acquire(&self) -> Option<u32> {
        for i in 1..=self.max_slots {
            let slot_dir = self.slots_dir.join(format!("slot-{i}"));
            if fs::create_dir(&slot_dir).is_ok() {
                return Some(i);
            }
        }
        None
    }

    /// Release a slot, cleaning up all tracking files inside.
    pub fn release(&self, slot_id: u32) {
        let slot_dir = self.slots_dir.join(format!("slot-{slot_id}"));
        fs::remove_dir_all(&slot_dir).ok();
    }

    /// Record which task owns a slot (for crash recovery).
    pub fn record_owner(&self, slot_id: u32, task_id: &str) {
        let slot_dir = self.slots_dir.join(format!("slot-{slot_id}"));
        let owner_file = slot_dir.join(task_id);
        fs::write(owner_file, task_id).ok();
    }

    /// Count currently occupied slots.
    pub fn occupied(&self) -> u32 {
        (1..=self.max_slots)
            .filter(|i| self.slots_dir.join(format!("slot-{i}")).is_dir())
            .count() as u32
    }

    /// Crash recovery: remove slots whose owner PID is dead.
    pub fn recover_stale(&self, pids_dir: &Path) {
        for i in 1..=self.max_slots {
            let slot_dir = self.slots_dir.join(format!("slot-{i}"));
            if !slot_dir.is_dir() {
                continue;
            }

            let mut alive = false;
            if let Ok(entries) = fs::read_dir(&slot_dir) {
                for entry in entries.flatten() {
                    let task_id = entry.file_name().to_string_lossy().to_string();
                    let pid_file = pids_dir.join(format!("{task_id}.pid"));
                    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
                        if let Ok(pid) = pid_str.trim().parse::<i32>() {
                            if is_process_alive(pid) {
                                alive = true;
                            }
                        }
                    }
                }
            }

            if !alive {
                fs::remove_dir_all(&slot_dir).ok();
            }
        }
    }
}

/// Per-project lock to prevent parallel tasks on same repo.
pub struct ProjectLock {
    locks_dir: PathBuf,
}

impl ProjectLock {
    pub fn new(bus: &Path) -> Self {
        let locks_dir = bus.join(".locks");
        fs::create_dir_all(&locks_dir).ok();
        Self { locks_dir }
    }

    /// Try to acquire project lock. Returns false if project already locked.
    pub fn try_acquire(&self, project: &str, task_id: &str) -> bool {
        let lock_file = self.locks_dir.join(project);
        if lock_file.exists() {
            return false;
        }
        fs::write(&lock_file, task_id).is_ok()
    }

    /// Release project lock.
    pub fn release(&self, project: &str) {
        let lock_file = self.locks_dir.join(project);
        fs::remove_file(lock_file).ok();
    }

    /// Cleanup all locks (crash recovery).
    pub fn clear_all(&self) {
        fs::remove_dir_all(&self.locks_dir).ok();
        fs::create_dir_all(&self.locks_dir).ok();
    }
}

/// Heartbeat tracker: tasks touch .hb files, we check mtime.
pub struct HeartbeatTracker {
    hb_dir: PathBuf,
    pids_dir: PathBuf,
}

impl HeartbeatTracker {
    pub fn new(bus: &Path) -> Self {
        let hb_dir = bus.join(".heartbeats");
        let pids_dir = bus.join(".pids");
        fs::create_dir_all(&hb_dir).ok();
        fs::create_dir_all(&pids_dir).ok();
        Self { hb_dir, pids_dir }
    }

    /// Create heartbeat + pid tracking for a task.
    pub fn register(&self, task_id: &str, pid: u32) {
        fs::write(self.hb_dir.join(format!("{task_id}.hb")), "").ok();
        fs::write(self.pids_dir.join(format!("{task_id}.pid")), pid.to_string()).ok();
    }

    /// Remove tracking files for a task.
    pub fn unregister(&self, task_id: &str) {
        fs::remove_file(self.hb_dir.join(format!("{task_id}.hb"))).ok();
        fs::remove_file(self.pids_dir.join(format!("{task_id}.pid"))).ok();
    }

    /// Find tasks whose heartbeat is stale (mtime older than timeout).
    pub fn find_stale(&self, cur_dir: &Path) -> Vec<StaleTask> {
        let mut stale = Vec::new();
        let entries = match fs::read_dir(&self.hb_dir) {
            Ok(e) => e,
            Err(_) => return stale,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "hb") {
                let task_id = path.file_stem().unwrap().to_string_lossy().to_string();
                let task_file = cur_dir.join(format!("{task_id}.json"));

                if !task_file.exists() {
                    continue;
                }

                let timeout_s = read_task_timeout(&task_file);
                let age_s = file_age_secs(&path);

                if age_s > timeout_s {
                    let pid = self.read_pid(&task_id);
                    stale.push(StaleTask {
                        task_id,
                        age_s,
                        timeout_s,
                        pid,
                    });
                }
            }
        }
        stale
    }

    fn read_pid(&self, task_id: &str) -> Option<i32> {
        let pid_file = self.pids_dir.join(format!("{task_id}.pid"));
        fs::read_to_string(pid_file)
            .ok()
            .and_then(|s| s.trim().parse().ok())
    }

    pub fn pids_dir(&self) -> &Path {
        &self.pids_dir
    }

    /// Crash recovery: use SlotManager to recover stale slots.
    pub fn recover_stale_slots(&self, slots: &SlotManager) {
        slots.recover_stale(&self.pids_dir);
    }
}

#[derive(Debug)]
pub struct StaleTask {
    pub task_id: String,
    pub age_s: u64,
    pub timeout_s: u64,
    pub pid: Option<i32>,
}

/// Scan the queue for claimable tasks in priority order.
/// Returns task file paths in order: p0 -> p1 -> p2 -> root.
pub fn scan_queue(bus: &Path) -> Vec<QueuedEntry> {
    let new_dir = bus.join("new");
    let mut entries = Vec::new();

    for priority in &["p0", "p1", "p2"] {
        let dir = new_dir.join(priority);
        if let Ok(files) = fs::read_dir(&dir) {
            for entry in files.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    if let Some(e) = parse_queued_entry(&path, priority) {
                        entries.push(e);
                    }
                }
            }
        }
    }

    // Root-level tasks (no priority subdir)
    if let Ok(files) = fs::read_dir(&new_dir) {
        for entry in files.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "json") {
                if let Some(e) = parse_queued_entry(&path, "p1") {
                    entries.push(e);
                }
            }
        }
    }

    entries
}

#[derive(Debug)]
pub struct QueuedEntry {
    pub task_id: String,
    pub path: PathBuf,
    pub project: String,
    pub model: String,
    pub worktree: bool,
    pub priority: String,
}

/// Atomically claim a task: rename from new/ to cur/.
/// Returns the new path in cur/ on success.
pub fn claim_task(bus: &Path, entry: &QueuedEntry) -> Option<PathBuf> {
    let dest = bus.join("cur").join(format!("{}.json", entry.task_id));
    fs::create_dir_all(bus.join("cur")).ok();
    fs::rename(&entry.path, &dest).ok().map(|_| dest)
}

/// Move a task from cur/ to failed/ (timeout, error).
pub fn move_to_failed(bus: &Path, task_id: &str) {
    let src = bus.join("cur").join(format!("{task_id}.json"));
    let dest_dir = bus.join("failed").join(task_id);
    fs::create_dir_all(&dest_dir).ok();
    fs::rename(src, dest_dir.join("task.json")).ok();
}

// --- Helpers ---

fn parse_queued_entry(path: &Path, priority: &str) -> Option<QueuedEntry> {
    let data = fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&data).ok()?;
    let task_id = path.file_stem()?.to_str()?.to_string();

    Some(QueuedEntry {
        task_id,
        path: path.to_path_buf(),
        project: v.get("project").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        model: v.get("model").and_then(|v| v.as_str()).unwrap_or("claude").to_string(),
        worktree: v.get("worktree").and_then(|v| v.as_bool()).unwrap_or(false),
        priority: priority.to_string(),
    })
}

fn read_task_timeout(task_file: &Path) -> u64 {
    fs::read_to_string(task_file)
        .ok()
        .and_then(|data| serde_json::from_str::<Value>(&data).ok())
        .and_then(|v| v.get("timeout_seconds").and_then(|v| v.as_u64()))
        .unwrap_or(600)
}

fn file_age_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_process_alive(pid: i32) -> bool {
    // kill(pid, 0) checks if process exists without sending a signal
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn slot_acquire_release() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        let sm = SlotManager::new(bus, 3);

        assert_eq!(sm.occupied(), 0);
        let s1 = sm.acquire().unwrap();
        assert_eq!(s1, 1);
        assert_eq!(sm.occupied(), 1);

        let s2 = sm.acquire().unwrap();
        let s3 = sm.acquire().unwrap();
        assert!(sm.acquire().is_none()); // all slots full

        sm.release(s2);
        assert_eq!(sm.occupied(), 2);
        let s4 = sm.acquire().unwrap();
        assert_eq!(s4, s2); // reuses released slot
        sm.release(s1);
        sm.release(s3);
        sm.release(s4);
    }

    #[test]
    fn project_lock() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        let pl = ProjectLock::new(bus);

        assert!(pl.try_acquire("signum", "task-1"));
        assert!(!pl.try_acquire("signum", "task-2")); // already locked
        assert!(pl.try_acquire("mycel", "task-3")); // different project OK

        pl.release("signum");
        assert!(pl.try_acquire("signum", "task-4")); // re-acquirable
    }

    #[test]
    fn scan_empty_queue() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        fs::create_dir_all(bus.join("new/p0")).unwrap();
        fs::create_dir_all(bus.join("new/p1")).unwrap();
        fs::create_dir_all(bus.join("new/p2")).unwrap();

        let entries = scan_queue(bus);
        assert!(entries.is_empty());
    }

    #[test]
    fn scan_and_claim() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        fs::create_dir_all(bus.join("new/p0")).unwrap();
        fs::create_dir_all(bus.join("new/p1")).unwrap();
        fs::create_dir_all(bus.join("new/p2")).unwrap();
        fs::create_dir_all(bus.join("cur")).unwrap();

        // Add a p1 task
        let task = r#"{"project":"signum","model":"claude","timeout_seconds":600}"#;
        fs::write(bus.join("new/p1/task-001.json"), task).unwrap();

        let entries = scan_queue(bus);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].task_id, "task-001");
        assert_eq!(entries[0].priority, "p1");

        // Claim it
        let claimed = claim_task(bus, &entries[0]);
        assert!(claimed.is_some());
        assert!(bus.join("cur/task-001.json").exists());
        assert!(!bus.join("new/p1/task-001.json").exists());

        // Queue is now empty
        assert!(scan_queue(bus).is_empty());
    }

    #[test]
    fn priority_ordering() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        for d in &["new/p0", "new/p1", "new/p2"] {
            fs::create_dir_all(bus.join(d)).unwrap();
        }

        let task = r#"{"project":"x","model":"claude"}"#;
        fs::write(bus.join("new/p2/low.json"), task).unwrap();
        fs::write(bus.join("new/p0/crit.json"), task).unwrap();
        fs::write(bus.join("new/p1/norm.json"), task).unwrap();

        let entries = scan_queue(bus);
        assert_eq!(entries[0].priority, "p0");
        assert_eq!(entries[1].priority, "p1");
        assert_eq!(entries[2].priority, "p2");
    }
}
