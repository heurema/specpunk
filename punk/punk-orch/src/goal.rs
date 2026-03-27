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
pub fn queue_ready_steps(bus: &Path, goal: &mut Goal) -> Vec<String> {
    let mut queued = Vec::new();

    let plan = match goal.plan.as_mut() {
        Some(p) => p,
        None => return queued,
    };

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
        let task_json = serde_json::json!({
            "project": goal.project,
            "project_path": format!("~/personal/heurema/{}", goal.project),
            "model": provider_from_agent(&step.agent),
            "prompt": step.prompt,
            "category": step.category,
            "timeout_seconds": 600,
            "max_budget_usd": step.est_cost_usd,
            "goal_id": goal.id,
            "step": step.step
        });

        let queue_path = bus.join("new/p1").join(format!("{task_id}.json"));
        if let Ok(data) = serde_json::to_string_pretty(&task_json) {
            if fs::write(&queue_path, data).is_ok() {
                step.status = StepStatus::Running;
                step.task_id = Some(task_id.clone());
                queued.push(task_id);
            }
        }
    }

    queued
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
}
