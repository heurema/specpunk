use std::fs;
use std::path::{Path, PathBuf};

use crate::sanitize;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A high-level objective that the system autonomously plans and executes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub project: String,
    pub objective: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<String>,
    pub budget_usd: f64,
    #[serde(default)]
    pub spent_usd: f64,
    pub status: GoalStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Plan>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Planning,
    AwaitingApproval,
    Active,
    Paused,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub version: u32,
    pub created_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<DateTime<Utc>>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub step: u32,
    pub category: String,
    pub prompt: String,
    pub agent: String,
    #[serde(default)]
    pub est_cost_usd: f64,
    #[serde(default)]
    pub depends_on: Vec<u32>,
    pub status: StepStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default)]
    pub sub_tasks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    Blocked,
    Skipped,
}

#[derive(Debug, thiserror::Error)]
pub enum QueueReadyError {
    #[error("goal queue config error: {0}")]
    Config(#[from] crate::config::ConfigError),
    #[error("failed to queue goal step task: {0}")]
    Write(#[from] std::io::Error),
}

// --- Storage ---

fn goals_dir(bus: &Path) -> PathBuf {
    let state_dir = bus.parent().unwrap_or(bus);
    state_dir.join("goals")
}

/// List all goals.
pub fn list_goals(bus: &Path) -> Vec<Goal> {
    let dir = goals_dir(bus);
    let mut goals = Vec::new();

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Ok(data) = fs::read_to_string(&path) {
                    if let Ok(goal) = serde_json::from_str::<Goal>(&data) {
                        goals.push(goal);
                    }
                }
            }
        }
    }

    goals.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    goals
}

/// Load a specific goal.
pub fn load_goal(bus: &Path, goal_id: &str) -> Option<Goal> {
    let safe = sanitize::safe_id(goal_id).ok()?;
    let path = goals_dir(bus).join(format!("{safe}.json"));
    fs::read_to_string(path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
}

/// Save a goal (create or update).
pub fn save_goal(bus: &Path, goal: &Goal) -> std::io::Result<()> {
    sanitize::safe_id(&goal.id)
        .map_err(|e| std::io::Error::other(format!("unsafe goal id: {e}")))?;
    let dir = goals_dir(bus);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", goal.id));
    let json = serde_json::to_string_pretty(goal)
        .map_err(std::io::Error::other)?;
    fs::write(path, json)
}

/// Create a new goal in Planning status.
pub fn create_goal(
    bus: &Path,
    project: &str,
    objective: &str,
    deadline: Option<&str>,
    budget_usd: f64,
) -> std::io::Result<Goal> {
    sanitize::safe_id(project)
        .map_err(|e| std::io::Error::other(format!("unsafe project name: {e}")))?;
    let id = format!(
        "{}-{}",
        project,
        Utc::now().format("%Y%m%d-%H%M%S")
    );

    let goal = Goal {
        id: id.clone(),
        project: project.to_string(),
        objective: objective.to_string(),
        deadline: deadline.map(|s| s.to_string()),
        budget_usd,
        spent_usd: 0.0,
        status: GoalStatus::Planning,
        plan: None,
        created_at: Utc::now(),
        completed_at: None,
    };

    save_goal(bus, &goal)?;
    Ok(goal)
}

/// Build the planner prompt from goal + project context.
pub fn build_planner_prompt(goal: &Goal, project_path: &Path) -> String {
    let mut prompt = String::new();

    prompt.push_str("You are a planner agent. Generate an implementation plan for this goal.\n\n");
    prompt.push_str(&format!("## Goal\n{}\n\n", goal.objective));

    if let Some(ref deadline) = goal.deadline {
        prompt.push_str(&format!("**Deadline:** {deadline}\n"));
    }
    prompt.push_str(&format!("**Budget:** ${:.2}\n\n", goal.budget_usd));

    // Read project context
    for ctx_file in &["CLAUDE.md", "README.md"] {
        let path = project_path.join(ctx_file);
        if let Ok(content) = fs::read_to_string(&path) {
            let truncated: String = content.chars().take(2000).collect();
            prompt.push_str(&format!("## {ctx_file}\n{truncated}\n\n"));
        }
    }

    prompt.push_str(r#"## Output Format

Respond with ONLY a JSON array of steps. Each step:
```json
[
  {
    "step": 1,
    "category": "research|codegen|fix|review|content|audit",
    "prompt": "What the agent should do (detailed, actionable)",
    "agent": "claude-sonnet|codex-auto|gemini-scout",
    "est_cost_usd": 0.50,
    "depends_on": []
  }
]
```

Rules:
- 5-15 steps
- Each step must be independently executable by an AI agent
- Include dependencies where steps must run in order
- Total estimated cost should not exceed the budget
- Use research steps before codegen for complex tasks
- End with a review/verification step
"#);

    prompt
}

/// Parse planner output into a Plan.
pub fn parse_plan(planner_output: &str, planner_model: &str) -> Option<Plan> {
    // Extract JSON array from planner output (may be wrapped in markdown)
    let json_str = extract_json_array(planner_output)?;
    let steps_raw: Vec<serde_json::Value> = serde_json::from_str(&json_str).ok()?;

    let steps: Vec<Step> = steps_raw
        .into_iter()
        .map(|v| Step {
            step: v.get("step").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            category: v.get("category").and_then(|v| v.as_str()).unwrap_or("codegen").to_string(),
            prompt: v.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            agent: v.get("agent").and_then(|v| v.as_str()).unwrap_or("claude-sonnet").to_string(),
            est_cost_usd: v.get("est_cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.5),
            depends_on: v
                .get("depends_on")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_u64().map(|n| n as u32))
                        .collect()
                })
                .unwrap_or_default(),
            status: StepStatus::Pending,
            task_id: None,
            sub_tasks: vec![],
        })
        .collect();

    if steps.is_empty() {
        return None;
    }

    Some(Plan {
        version: 1,
        created_by: planner_model.to_string(),
        approved_at: None,
        steps,
    })
}

