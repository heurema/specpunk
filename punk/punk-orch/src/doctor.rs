use std::path::Path;
use std::process::Command;
use std::time::Instant;

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
pub fn check_all(bus_path: &Path, config_dir: &Path) -> HealthReport {
    let providers = vec![
        check_provider("claude", &["--version"]),
        check_provider("codex", &["--version"]),
        check_provider("gemini", &["--version"]),
    ];

    let bus_ok = bus_path.join("new").is_dir() && bus_path.join("cur").is_dir();
    let config_ok = config_dir.join("projects.toml").is_file()
        && config_dir.join("agents.toml").is_file()
        && config_dir.join("policy.toml").is_file();

    let slots_dir = bus_path.join(".slots");
    let occupied_slots = std::fs::read_dir(&slots_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .count() as u32;

    HealthReport {
        providers,
        bus_ok,
        bus_path: bus_path.to_string_lossy().to_string(),
        config_ok,
        config_path: config_dir.to_string_lossy().to_string(),
        occupied_slots,
    }
}

#[derive(Debug)]
pub struct HealthReport {
    pub providers: Vec<ProviderHealth>,
    pub bus_ok: bool,
    pub bus_path: String,
    pub config_ok: bool,
    pub config_path: String,
    pub occupied_slots: u32,
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
        out.push_str(&format!("Config: {} ({})\n", cfg_status, self.config_path));
        out.push_str(&format!("Slots:  {}/5 occupied\n\n", self.occupied_slots));

        let healthy = self.providers.iter().filter(|p| p.binary_found).count();
        let total = self.providers.len();
        if healthy == total && self.bus_ok && self.config_ok {
            out.push_str("Overall: HEALTHY\n");
        } else {
            out.push_str(&format!("Overall: DEGRADED ({healthy}/{total} providers)\n"));
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
                out.push_str(&format!(
                    "  config: create files in {}\n",
                    self.config_path
                ));
            }
        }

        out
    }
}

fn check_provider(name: &str, version_args: &[&str]) -> ProviderHealth {
    let which = Command::new("which").arg(name).output();

    let binary_found = which
        .as_ref()
        .map(|o| o.status.success())
        .unwrap_or(false);

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
