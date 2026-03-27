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
    /// Uses O_CREAT|O_EXCL (create_new) for atomic lock acquisition.
    pub fn try_acquire(&self, project: &str, task_id: &str) -> bool {
        let lock_file = self.locks_dir.join(project);
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_file)
            .and_then(|mut f| std::io::Write::write_all(&mut f, task_id.as_bytes()))
            .is_ok()
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
    use std::collections::HashSet;
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;
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

    // --- Concurrency tests ---

    /// 10 threads each try to acquire 3 slots from a 3-slot manager.
    /// Invariant: no slot ID is ever held by more than one thread simultaneously.
    #[test]
    fn concurrent_slot_no_double_acquire() {
        const ITERATIONS: usize = 100;

        for _ in 0..ITERATIONS {
            let tmp = TempDir::new().unwrap();
            let bus = tmp.path().to_path_buf();
            let sm = Arc::new(SlotManager::new(&bus, 3));
            let barrier = Arc::new(Barrier::new(10));
            let acquired: Arc<Mutex<HashSet<u32>>> = Arc::new(Mutex::new(HashSet::new()));
            let double_acquire_detected = Arc::new(Mutex::new(false));

            let mut handles = vec![];
            for _ in 0..10 {
                let sm = Arc::clone(&sm);
                let barrier = Arc::clone(&barrier);
                let acquired = Arc::clone(&acquired);
                let flag = Arc::clone(&double_acquire_detected);

                handles.push(thread::spawn(move || {
                    barrier.wait();
                    if let Some(slot_id) = sm.acquire() {
                        {
                            let mut set = acquired.lock().unwrap();
                            if !set.insert(slot_id) {
                                // slot_id already in set — double acquire
                                *flag.lock().unwrap() = true;
                            }
                        }
                        // hold briefly then release
                        thread::yield_now();
                        {
                            let mut set = acquired.lock().unwrap();
                            set.remove(&slot_id);
                        }
                        sm.release(slot_id);
                    }
                }));
            }

            for h in handles {
                h.join().unwrap();
            }

            assert!(
                !*double_acquire_detected.lock().unwrap(),
                "double acquire detected: same slot given to two threads simultaneously"
            );
        }
    }

    /// release() called concurrently with acquire() must not panic or corrupt state.
    #[test]
    fn concurrent_slot_release_during_acquire() {
        const ITERATIONS: usize = 100;

        for _ in 0..ITERATIONS {
            let tmp = TempDir::new().unwrap();
            let bus = tmp.path().to_path_buf();
            let sm = Arc::new(SlotManager::new(&bus, 3));

            // Pre-fill all 3 slots
            let s1 = sm.acquire().unwrap();
            let s2 = sm.acquire().unwrap();
            let s3 = sm.acquire().unwrap();
            assert!(sm.acquire().is_none());

            let barrier = Arc::new(Barrier::new(2));

            // Thread 1: release slots rapidly
            let sm1 = Arc::clone(&sm);
            let b1 = Arc::clone(&barrier);
            let releaser = thread::spawn(move || {
                b1.wait();
                sm1.release(s1);
                sm1.release(s2);
                sm1.release(s3);
            });

            // Thread 2: try to acquire while releases are happening
            let sm2 = Arc::clone(&sm);
            let b2 = Arc::clone(&barrier);
            let acquirer = thread::spawn(move || {
                b2.wait();
                let mut acquired_ids = vec![];
                for _ in 0..3 {
                    if let Some(id) = sm2.acquire() {
                        acquired_ids.push(id);
                    }
                }
                acquired_ids
            });

            releaser.join().unwrap();
            let ids = acquirer.join().unwrap();

            // All returned IDs must be in valid range and unique
            let id_set: HashSet<u32> = ids.iter().copied().collect();
            assert_eq!(id_set.len(), ids.len(), "duplicate slot IDs returned");
            for id in &ids {
                assert!(*id >= 1 && *id <= 3, "slot ID out of range: {id}");
            }
        }
    }

    /// Two threads try_acquire the same project simultaneously.
    /// At most one must succeed per attempt.
    #[test]
    fn concurrent_project_lock_at_most_one() {
        const ITERATIONS: usize = 100;

        for _ in 0..ITERATIONS {
            let tmp = TempDir::new().unwrap();
            let bus = tmp.path().to_path_buf();
            let pl = Arc::new(ProjectLock::new(&bus));
            let barrier = Arc::new(Barrier::new(2));
            let wins: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(vec![]));

            let mut handles = vec![];
            for t in 0..2u32 {
                let pl = Arc::clone(&pl);
                let barrier = Arc::clone(&barrier);
                let wins = Arc::clone(&wins);

                handles.push(thread::spawn(move || {
                    barrier.wait();
                    let task_id = format!("task-{t}");
                    let ok = pl.try_acquire("test-project", &task_id);
                    wins.lock().unwrap().push(ok);
                }));
            }

            for h in handles {
                h.join().unwrap();
            }

            let results = wins.lock().unwrap();
            let successes = results.iter().filter(|&&v| v).count();
            assert!(
                successes <= 1,
                "both threads acquired the same project lock simultaneously"
            );

            // cleanup for next iteration
            pl.release("test-project");
        }
    }

    /// 5 tasks in the queue, 5 threads each claiming. Each task must be claimed exactly once.
    #[test]
    fn concurrent_scan_claim_exactly_once() {
        const ITERATIONS: usize = 100;

        for _ in 0..ITERATIONS {
            let tmp = TempDir::new().unwrap();
            let bus = tmp.path().to_path_buf();

            // Setup queue dirs
            for d in &["new/p1", "cur"] {
                fs::create_dir_all(bus.join(d)).unwrap();
            }

            // Write 5 tasks
            let task_json = r#"{"project":"test","model":"claude","timeout_seconds":600}"#;
            for i in 0..5u32 {
                fs::write(bus.join(format!("new/p1/task-{i:03}.json")), task_json).unwrap();
            }

            let bus_arc = Arc::new(bus.clone());
            let barrier = Arc::new(Barrier::new(5));
            let claimed_ids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));

            let mut handles = vec![];
            for _ in 0..5 {
                let bus_arc = Arc::clone(&bus_arc);
                let barrier = Arc::clone(&barrier);
                let claimed_ids = Arc::clone(&claimed_ids);

                handles.push(thread::spawn(move || {
                    barrier.wait();
                    // Each thread scans and tries to claim whatever it sees
                    let entries = scan_queue(&bus_arc);
                    for entry in entries {
                        if let Some(_path) = claim_task(&bus_arc, &entry) {
                            claimed_ids.lock().unwrap().push(entry.task_id);
                        }
                    }
                }));
            }

            for h in handles {
                h.join().unwrap();
            }

            let ids = claimed_ids.lock().unwrap();
            let id_set: HashSet<String> = ids.iter().cloned().collect();

            // Each task must be claimed at most once (rename is atomic)
            assert_eq!(
                id_set.len(),
                ids.len(),
                "task claimed more than once: {:?}",
                ids
            );
            // All 5 tasks must be claimed
            assert_eq!(
                ids.len(),
                5,
                "not all tasks claimed: only {} out of 5",
                ids.len()
            );
        }
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

    // --- Adversarial tests ---

    #[test]
    fn adversarial_scan_queue_no_new_dir() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        // new/ dir doesn't exist at all — should return empty, not panic
        let entries = scan_queue(bus);
        assert!(entries.is_empty(), "missing new/ dir should return empty entries");
    }

    #[test]
    fn adversarial_scan_queue_zero_json_files() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        for d in &["new/p0", "new/p1", "new/p2"] {
            fs::create_dir_all(bus.join(d)).unwrap();
        }
        // Non-JSON files should be ignored
        fs::write(bus.join("new/p0/task.txt"), "not json").unwrap();
        fs::write(bus.join("new/p1/task.yaml"), "project: x").unwrap();

        let entries = scan_queue(bus);
        assert!(entries.is_empty(), "non-JSON files should be ignored");
    }

    #[test]
    fn adversarial_scan_queue_corrupt_json() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        fs::create_dir_all(bus.join("new/p0")).unwrap();

        // Corrupt JSON — parse_queued_entry returns None, entries silently skipped
        fs::write(bus.join("new/p0/corrupt.json"), b"not json!!!").unwrap();
        fs::write(bus.join("new/p0/empty.json"), b"").unwrap();
        fs::write(bus.join("new/p0/partial.json"), b"{\"project\":").unwrap();

        let entries = scan_queue(bus);
        assert!(entries.is_empty(), "corrupt JSON files should be silently skipped");
    }

    #[test]
    fn adversarial_claim_nonexistent_task() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        fs::create_dir_all(bus.join("cur")).unwrap();

        let fake_entry = QueuedEntry {
            task_id: "ghost-task".into(),
            path: bus.join("new/p1/ghost-task.json"), // doesn't exist
            project: "test".into(),
            model: "claude".into(),
            worktree: false,
            priority: "p1".into(),
        };

        let result = claim_task(bus, &fake_entry);
        // rename of nonexistent src fails — should return None, not panic
        assert!(result.is_none(), "claiming nonexistent task should return None");
    }

    #[test]
    fn adversarial_release_nonexistent_slot() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        let sm = SlotManager::new(bus, 3);

        // Release slots that were never acquired — remove_dir_all on nonexistent is .ok()
        sm.release(1);
        sm.release(99); // out-of-range slot
        sm.release(0);  // boundary: slot 0 not in 1..=max_slots

        // Should still work normally after ghost releases
        assert_eq!(sm.occupied(), 0);
        let s = sm.acquire().unwrap();
        assert_eq!(s, 1);
    }

    #[test]
    fn adversarial_slot_exhaustion_with_zero_max() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        // max_slots = 0: loop 1..=0 never executes
        let sm = SlotManager::new(bus, 0);
        let result = sm.acquire();
        assert!(result.is_none(), "0-slot manager should always return None");
        assert_eq!(sm.occupied(), 0);
    }

    #[test]
    fn adversarial_move_to_failed_nonexistent_task() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        // cur/ doesn't have the task — rename fails, should be silent (.ok())
        // No panic expected
        move_to_failed(bus, "nonexistent-task-xyz");
    }

    #[test]
    fn adversarial_scan_queue_task_missing_model_field() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        fs::create_dir_all(bus.join("new/p1")).unwrap();

        // Task JSON missing model — should use default "claude"
        let task = r#"{"project":"signum"}"#;
        fs::write(bus.join("new/p1/no-model.json"), task).unwrap();

        let entries = scan_queue(bus);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].model, "claude", "missing model should default to 'claude'");
        assert_eq!(entries[0].project, "signum");
    }

    #[test]
    fn adversarial_heartbeat_empty_task_id() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        let hbt = HeartbeatTracker::new(bus);

        // Empty task_id — creates files named ".hb" and ".pid"
        // Should not panic
        hbt.register("", 12345);
        hbt.unregister("");
    }

    #[test]
    fn adversarial_project_lock_lock_and_relock_same_task() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path();
        let pl = ProjectLock::new(bus);

        assert!(pl.try_acquire("proj", "task-1"));
        // Same task tries to acquire again — file already exists, should fail
        assert!(!pl.try_acquire("proj", "task-1"), "re-acquiring same lock should fail");
        pl.release("proj");
    }
}