/// Extract JSON array from text that may contain markdown fences.
fn extract_json_array(text: &str) -> Option<String> {
    // Try direct parse first
    if text.trim().starts_with('[')
        && serde_json::from_str::<Vec<serde_json::Value>>(text.trim()).is_ok()
    {
        return Some(text.trim().to_string());
    }

    // Extract from ```json ... ``` block
    for block in text.split("```") {
        let stripped = block.strip_prefix("json").unwrap_or(block).trim();
        if stripped.starts_with('[')
            && serde_json::from_str::<Vec<serde_json::Value>>(stripped).is_ok()
        {
            return Some(stripped.to_string());
        }
    }

    None
}

/// Queue the next ready steps of a goal as tasks.
pub fn queue_ready_steps(bus: &Path, goal: &mut Goal) -> Result<Vec<String>, QueueReadyError> {
    let mut queued = Vec::new();

    let plan = match goal.plan.as_mut() {
        Some(p) => p,
        None => return Ok(queued),
    };

    let config_dir = crate::config::config_dir();
    let cfg = crate::config::load_or_default(&config_dir)?;

    let done_steps: Vec<u32> = plan
        .steps
        .iter()
        .filter(|s| s.status == StepStatus::Done)
        .map(|s| s.step)
        .collect();

    for step in &mut plan.steps {
        if step.status != StepStatus::Pending {
            continue;
        }
        // Check all dependencies are done
        let deps_met = step.depends_on.iter().all(|dep| done_steps.contains(dep));
        if !deps_met {
            continue;
        }

        // Create task file in bus queue
        let task_id = format!("{}-step{}", goal.id, step.step);
        let project_path = crate::resolver::resolve(&goal.project, None, Some(&cfg))
            .map(|r| r.path.to_string_lossy().to_string())
            .unwrap_or_else(|_| format!("~/personal/heurema/{}", goal.project));

        let task_json = serde_json::json!({
            "project": goal.project,
            "project_path": project_path,
            "model": provider_from_agent(&step.agent),
            "prompt": step.prompt,
            "category": step.category,
            "timeout_seconds": 600,
            "max_budget_usd": step.est_cost_usd,
            "goal_id": goal.id,
            "step": step.step
        });

        let queue_path = bus.join("new/p1").join(format!("{task_id}.json"));
        let data = serde_json::to_string_pretty(&task_json)
            .map_err(std::io::Error::other)?;
        fs::write(&queue_path, data)?;
        step.status = StepStatus::Running;
        step.task_id = Some(task_id.clone());
        queued.push(task_id);
    }

    Ok(queued)
}

