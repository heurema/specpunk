use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A configured LLM provider (endpoint + API key).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub endpoint: String,
    pub api_key: String,
    /// Optional model name override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Top-level punk configuration stored at ~/.config/punk/providers.toml.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PunkConfig {
    /// Default provider name (key into `providers` map).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
    /// Named providers.
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
}

/// Errors from config operations.
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(String),
    Serialize(String),
    NoProvider(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "config I/O error: {e}"),
            ConfigError::Parse(s) => write!(f, "config parse error: {s}"),
            ConfigError::Serialize(s) => write!(f, "config serialize error: {s}"),
            ConfigError::NoProvider(s) => write!(f, "provider not found: {s}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::Io(e)
    }
}

/// Returns the config directory: ~/.config/punk/
pub fn config_dir() -> PathBuf {
    dirs_or_default().join("punk")
}

/// Returns the providers.toml path.
pub fn providers_path() -> PathBuf {
    config_dir().join("providers.toml")
}

/// Load config from disk. Returns default if file doesn't exist.
pub fn load_config() -> Result<PunkConfig, ConfigError> {
    let path = providers_path();
    if !path.exists() {
        return Ok(PunkConfig::default());
    }
    let content = std::fs::read_to_string(&path)?;
    toml::from_str(&content).map_err(|e| ConfigError::Parse(e.to_string()))
}

/// Save config to disk. Creates directory if needed.
pub fn save_config(config: &PunkConfig) -> Result<(), ConfigError> {
    let path = providers_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content =
        toml::to_string_pretty(config).map_err(|e| ConfigError::Serialize(e.to_string()))?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// Set a provider in config. If first provider, also set as default.
pub fn set_provider(
    name: &str,
    endpoint: &str,
    api_key: &str,
    model: Option<&str>,
) -> Result<(), ConfigError> {
    let mut config = load_config()?;
    let is_first = config.providers.is_empty();
    config.providers.insert(
        name.to_string(),
        ProviderConfig {
            endpoint: endpoint.to_string(),
            api_key: api_key.to_string(),
            model: model.map(|s| s.to_string()),
        },
    );
    if is_first || config.default_provider.is_none() {
        config.default_provider = Some(name.to_string());
    }
    save_config(&config)
}

/// Remove a provider from config.
pub fn remove_provider(name: &str) -> Result<(), ConfigError> {
    let mut config = load_config()?;
    if config.providers.remove(name).is_none() {
        return Err(ConfigError::NoProvider(name.to_string()));
    }
    if config.default_provider.as_deref() == Some(name) {
        config.default_provider = config.providers.keys().next().cloned();
    }
    save_config(&config)
}

/// Get the active provider config. Resolution order:
/// 1. PUNK_LLM_ENDPOINT + PUNK_LLM_API_KEY env vars
/// 2. Well-known env vars (ANTHROPIC_API_KEY, OPENAI_API_KEY)
/// 3. Default provider from ~/.config/punk/providers.toml
pub fn resolve_provider() -> Option<ProviderConfig> {
    // 1. Explicit punk env vars
    if let (Ok(endpoint), Ok(key)) = (
        std::env::var("PUNK_LLM_ENDPOINT"),
        std::env::var("PUNK_LLM_API_KEY"),
    ) {
        return Some(ProviderConfig {
            endpoint,
            api_key: key,
            model: None,
        });
    }

    // 2. Well-known provider env vars
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        return Some(ProviderConfig {
            endpoint: "https://api.anthropic.com/v1/messages".to_string(),
            api_key: key,
            model: Some("claude-sonnet-4-20250514".to_string()),
        });
    }
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        return Some(ProviderConfig {
            endpoint: "https://api.openai.com/v1/chat/completions".to_string(),
            api_key: key,
            model: Some("gpt-4o".to_string()),
        });
    }

    // 3. Config file
    let config = load_config().ok()?;
    let default_name = config.default_provider.as_ref()?;
    config.providers.get(default_name).cloned()
}

/// Returns ~/.config/ (XDG_CONFIG_HOME or fallback).
fn dirs_or_default() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg);
    }
    if let Some(home) = home_dir() {
        return home.join(".config");
    }
    PathBuf::from("/tmp")
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("providers.toml");

        let config = PunkConfig {
            default_provider: Some("test".to_string()),
            providers: {
                let mut m = HashMap::new();
                m.insert(
                    "test".to_string(),
                    ProviderConfig {
                        endpoint: "https://api.example.com".to_string(),
                        api_key: "sk-test".to_string(),
                        model: None,
                    },
                );
                m
            },
        };

        let content = toml::to_string_pretty(&config).unwrap();
        std::fs::write(&path, &content).unwrap();

        let loaded: PunkConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.default_provider, Some("test".to_string()));
        assert!(loaded.providers.contains_key("test"));
        assert_eq!(loaded.providers["test"].api_key, "sk-test");
    }

    #[test]
    fn resolve_provider_env() {
        // Save and restore env
        let old_endpoint = std::env::var("PUNK_LLM_ENDPOINT").ok();
        let old_key = std::env::var("PUNK_LLM_API_KEY").ok();

        std::env::set_var("PUNK_LLM_ENDPOINT", "https://test.api");
        std::env::set_var("PUNK_LLM_API_KEY", "sk-env-test");

        let provider = resolve_provider();
        assert!(provider.is_some());
        let p = provider.unwrap();
        assert_eq!(p.endpoint, "https://test.api");
        assert_eq!(p.api_key, "sk-env-test");

        // Restore
        match old_endpoint {
            Some(v) => std::env::set_var("PUNK_LLM_ENDPOINT", v),
            None => std::env::remove_var("PUNK_LLM_ENDPOINT"),
        }
        match old_key {
            Some(v) => std::env::set_var("PUNK_LLM_API_KEY", v),
            None => std::env::remove_var("PUNK_LLM_API_KEY"),
        }
    }

    #[test]
    fn empty_config_returns_default() {
        let config = PunkConfig::default();
        assert!(config.default_provider.is_none());
        assert!(config.providers.is_empty());
    }
}
