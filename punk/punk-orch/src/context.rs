use std::fs;
use std::path::Path;

use crate::goal;
use crate::ratchet;
use crate::recall;
use crate::session;
use crate::skill;

/// Unified context assembled before agent dispatch.
/// Combines: agent guidance + skills + recall + session + project stats.
pub struct ContextPack {
    pub sections: Vec<String>,
}

impl ContextPack {
    /// Build a unified context for a task dispatch.
    pub fn build(
        bus: &Path,
        project: &str,
        category: &str,
        agent_id: &str,
        config_dir: &Path,
    ) -> Self {
        let mut sections = Vec::new();

        // 1. Agent Guidance (from agents.toml system_prompt)
        if let Some(guidance) = load_agent_guidance(config_dir, agent_id) {
            sections.push(guidance);
        }

        // 2. Auto-triggered Skills (match by category + project)
        let matched_skills = match_skills(bus, project, category);
        if !matched_skills.is_empty() {
            sections.push(matched_skills);
        }

        // 3. Recall (past failures relevant to this project)
        let recall_events = recall::recall(bus, project, Some(project), 3);
        let recall_section = recall::format_recall(&recall_events);
        if !recall_section.is_empty() {
            sections.push(recall_section);
        }

        // 4. Session context (recent task results)
        let session_ctx = session::load(bus, project);
        let session_section = session::format_for_prompt(&session_ctx);
        if !session_section.is_empty() {
            sections.push(session_section);
        }

        // 5. Project stats (compact)
        let stats = project_stats(bus, project);
        if !stats.is_empty() {
            sections.push(stats);
        }

        // 6. Latest run triage for fix/goal/planning flows.
        if matches!(category, "fix" | "goal" | "plan" | "planning") {
            let triage = crate::run::latest_run_triage(bus, project);
            if triage.verdict != crate::run::TriageVerdict::NoActiveRun {
                sections.push(format_latest_run_triage(&triage));
            }
        }

        Self { sections }
    }

    /// Format as a single prefix block for prompt injection.
    pub fn format(&self) -> String {
        if self.sections.is_empty() {
            return String::new();
        }
        let joined = self.sections.join("\n");
        format!("{joined}\n---\n\n")
    }

    /// Is there any context to inject?
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

/// Load agent guidance from system_prompt file in agents.toml.
fn load_agent_guidance(config_dir: &Path, agent_id: &str) -> Option<String> {
    let cfg = crate::config::load_or_default(config_dir).ok()?;
    let agent = cfg.agents.agents.get(agent_id)?;
    let prompt_path = agent.system_prompt.as_ref()?;

    // Resolve path: relative to config_dir or absolute
    let full_path = if prompt_path.starts_with('/') {
        std::path::PathBuf::from(prompt_path)
    } else {
        config_dir.join(prompt_path)
    };

    fs::read_to_string(full_path).ok()
}

/// Find skills that match the current task's category + project.
fn match_skills(bus: &Path, project: &str, category: &str) -> String {
    let skills = skill::list_skills(bus);
    let mut matched = Vec::new();

    for s in &skills {
        if let Ok(content) = fs::read_to_string(&s.path) {
            // Parse triggers from frontmatter
            if let Some(triggers) = extract_triggers(&content) {
                let cat_match = triggers.categories.is_empty()
                    || triggers.categories.iter().any(|c| c == category);
                let proj_match =
                    triggers.projects.is_empty() || triggers.projects.iter().any(|p| p == project);

                if cat_match && proj_match {
                    // Extract body (after frontmatter)
                    let body = content
                        .strip_prefix("---")
                        .and_then(|rest| rest.find("---").map(|i| &rest[i + 3..]))
                        .unwrap_or(&content)
                        .trim();
                    if !body.is_empty() {
                        matched.push(format!("### Skill: {}\n{}", s.name, body));
                    }
                }
            }
        }
    }

    if matched.is_empty() {
        return String::new();
    }

    format!("## Applied Skills\n\n{}\n", matched.join("\n\n"))
}

struct SkillTriggers {
    categories: Vec<String>,
    projects: Vec<String>,
}

fn extract_triggers(content: &str) -> Option<SkillTriggers> {
    let rest = content.strip_prefix("---")?;
    let end = rest.find("---")?;
    let frontmatter = &rest[..end];

    let mut categories = Vec::new();
    let mut projects = Vec::new();

    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("category:") {
            categories = parse_yaml_list(val);
        } else if let Some(val) = line.strip_prefix("project:") {
            projects = parse_yaml_list(val);
        }
    }

    Some(SkillTriggers {
        categories,
        projects,
    })
}

