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
