use std::fs;
use std::path::Path;

use crate::recall;
use crate::session;
use crate::skill;
use crate::ratchet;
use crate::goal;

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
    let cfg = crate::config::load(config_dir).ok()?;
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
                let proj_match = triggers.projects.is_empty()
                    || triggers.projects.iter().any(|p| p == project);

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

    Some(SkillTriggers { categories, projects })
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
    let active_goals: Vec<_> = goals.iter().filter(|g| {
        g.project == project && g.status == goal::GoalStatus::Active
    }).collect();

    if metrics.total_tasks == 0 && active_goals.is_empty() {
        return String::new();
    }

    let mut out = format!("## Project: {} (7d)\n", project);
    if metrics.total_tasks > 0 {
        out.push_str(&format!(
            "- {} tasks ({} ok, {} fail), ${:.2}, {:.0}% success\n",
            metrics.total_tasks, metrics.success_count, metrics.failure_count,
            metrics.total_cost_usd, metrics.success_rate_pct
        ));
    }
    for g in &active_goals {
        if let Some(ref plan) = g.plan {
            let done = plan.steps.iter().filter(|s| s.status == goal::StepStatus::Done).count();
            out.push_str(&format!(
                "- Goal: {} ({}/{} steps)\n",
                g.objective, done, plan.steps.len()
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
