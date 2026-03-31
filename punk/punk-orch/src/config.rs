use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Resolve config directory: $PUNK_CONFIG_DIR or ~/.config/punk/
pub fn config_dir() -> PathBuf {
    std::env::var("PUNK_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config/punk")
        })
}

// --- Projects ---

#[derive(Debug, Deserialize)]
pub struct ProjectsFile {
    pub projects: Vec<Project>,
}

#[derive(Debug, Deserialize)]
pub struct Project {
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub stack: String,
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default)]
    pub budget_usd: f64,
    #[serde(default)]
    pub checkpoint: String,
}

// --- Agents ---

#[derive(Debug, Deserialize)]
pub struct AgentsFile {
    pub agents: HashMap<String, Agent>,
}

#[derive(Debug, Deserialize)]
pub struct Agent {
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub role: String,
    #[serde(default = "default_cli")]
    pub invoke: String,
    #[serde(default = "default_budget")]
    pub budget_usd: f64,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

// --- Policy ---

#[derive(Debug, Deserialize)]
pub struct PolicyFile {
    pub defaults: PolicyDefaults,
    #[serde(default)]
    pub budget: BudgetPolicy,
    #[serde(default)]
    pub rules: Vec<PolicyRule>,
    #[serde(default)]
    pub features: HashMap<String, toml::Value>,
}

#[derive(Debug, Deserialize)]
pub struct PolicyDefaults {
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_budget")]
    pub budget_usd: f64,
    #[serde(default = "default_timeout")]
    pub timeout_s: u64,
    #[serde(default = "default_slots")]
    pub max_slots: u32,
}

#[derive(Debug, Default, Deserialize)]
pub struct BudgetPolicy {
    #[serde(default = "default_ceiling")]
    pub monthly_ceiling_usd: f64,
    #[serde(default = "default_soft")]
    pub soft_alert_pct: u32,
    #[serde(default = "default_hard")]
    pub hard_stop_pct: u32,
}

#[derive(Debug, Deserialize)]
pub struct PolicyRule {
    #[serde(rename = "match")]
    pub match_criteria: HashMap<String, String>,
    pub set: HashMap<String, toml::Value>,
}

// --- Loader ---

#[derive(Debug)]
pub struct Config {
    pub projects: ProjectsFile,
    pub agents: AgentsFile,
    pub policy: PolicyFile,
    pub dir: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config dir not found: {0}")]
    DirNotFound(PathBuf),
    #[error("{file}: {source}")]
    ReadError {
        file: String,
        source: std::io::Error,
    },
    #[error("{file}: {source}")]
    ParseError {
        file: String,
        source: toml::de::Error,
    },
}

pub fn load(dir: &Path) -> Result<Config, ConfigError> {
    if !dir.is_dir() {
        return Err(ConfigError::DirNotFound(dir.to_path_buf()));
    }

    let projects = load_toml::<ProjectsFile>(dir, "projects.toml")?;
    let agents = load_toml::<AgentsFile>(dir, "agents.toml")?;
    let policy = load_toml::<PolicyFile>(dir, "policy.toml")?;

    Ok(Config {
        projects,
        agents,
        policy,
        dir: dir.to_path_buf(),
    })
}

fn load_toml<T: serde::de::DeserializeOwned>(dir: &Path, filename: &str) -> Result<T, ConfigError> {
    let path = dir.join(filename);
    let content = fs::read_to_string(&path).map_err(|e| ConfigError::ReadError {
        file: filename.to_string(),
        source: e,
    })?;
    toml::from_str(&content).map_err(|e| ConfigError::ParseError {
        file: filename.to_string(),
        source: e,
    })
}

fn load_optional_toml<T: serde::de::DeserializeOwned>(
    dir: &Path,
    filename: &str,
) -> Result<Option<T>, ConfigError> {
    let path = dir.join(filename);
    match fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content)
            .map(Some)
            .map_err(|e| ConfigError::ParseError {
                file: filename.to_string(),
                source: e,
            }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ConfigError::ReadError {
            file: filename.to_string(),
            source: e,
        }),
    }
}

// --- Zero-config fallbacks ---

/// Load config with graceful fallbacks for missing files, but fail explicitly
/// when an existing config file is unreadable or invalid.
/// L0: no files at all -> built-in defaults
/// L1: partial files -> loads what exists, defaults for rest
/// L2: full TOML -> current behavior
pub fn load_or_default(dir: &Path) -> Result<Config, ConfigError> {
    let projects = load_optional_toml::<ProjectsFile>(dir, "projects.toml")?
        .unwrap_or(ProjectsFile { projects: vec![] });

    let agents = load_optional_toml::<AgentsFile>(dir, "agents.toml")?
        .unwrap_or_else(detect_agents);

    let policy = load_optional_toml::<PolicyFile>(dir, "policy.toml")?
        .unwrap_or_else(default_policy);

    Ok(Config {
        projects,
        agents,
        policy,
        dir: dir.to_path_buf(),
    })
}