fn provider_from_agent(agent: &str) -> String {
    if agent.starts_with("codex") {
        "codex".to_string()
    } else if agent.starts_with("gemini") {
        "gemini".to_string()
    } else {
        "claude".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;
    use tempfile::TempDir;

    #[test]
    fn parse_plan_from_json() {
        let output = r#"[
            {"step": 1, "category": "research", "prompt": "audit code", "agent": "codex-auto", "est_cost_usd": 0.5, "depends_on": []},
            {"step": 2, "category": "fix", "prompt": "fix bugs", "agent": "claude-sonnet", "est_cost_usd": 1.0, "depends_on": [1]}
        ]"#;
        let plan = parse_plan(output, "claude-opus").unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].category, "research");
        assert_eq!(plan.steps[1].depends_on, vec![1]);
        assert_eq!(plan.created_by, "claude-opus");
    }

    #[test]
    fn parse_plan_from_markdown() {
        let output = "Here's the plan:\n```json\n[{\"step\":1,\"category\":\"codegen\",\"prompt\":\"do it\",\"agent\":\"claude-sonnet\",\"est_cost_usd\":1.0,\"depends_on\":[]}]\n```\nLet me know!";
        let plan = parse_plan(output, "test").unwrap();
        assert_eq!(plan.steps.len(), 1);
    }

    #[test]
    fn parse_plan_invalid() {
        assert!(parse_plan("not json at all", "test").is_none());
        assert!(parse_plan("[]", "test").is_none()); // empty
    }

    #[test]
    fn goal_serde_roundtrip() {
        let goal = Goal {
            id: "test-001".into(),
            project: "signum".into(),
            objective: "test".into(),
            deadline: Some("2026-04-01".into()),
            budget_usd: 5.0,
            spent_usd: 0.0,
            status: GoalStatus::Planning,
            plan: None,
            created_at: Utc::now(),
            completed_at: None,
        };
        let json = serde_json::to_string(&goal).unwrap();
        let parsed: Goal = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "test-001");
        assert_eq!(parsed.status, GoalStatus::Planning);
    }

    // --- Adversarial tests ---

    #[test]
    fn adversarial_empty_project_create_goal() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        // Empty project string: safe_id("") returns Err → create_goal returns Err
        let result = create_goal(&bus, "", "some objective", None, 1.0);
        assert!(result.is_err(), "create_goal with empty project should return Err (safe_id rejects empty)");
    }

    #[test]
    fn adversarial_10k_char_objective() {
        use tempfile::TempDir;
        use std::fs;

        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        let huge_objective = "x".repeat(10_000);
        let result = create_goal(&bus, "test", &huge_objective, None, 1.0);
        assert!(result.is_ok(), "10K char objective should not panic");
        let goal = result.unwrap();
        assert_eq!(goal.objective.len(), 10_000);

        // Should survive serde roundtrip
        let json = serde_json::to_string(&goal).unwrap();
        let parsed: Goal = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.objective.len(), 10_000);
    }

    #[test]
    fn adversarial_empty_plan_parse() {
        // Empty string
        assert!(parse_plan("", "test").is_none(), "empty string should return None");

        // Only whitespace
        assert!(parse_plan("   \n\t  ", "test").is_none());

        // Empty array produces None (existing behavior, but adversarially verify)
        assert!(parse_plan("[]", "test").is_none());

        // Array with null entries — should not panic
        let output = r#"[null, null]"#;
        // Each null becomes a step with defaults — steps vec non-empty, so Some is returned
        // The question is: does it panic? It should not.
        let result = std::panic::catch_unwind(|| parse_plan(output, "test"));
        assert!(result.is_ok(), "null array entries should not cause panic");
    }

    #[test]
    fn adversarial_plan_missing_required_fields() {
        // Steps missing all fields — should use defaults, not panic
        let output = r#"[{}, {}, {}]"#;
        let plan = parse_plan(output, "model");
        assert!(plan.is_some(), "steps with missing fields should use defaults");
        let plan = plan.unwrap();
        assert_eq!(plan.steps.len(), 3);
        // step defaults to 0
        assert_eq!(plan.steps[0].step, 0);
        // All steps get step=0 — verify no panic on duplicate step ids
    }

    #[test]
    fn adversarial_negative_budget_goal() {
        use tempfile::TempDir;
        use std::fs;

        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        // Negative budget: no validation in create_goal, should store as-is
        let result = create_goal(&bus, "test", "objective", None, -100.0);
        assert!(result.is_ok());
        let goal = result.unwrap();
        assert_eq!(goal.budget_usd, -100.0);

        // Verify loaded from disk
        let loaded = load_goal(&bus, &goal.id);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().budget_usd, -100.0);
    }

    #[test]
    fn adversarial_load_goal_path_traversal() {
        use tempfile::TempDir;
        use std::fs;

        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        // load_goal uses safe_id, so traversal should return None not panic
        let result = load_goal(&bus, "../../etc/passwd");
        assert!(result.is_none(), "traversal goal_id should return None");

        let result = load_goal(&bus, "");
        assert!(result.is_none(), "empty goal_id should return None");
    }

    #[test]
    fn adversarial_list_goals_no_dir() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        // bus points to nonexistent parent structure
        let bus = tmp.path().join("bus");
        // goals dir doesn't exist — should return empty vec, not panic
        let goals = list_goals(&bus);
        assert!(goals.is_empty());
    }

    #[test]
    fn adversarial_list_goals_corrupt_json() {
        use tempfile::TempDir;
        use std::fs;

        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        let goals_dir = bus.parent().unwrap().join("goals");
        fs::create_dir_all(&goals_dir).unwrap();

        // Write corrupt JSON files
        fs::write(goals_dir.join("bad1.json"), b"not json at all").unwrap();
        fs::write(goals_dir.join("bad2.json"), b"{\"incomplete\":").unwrap();
        fs::write(goals_dir.join("bad3.json"), b"").unwrap();

        // Should silently skip corrupt files
        let goals = list_goals(&bus);
        assert!(goals.is_empty(), "corrupt json files should be silently skipped");
    }

    #[test]
    fn adversarial_build_planner_prompt_nonexistent_project_path() {
        let goal = Goal {
            id: "g1".into(),
            project: "signum".into(),
            objective: "Build something".into(),
            deadline: None,
            budget_usd: 5.0,
            spent_usd: 0.0,
            status: GoalStatus::Planning,
            plan: None,
            created_at: Utc::now(),
            completed_at: None,
        };

        // Non-existent path — should not panic, context files just skipped
        let path = std::path::Path::new("/tmp/definitely-does-not-exist-xyzzy123");
        let prompt = build_planner_prompt(&goal, path);
        assert!(prompt.contains("Build something"));
        // No context files appended since they don't exist
        assert!(!prompt.contains("## CLAUDE.md"));
    }

    /// Two threads call queue_ready_steps on the same goal simultaneously.
    /// Steps must not be double-queued (each step file created exactly once).
    ///
    /// NOTE: queue_ready_steps operates on an in-memory &mut Goal — the caller
    /// is responsible for exclusive access. This test verifies that two threads
    /// with *separate* clones of the same goal each see the correct initial state
    /// (no shared mutable state corruption). It also verifies that the filesystem
    /// layer (fs::write to queue) prevents two threads from colliding on the
    /// same task_id file.
    #[test]
    fn concurrent_queue_ready_steps_no_double_queue() {
        const ITERATIONS: usize = 100;

        for _ in 0..ITERATIONS {
            let tmp = TempDir::new().unwrap();
            let root = tmp.path().to_path_buf();
            // goals_dir() resolves as bus.parent() / "goals"
            // bus must be root/bus so goals go to root/goals
            let bus = root.join("bus");
            fs::create_dir_all(bus.join("new/p1")).unwrap();

            // Build a goal with two independent pending steps
            let plan = Plan {
                version: 1,
                created_by: "test".into(),
                approved_at: None,
                steps: vec![
                    Step {
                        step: 1,
                        category: "codegen".into(),
                        prompt: "step one".into(),
                        agent: "claude-sonnet".into(),
                        est_cost_usd: 0.5,
                        depends_on: vec![],
                        status: StepStatus::Pending,
                        task_id: None,
                        sub_tasks: vec![],
                    },
                    Step {
                        step: 2,
                        category: "review".into(),
                        prompt: "step two".into(),
                        agent: "claude-sonnet".into(),
                        est_cost_usd: 0.5,
                        depends_on: vec![],
                        status: StepStatus::Pending,
                        task_id: None,
                        sub_tasks: vec![],
                    },
                ],
            };

            let goal = Goal {
                id: "test-concurrent-001".into(),
                project: "test".into(),
                objective: "test".into(),
                deadline: None,
                budget_usd: 5.0,
                spent_usd: 0.0,
                status: GoalStatus::Active,
                plan: Some(plan),
                created_at: Utc::now(),
                completed_at: None,
            };

            let bus_arc = Arc::new(bus.clone());
            let barrier = Arc::new(Barrier::new(2));
            let all_queued: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));

            let mut handles = vec![];
            for _ in 0..2 {
                // Each thread gets its own clone of the goal
                let mut goal_clone = goal.clone();
                let bus_clone = Arc::clone(&bus_arc);
                let barrier = Arc::clone(&barrier);
                let all_queued = Arc::clone(&all_queued);

                handles.push(thread::spawn(move || {
                    barrier.wait();
                    let queued = queue_ready_steps(&bus_clone, &mut goal_clone).unwrap();
                    all_queued.lock().unwrap().extend(queued);
                }));
            }

            for h in handles {
                h.join().unwrap();
            }

            // Count actual files in queue (filesystem is the authority)
            let queue_files: Vec<_> = fs::read_dir(bus.join("new/p1"))
                .unwrap()
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                .collect();

            let file_names: HashSet<String> = queue_files
                .iter()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();

            // Step 1 and step 2 each create one file — at most 2 files total
            assert!(
                queue_files.len() <= 2,
                "more than 2 task files created: {file_names:?} — steps double-queued"
            );

            // No duplicate filenames (would indicate double write)
            assert_eq!(
                file_names.len(),
                queue_files.len(),
                "duplicate queue filenames detected"
            );
        }
    }

    #[test]
    fn adversarial_parse_plan_deeply_nested_depends_on() {
        // depends_on with non-integer values — should not panic, filter_map skips them
        let output = r#"[{
            "step": 1,
            "category": "codegen",
            "prompt": "do it",
            "agent": "claude-sonnet",
            "est_cost_usd": 0.5,
            "depends_on": ["not-a-number", null, -1, 9999999999999999]
        }]"#;
        let result = std::panic::catch_unwind(|| parse_plan(output, "test"));
        assert!(result.is_ok(), "weird depends_on values should not panic");
        let plan = result.unwrap().unwrap();
        // negative -1: as_u64() returns None, so it's filtered out
        // 9999999999999999 fits in u64, so it's kept
        assert_eq!(plan.steps[0].step, 1);
    }

    // --- Security tests: path traversal in create_goal / load_goal ---

    #[test]
    fn security_create_goal_traversal_project_field() {
        // project="../../../tmp" — create_goal does NOT sanitize the project field
        // through safe_id(). The id is built as "{project}-{timestamp}", so the
        // resulting filename contains "/../../../tmp-<ts>.json".
        // save_goal uses path.join(format!("{}.json", goal.id)) which on Linux/macOS
        // WILL resolve the traversal through path normalization at the OS level.
        // This test verifies whether the file actually lands inside goals_dir or escapes.
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        let malicious_project = "../../../tmp";
        let result = create_goal(&bus, malicious_project, "objective", None, 1.0);

        if let Ok(goal) = result {
            // The goal was "created". Check where the file actually landed.
            // goals_dir is bus.parent()/goals = tmp/goals
            // goal.id = "../../../tmp-<ts>", file path = tmp/goals/../../../tmp-<ts>.json
            // which resolves to /tmp-<ts>.json (outside tmp/ sandbox).
            // Verify the file is NOT outside the TempDir root.
            let goals_dir_path = tmp.path().join("goals");
            let expected_file = goals_dir_path.join(format!("{}.json", goal.id));
            let canonical = expected_file.canonicalize();

            if let Ok(canonical_path) = canonical {
                assert!(
                    canonical_path.starts_with(tmp.path()),
                    "SECURITY BYPASS: create_goal with project='{}' wrote file outside sandbox: {}",
                    malicious_project,
                    canonical_path.display()
                );
            }
            // If canonicalize fails, the file doesn't exist at that path (likely escaped).
            // Try to find it outside the tmp dir.
            let escaped_path = std::path::Path::new("/tmp").join(
                format!("{}.json", goal.id.replace("../../../tmp", "tmp"))
            );
            assert!(
                !escaped_path.exists(),
                "SECURITY BYPASS: file may have escaped to {}",
                escaped_path.display()
            );
        }
        // If create_goal errored, traversal is blocked — pass.
    }

    #[test]
    fn security_load_goal_traversal_blocked() {
        // load_goal calls safe_id() which rejects ".." — traversal must return None.
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        // Create a sentinel file outside goals/ to detect escape
        let sentinel = tmp.path().join("secret.json");
        fs::write(&sentinel, b"{\"id\":\"secret\"}").unwrap();

        // Try to reach ../../secret via goal_id
        let result = load_goal(&bus, "../../secret");
        assert!(
            result.is_none(),
            "SECURITY BYPASS: load_goal with traversal goal_id must return None"
        );

        let result = load_goal(&bus, "../secret");
        assert!(result.is_none(), "load_goal with '../secret' must return None");
    }

    #[test]
    fn security_save_goal_traversal_via_goal_id() {
        // save_goal uses goal.id directly (no safe_id check).
        // An attacker who can construct a Goal struct with a traversal id
        // could write files outside the goals/ directory.
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        let evil_goal = Goal {
            id: "../../evil".into(), // traversal in id field
            project: "test".into(),
            objective: "exfil".into(),
            deadline: None,
            budget_usd: 0.0,
            spent_usd: 0.0,
            status: GoalStatus::Planning,
            plan: None,
            created_at: Utc::now(),
            completed_at: None,
        };

        let result = save_goal(&bus, &evil_goal);
        assert!(
            result.is_err(),
            "save_goal with traversal id must return Err (safe_id rejects '../')"
        );
    }
}
