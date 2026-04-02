use std::path::Path;
use std::process::Command;
use std::time::Instant;

use punk_core::vcs::{detect_mode as detect_vcs_mode, VcsMode};

/// Provider health check result.
#[derive(Debug)]
pub struct ProviderHealth {
    pub name: String,
    pub binary_found: bool,
    pub version: String,
    pub auth_ok: bool,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

/// Run health checks on all providers + bus.
pub fn check_all(bus_path: &Path, config_dir: &Path, repo_path: &Path) -> HealthReport {
    let providers = vec![
        check_provider("claude", &["--version"]),
        check_provider("codex", &["--version"]),
        check_provider("gemini", &["--version"]),
    ];
    let cfg = crate::config::load_or_default(config_dir).ok();
    let queue_state = crate::bus::read_state(bus_path, 10);

    let bus_ok = bus_path.join("new").is_dir() && bus_path.join("cur").is_dir();
    let status = crate::config::config_status(config_dir);
    // Config is always valid now (defaults work at L0). Report detail level.
    let config_ok = true; // defaults are valid
    let config_detail = if status.is_complete() {
        "complete".to_string()
    } else if status.is_empty() {
        "defaults".to_string()
    } else {
        format!("partial ({}/3)", status.present_count())
    };

    let slots_dir = bus_path.join(".slots");
    let occupied_slots = std::fs::read_dir(&slots_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .count() as u32;

    let vcs_mode = detect_vcs_mode(repo_path);

    HealthReport {
        providers,
        bus_ok,
        bus_path: bus_path.to_string_lossy().to_string(),
        config_ok,
        config_detail,
        config_path: config_dir.to_string_lossy().to_string(),
        known_projects: crate::resolver::list_known(cfg.as_ref()).len(),
        default_queue_agent: cfg.as_ref().and_then(preferred_queue_agent),
        queued_count: queue_state.queued.len(),
        running_count: queue_state.running.len(),
        done_count: queue_state.done.len(),
        failed_count: queue_state.failed.len(),
        occupied_slots,
        vcs_mode,
    }
}

#[derive(Debug)]
pub struct HealthReport {
    pub providers: Vec<ProviderHealth>,
    pub bus_ok: bool,
    pub bus_path: String,
    pub config_ok: bool,
    pub config_detail: String,
    pub config_path: String,
    pub known_projects: usize,
    pub default_queue_agent: Option<String>,
    pub queued_count: usize,
    pub running_count: usize,
    pub done_count: usize,
    pub failed_count: usize,
    pub occupied_slots: u32,
    pub vcs_mode: VcsMode,
}

impl HealthReport {
    pub fn display(&self) -> String {
        let mut out = String::new();
        out.push_str("punk-run doctor\n\n");

        out.push_str(&format!(
            "{:<10} {:<10} {:<20} {}\n",
            "PROVIDER", "BINARY", "VERSION", "STATUS"
        ));
        for p in &self.providers {
            let binary = if p.binary_found { "found" } else { "MISSING" };
            let status = if !p.binary_found {
                "not installed".to_string()
            } else if let Some(ref err) = p.error {
                err.clone()
            } else {
                match p.latency_ms {
                    Some(ms) => format!("ok ({}ms)", ms),
                    None => "ok".to_string(),
                }
            };
            out.push_str(&format!(
                "{:<10} {:<10} {:<20} {}\n",
                p.name, binary, p.version, status
            ));
        }
        out.push('\n');

        let bus_status = if self.bus_ok { "ok" } else { "NOT FOUND" };
        out.push_str(&format!("Bus:    {} ({})\n", bus_status, self.bus_path));
        let cfg_status = if self.config_ok { "ok" } else { "INCOMPLETE" };
        out.push_str(&format!(
            "Config: {} ({}, {})\n",
            cfg_status, self.config_detail, self.config_path
        ));
        out.push_str(&format!("Projects: {} known\n", self.known_projects));
        match &self.default_queue_agent {
            Some(agent) => out.push_str(&format!("Queue:  default agent = {agent}\n")),
            None => out.push_str("Queue:  default agent = unavailable\n"),
        }
        out.push_str(&format!(
            "Tasks:  queued={} running={} done={} failed={}\n",
            self.queued_count, self.running_count, self.done_count, self.failed_count
        ));
        out.push_str(&format!("VCS:    {}\n", format_vcs_mode(self.vcs_mode)));
        out.push_str(&format!("Slots:  {}/5 occupied\n\n", self.occupied_slots));

        let healthy = self.providers.iter().filter(|p| p.binary_found).count();
        let total = self.providers.len();
        if healthy == total && self.bus_ok && self.config_ok && !vcs_mode_is_degraded(self.vcs_mode)
        {
            out.push_str("Overall: HEALTHY\n");
        } else {
            out.push_str(&format!(
                "Overall: DEGRADED ({healthy}/{total} providers)\n"
            ));
            for p in &self.providers {
                if !p.binary_found {
                    let url = match p.name.as_str() {
                        "codex" => "https://github.com/openai/codex",
                        "gemini" => "https://github.com/google-gemini/gemini-cli",
                        "claude" => "https://claude.ai/download",
                        _ => "",
                    };
                    out.push_str(&format!("  {}: install from {url}\n", p.name));
                }
            }
            if !self.config_ok {
                out.push_str("  config: punk-run init  (generate from environment)\n");
            }
            if self.vcs_mode == VcsMode::GitWithJjAvailableButDisabled {
                out.push_str("  vcs: punk-run vcs enable-jj  (enable fuller punk functionality)\n");
            }
        }

        out
    }
}

fn format_vcs_mode(mode: VcsMode) -> &'static str {
    match mode {
        VcsMode::Jj => "jj",
        VcsMode::GitOnly => "git-only",
        VcsMode::GitWithJjAvailableButDisabled => "git-only (degraded; jj available but disabled)",
        VcsMode::NoVcs => "no VCS detected",
    }
}

fn vcs_mode_is_degraded(mode: VcsMode) -> bool {
    matches!(
        mode,
        VcsMode::GitWithJjAvailableButDisabled | VcsMode::NoVcs
    )
}

fn preferred_queue_agent(cfg: &crate::config::Config) -> Option<String> {
    let agents = &cfg.agents.agents;

    for preferred in ["claude", "codex", "gemini"] {
        if agents.contains_key(preferred) {
            return Some(preferred.to_string());
        }
    }

    for preferred_provider in ["claude", "codex", "gemini"] {
        let mut matching: Vec<_> = agents
            .iter()
            .filter(|(_, agent)| agent.provider == preferred_provider)
            .map(|(id, _)| id.clone())
            .collect();
        matching.sort();
        if let Some(id) = matching.into_iter().next() {
            return Some(id);
        }
    }

    let mut fallback_ids: Vec<_> = agents.keys().cloned().collect();
    fallback_ids.sort();
    fallback_ids.into_iter().next()
}

fn check_provider(name: &str, version_args: &[&str]) -> ProviderHealth {
    let which = Command::new("which").arg(name).output();

    let binary_found = which.as_ref().map(|o| o.status.success()).unwrap_or(false);

    if !binary_found {
        return ProviderHealth {
            name: name.to_string(),
            binary_found: false,
            version: String::new(),
            auth_ok: false,
            latency_ms: None,
            error: None,
        };
    }

    let start = Instant::now();
    let version_out = Command::new(name).args(version_args).output();
    let latency = start.elapsed().as_millis() as u64;

    let version = version_out
        .as_ref()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .unwrap_or_default();

    ProviderHealth {
        name: name.to_string(),
        binary_found: true,
        version,
        auth_ok: true, // version check doesn't test auth, full smoke test is Phase 3
        latency_ms: Some(latency),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        Agent, AgentsFile, BudgetPolicy, Config, PolicyDefaults, PolicyFile, Project, ProjectsFile,
    };
    use std::collections::HashMap;

    fn sample_provider(name: &str) -> ProviderHealth {
        ProviderHealth {
            name: name.to_string(),
            binary_found: true,
            version: "1.0".to_string(),
            auth_ok: true,
            latency_ms: Some(1),
            error: None,
        }
    }

    fn config_with_agents(agents: &[(&str, &str, &str)]) -> Config {
        let mut map = HashMap::new();
        for (id, provider, model) in agents {
            map.insert(
                (*id).to_string(),
                Agent {
                    provider: (*provider).to_string(),
                    model: (*model).to_string(),
                    role: "engineer".into(),
                    invoke: "cli".into(),
                    budget_usd: 1.0,
                    system_prompt: None,
                    skills: vec![],
                },
            );
        }
        Config {
            projects: ProjectsFile {
                projects: vec![Project {
                    id: "demo".into(),
                    path: "/tmp/demo".into(),
                    stack: String::new(),
                    active: true,
                    budget_usd: 0.0,
                    checkpoint: String::new(),
                }],
            },
            agents: AgentsFile { agents: map },
            policy: PolicyFile {
                defaults: PolicyDefaults {
                    model: "sonnet".into(),
                    budget_usd: 1.0,
                    timeout_s: 600,
                    max_slots: 1,
                },
                budget: BudgetPolicy::default(),
                rules: vec![],
                features: HashMap::new(),
            },
            dir: "/tmp/config".into(),
        }
    }

    #[test]
    fn doctor_display_shows_degraded_vcs_hint() {
        let report = HealthReport {
            providers: vec![
                sample_provider("claude"),
                sample_provider("codex"),
                sample_provider("gemini"),
            ],
            bus_ok: true,
            bus_path: "/tmp/bus".to_string(),
            config_ok: true,
            config_detail: "complete".to_string(),
            config_path: "/tmp/config".to_string(),
            known_projects: 1,
            default_queue_agent: Some("claude".to_string()),
            queued_count: 0,
            running_count: 0,
            done_count: 0,
            failed_count: 0,
            occupied_slots: 0,
            vcs_mode: VcsMode::GitWithJjAvailableButDisabled,
        };

        let rendered = report.display();
        assert!(rendered.contains("VCS:    git-only (degraded; jj available but disabled)"));
        assert!(rendered.contains("punk-run vcs enable-jj"));
        assert!(rendered.contains("Overall: DEGRADED"));
    }

    #[test]
    fn doctor_display_reports_healthy_with_jj_mode() {
        let report = HealthReport {
            providers: vec![
                sample_provider("claude"),
                sample_provider("codex"),
                sample_provider("gemini"),
            ],
            bus_ok: true,
            bus_path: "/tmp/bus".to_string(),
            config_ok: true,
            config_detail: "complete".to_string(),
            config_path: "/tmp/config".to_string(),
            known_projects: 1,
            default_queue_agent: Some("claude".to_string()),
            queued_count: 0,
            running_count: 0,
            done_count: 0,
            failed_count: 0,
            occupied_slots: 0,
            vcs_mode: VcsMode::Jj,
        };

        let rendered = report.display();
        assert!(rendered.contains("VCS:    jj"));
        assert!(rendered.contains("Overall: HEALTHY"));
    }

    #[test]
    fn doctor_display_shows_default_queue_agent_and_project_count() {
        let report = HealthReport {
            providers: vec![sample_provider("codex")],
            bus_ok: true,
            bus_path: "/tmp/bus".to_string(),
            config_ok: true,
            config_detail: "defaults".to_string(),
            config_path: "/tmp/config".to_string(),
            known_projects: 3,
            default_queue_agent: Some("codex".to_string()),
            queued_count: 2,
            running_count: 1,
            done_count: 4,
            failed_count: 1,
            occupied_slots: 0,
            vcs_mode: VcsMode::Jj,
        };

        let rendered = report.display();
        assert!(rendered.contains("Projects: 3 known"));
        assert!(rendered.contains("Queue:  default agent = codex"));
        assert!(rendered.contains("Tasks:  queued=2 running=1 done=4 failed=1"));
    }

    #[test]
    fn doctor_display_shows_unavailable_default_queue_agent() {
        let report = HealthReport {
            providers: vec![],
            bus_ok: true,
            bus_path: "/tmp/bus".to_string(),
            config_ok: true,
            config_detail: "defaults".to_string(),
            config_path: "/tmp/config".to_string(),
            known_projects: 0,
            default_queue_agent: None,
            queued_count: 0,
            running_count: 0,
            done_count: 0,
            failed_count: 0,
            occupied_slots: 0,
            vcs_mode: VcsMode::Jj,
        };

        let rendered = report.display();
        assert!(rendered.contains("Queue:  default agent = unavailable"));
    }

    #[test]
    fn preferred_queue_agent_prefers_supported_provider_order() {
        let cfg = config_with_agents(&[
            ("gemini", "gemini", "gemini-2.5-flash"),
            ("codex", "codex", "o4-mini"),
        ]);
        assert_eq!(preferred_queue_agent(&cfg).as_deref(), Some("codex"));
    }
}