fn parse_yaml_list(val: &str) -> Vec<String> {
    let trimmed = val.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        trimmed[1..trimmed.len() - 1]
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        vec![trimmed.to_string()]
    }
}

/// Compact project stats for context.
fn project_stats(bus: &Path, project: &str) -> String {
    let metrics = ratchet::compute_metrics(bus, 7);
    let goals = goal::list_goals(bus);
    let active_goals: Vec<_> = goals
        .iter()
        .filter(|g| g.project == project && g.status == goal::GoalStatus::Active)
        .collect();

    if metrics.total_tasks == 0 && active_goals.is_empty() {
        return String::new();
    }

    let mut out = format!("## Project: {} (7d)\n", project);
    if metrics.total_tasks > 0 {
        out.push_str(&format!(
            "- {} tasks ({} ok, {} fail), ${:.2}, {:.0}% success\n",
            metrics.total_tasks,
            metrics.success_count,
            metrics.failure_count,
            metrics.total_cost_usd,
            metrics.success_rate_pct
        ));
    }
    for g in &active_goals {
        if let Some(ref plan) = g.plan {
            let done = plan
                .steps
                .iter()
                .filter(|s| s.status == goal::StepStatus::Done)
                .count();
            out.push_str(&format!(
                "- Goal: {} ({}/{} steps)\n",
                g.objective,
                done,
                plan.steps.len()
            ));
        }
    }
    out
}

/// CLI command: punk-run context <project>
pub fn format_context_report(bus: &Path, project: &str, config_dir: &Path) -> String {
    let pack = ContextPack::build(bus, project, "", "", config_dir);
    if pack.is_empty() {
        return format!("No context for project: {project}\n");
    }
    pack.sections.join("\n")
}

fn format_latest_run_triage(triage: &crate::run::RunTriage) -> String {
    let mut out = String::from("## Latest run triage\n");
    out.push_str(&format!("- Verdict: {:?}\n", triage.verdict));
    if !triage.run_id.is_empty() {
        out.push_str(&format!("- Run: {}\n", triage.run_id));
    }
    if let Some(ref status) = triage.status {
        out.push_str(&format!("- Status: {:?}\n", status));
    }
    if let Some(age_s) = triage.age_s {
        out.push_str(&format!("- Age: {}s\n", age_s));
    }
    if let Some(heartbeat_age_s) = triage.heartbeat_age_s {
        out.push_str(&format!("- Heartbeat age: {}s\n", heartbeat_age_s));
    }
    if !triage.stdout_tail.is_empty() {
        out.push_str(&format!(
            "- Stdout tail: {}\n",
            triage.stdout_tail.replace('\n', " ")
        ));
    }
    if !triage.stderr_tail.is_empty() {
        out.push_str(&format!(
            "- Stderr tail: {}\n",
            triage.stderr_tail.replace('\n', " ")
        ));
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::tempdir;

    #[test]
    fn fix_context_includes_latest_run_triage() {
        let bus_dir = tempdir().unwrap();
        let cfg_dir = tempdir().unwrap();
        let bus = bus_dir.path();
        let task_id = "specpunk-20260331-120003";

        fs::create_dir_all(bus.join("cur")).unwrap();
        fs::write(
            bus.join("cur").join(format!("{task_id}.json")),
            serde_json::json!({
                "project": "specpunk",
                "model": "claude",
                "category": "fix"
            })
            .to_string(),
        )
        .unwrap();

        fs::create_dir_all(bus.join("runs").join(task_id)).unwrap();
        let run = crate::run::Run {
            run_id: format!("{task_id}-1"),
            task_id: task_id.to_string(),
            attempt: 1,
            retry_of: None,
            slot_id: 1,
            agent: "claude".to_string(),
            model: "sonnet".to_string(),
            invoke_tier: crate::run::InvokeTier::Cli,
            status: crate::run::RunStatus::Running,
            error_type: None,
            termination_reason: None,
            claimed_at: Utc::now(),
            started_at: Some(Utc::now()),
            finished_at: None,
            duration_ms: 0,
            exit_code: 0,
            pid: Some(1),
            stdout_path: None,
            stderr_path: None,
        };
        fs::write(
            bus.join("runs").join(task_id).join("run-1.json"),
            serde_json::to_string_pretty(&run).unwrap(),
        )
        .unwrap();
        fs::create_dir_all(bus.join(".heartbeats")).unwrap();
        fs::write(bus.join(".heartbeats").join(format!("{task_id}.hb")), "").unwrap();

        let pack = ContextPack::build(bus, "specpunk", "fix", "claude", cfg_dir.path());
        let joined = pack.sections.join("\n");
        assert!(joined.contains("## Latest run triage"));
        assert!(joined.contains("StillAlive") || joined.contains("Still Alive"));
    }
}