/// Detect available agents by checking which CLIs are in PATH.
pub fn detect_agents() -> AgentsFile {
    detect_agents_with(which_exists)
}

fn detect_agents_with<F>(mut exists: F) -> AgentsFile
where
    F: FnMut(&str) -> bool,
{
    let mut agents = HashMap::new();
    for (name, provider, model) in [
        ("claude", "claude", "sonnet"),
        ("codex", "codex", "o4-mini"),
        ("gemini", "gemini", "gemini-2.5-flash"),
    ] {
        if exists(name) {
            agents.insert(
                name.to_string(),
                Agent {
                    provider: provider.to_string(),
                    model: model.to_string(),
                    role: "engineer".to_string(),
                    invoke: "cli".to_string(),
                    budget_usd: default_budget(),
                    system_prompt: None,
                    skills: vec![],
                },
            );
        }
    }
    AgentsFile { agents }
}

/// Built-in safe policy defaults.
pub fn default_policy() -> PolicyFile {
    PolicyFile {
        defaults: PolicyDefaults {
            model: default_model(),
            budget_usd: default_budget(),
            timeout_s: default_timeout(),
            max_slots: default_slots(),
        },
        budget: BudgetPolicy {
            monthly_ceiling_usd: default_ceiling(),
            soft_alert_pct: default_soft(),
            hard_stop_pct: default_hard(),
        },
        rules: vec![],
        features: HashMap::new(),
    }
}

/// Report which config files are present.
#[derive(Debug)]
pub struct ConfigStatus {
    pub dir: PathBuf,
    pub has_projects: bool,
    pub has_agents: bool,
    pub has_policy: bool,
}

impl ConfigStatus {
    pub fn is_complete(&self) -> bool {
        self.has_projects && self.has_agents && self.has_policy
    }
    pub fn is_empty(&self) -> bool {
        !self.has_projects && !self.has_agents && !self.has_policy
    }
    pub fn present_count(&self) -> u32 {
        [self.has_projects, self.has_agents, self.has_policy]
            .iter()
            .filter(|&&v| v)
            .count() as u32
    }
}

pub fn config_status(dir: &Path) -> ConfigStatus {
    ConfigStatus {
        dir: dir.to_path_buf(),
        has_projects: dir.join("projects.toml").is_file(),
        has_agents: dir.join("agents.toml").is_file(),
        has_policy: dir.join("policy.toml").is_file(),
    }
}

fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// --- Defaults ---

fn default_true() -> bool {
    true
}
fn default_cli() -> String {
    "cli".to_string()
}
fn default_budget() -> f64 {
    1.0
}
fn default_model() -> String {
    "sonnet".to_string()
}
fn default_timeout() -> u64 {
    600
}
fn default_slots() -> u32 {
    5
}
fn default_ceiling() -> f64 {
    50.0
}
fn default_soft() -> u32 {
    80
}
fn default_hard() -> u32 {
    95
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_or_default_rejects_invalid_existing_projects_toml() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("projects.toml"), "[[projects]\nid = \"broken\"").unwrap();

        let err = load_or_default(tmp.path()).unwrap_err();
        match err {
            ConfigError::ParseError { file, .. } => assert_eq!(file, "projects.toml"),
            other => panic!("expected parse error, got {other:?}"),
        }
    }

    #[test]
    fn load_or_default_rejects_invalid_existing_agents_toml() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("agents.toml"), "[agents.claude").unwrap();

        let err = load_or_default(tmp.path()).unwrap_err();
        match err {
            ConfigError::ParseError { file, .. } => assert_eq!(file, "agents.toml"),
            other => panic!("expected parse error, got {other:?}"),
        }
    }

    #[test]
    fn load_or_default_rejects_invalid_existing_policy_toml() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("policy.toml"), "[defaults").unwrap();

        let err = load_or_default(tmp.path()).unwrap_err();
        match err {
            ConfigError::ParseError { file, .. } => assert_eq!(file, "policy.toml"),
            other => panic!("expected parse error, got {other:?}"),
        }
    }

    #[test]
    fn detect_agents_returns_empty_when_no_supported_cli_is_installed() {
        let agents = detect_agents_with(|_| false);
        assert!(agents.agents.is_empty());
    }
}
