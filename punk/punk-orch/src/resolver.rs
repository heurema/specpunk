use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;

// --- Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedProject {
    pub id: String,
    pub path: PathBuf,
    pub source: ResolveSource,
    pub stack: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResolveSource {
    CliPath,
    Pinned,
    Toml,
    CachedScan,
    LazyScan,
}

impl std::fmt::Display for ResolveSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CliPath => write!(f, "cli-path"),
            Self::Pinned => write!(f, "pinned"),
            Self::Toml => write!(f, "toml"),
            Self::CachedScan => write!(f, "cached-scan"),
            Self::LazyScan => write!(f, "lazy-scan"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectCache {
    pub version: u32,
    pub updated_at: String,
    pub projects: Vec<CachedProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedProject {
    pub id: String,
    pub path: String,
    pub pinned: bool,
    pub stack: Option<String>,
    pub discovered_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbiguousProjectCandidate {
    pub path: PathBuf,
    pub sources: Vec<ResolveSource>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("project '{name}' not found{}", format_suggestions(suggestions))]
    NotFound {
        name: String,
        suggestions: Vec<String>,
    },
    #[error("path does not exist: {0}")]
    PathNotFound(PathBuf),
    #[error("configured path for project '{name}' does not exist: {path}")]
    ConfiguredPathNotFound { name: String, path: PathBuf },
    #[error("project '{name}' is ambiguous{}", format_ambiguity(candidates))]
    Ambiguous {
        name: String,
        candidates: Vec<AmbiguousProjectCandidate>,
    },
    #[error("cache error: {0}")]
    CacheError(#[from] std::io::Error),
}

fn format_suggestions(suggestions: &[String]) -> String {
    if suggestions.is_empty() {
        String::new()
    } else {
        format!(". Did you mean: {}?", suggestions.join(", "))
    }
}

fn format_ambiguity(candidates: &[AmbiguousProjectCandidate]) -> String {
    if candidates.is_empty() {
        return String::new();
    }
    let details = candidates
        .iter()
        .map(|candidate| {
            let sources = candidate
                .sources
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("\n  - {} ({sources})", candidate.path.display())
        })
        .collect::<String>();
    format!(":{details}")
}

// --- Scan roots ---

const SCAN_ROOTS: &[&str] = &["~/personal/heurema", "~/works", "~/contrib", "~/personal"];

fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(rest)
    } else {
        PathBuf::from(p)
    }
}

// --- Cache ---

pub fn cache_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".cache")
        })
        .join("punk/projects.json")
}

pub fn load_cache() -> ProjectCache {
    let path = cache_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_cache(cache: &ProjectCache) -> Result<(), std::io::Error> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(cache).map_err(std::io::Error::other)?;
    fs::write(&path, data)
}

pub fn pin_project(id: &str, path: &Path) -> Result<(), ResolveError> {
    if !path.is_dir() {
        return Err(ResolveError::PathNotFound(path.to_path_buf()));
    }
    let abs = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let stack = detect_stack(&abs);
    let now = chrono::Utc::now().to_rfc3339();

    let mut cache = load_cache();
    // Remove existing entry with same id
    cache.projects.retain(|p| p.id != id);
    cache.projects.push(CachedProject {
        id: id.to_string(),
        path: abs.to_string_lossy().to_string(),
        pinned: true,
        stack,
        discovered_at: now.clone(),
    });
    cache.version = 1;
    cache.updated_at = now;
    save_cache(&cache)?;
    Ok(())
}

pub fn unpin_project(id: &str) -> Result<bool, ResolveError> {
    let mut cache = load_cache();
    let before = cache.projects.len();
    cache.projects.retain(|p| !(p.id == id && p.pinned));
    if cache.projects.len() == before {
        return Ok(false);
    }
    cache.updated_at = chrono::Utc::now().to_rfc3339();
    save_cache(&cache)?;
    Ok(true)
}

// --- Resolution chain ---

pub fn resolve(
    name: &str,
    cli_path: Option<&Path>,
    config: Option<&Config>,
) -> Result<ResolvedProject, ResolveError> {
    // 1. --path flag
    if let Some(path) = cli_path {
        if !path.is_dir() {
            return Err(ResolveError::PathNotFound(path.to_path_buf()));
        }
        let abs = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        return Ok(ResolvedProject {
            id: name.to_string(),
            path: abs.clone(),
            source: ResolveSource::CliPath,
            stack: detect_stack(&abs),
        });
    }

    let cache = load_cache();
    let scanned = scan_for_project_candidates(name);
    if let Some(resolved) = resolve_from_sources(name, config, &cache, &scanned)? {
        if matches!(resolved.source, ResolveSource::LazyScan)
            && !cache.projects.iter().any(|p| p.id == name)
        {
            let mut cache = cache;
            cache.projects.push(CachedProject {
                id: resolved.id.clone(),
                path: resolved.path.to_string_lossy().to_string(),
                pinned: false,
                stack: resolved.stack.clone(),
                discovered_at: chrono::Utc::now().to_rfc3339(),
            });
            cache.version = 1;
            cache.updated_at = chrono::Utc::now().to_rfc3339();
            save_cache(&cache).ok();
        }
        return Ok(resolved);
    }

    // 6. Fuzzy suggestions
    let suggestions = collect_suggestions(name, config, &cache);
    Err(ResolveError::NotFound {
        name: name.to_string(),
        suggestions,
    })
}

fn resolve_from_sources(
    name: &str,
    config: Option<&Config>,
    cache: &ProjectCache,
    scanned: &[ResolvedProject],
) -> Result<Option<ResolvedProject>, ResolveError> {
    let mut candidates = Vec::new();

    for entry in cache.projects.iter().filter(|p| p.id == name && p.pinned) {
        let path = PathBuf::from(&entry.path);
        if path.is_dir() {
            candidates.push(ResolvedProject {
                id: name.to_string(),
                path,
                source: ResolveSource::Pinned,
                stack: entry.stack.clone(),
            });
        }
    }

    if let Some(cfg) = config {
        for proj in cfg.projects.projects.iter().filter(|p| p.id == name) {
            let path = expand_tilde(&proj.path);
            if !path.is_dir() {
                return Err(ResolveError::ConfiguredPathNotFound {
                    name: name.to_string(),
                    path,
                });
            }
            candidates.push(ResolvedProject {
                id: name.to_string(),
                path: path.clone(),
                source: ResolveSource::Toml,
                stack: if proj.stack.is_empty() {
                    detect_stack(&path)
                } else {
                    Some(proj.stack.clone())
                },
            });
        }
    }

    for entry in cache.projects.iter().filter(|p| p.id == name && !p.pinned) {
        let path = PathBuf::from(&entry.path);
        if path.is_dir() {
            candidates.push(ResolvedProject {
                id: name.to_string(),
                path,
                source: ResolveSource::CachedScan,
                stack: entry.stack.clone(),
            });
        }
    }

    candidates.extend(scanned.iter().filter(|p| p.id == name).cloned());

    if candidates.is_empty() {
        return Ok(None);
    }

    let ambiguities = collapse_ambiguous_candidates(&candidates);
    if ambiguities.len() > 1 {
        return Err(ResolveError::Ambiguous {
            name: name.to_string(),
            candidates: ambiguities,
        });
    }

    Ok(candidates.into_iter().next())
}

fn collapse_ambiguous_candidates(candidates: &[ResolvedProject]) -> Vec<AmbiguousProjectCandidate> {
    let mut by_path: HashMap<PathBuf, AmbiguousProjectCandidate> = HashMap::new();
    for candidate in candidates {
        let key = if candidate.path.is_dir() {
            fs::canonicalize(&candidate.path).unwrap_or_else(|_| candidate.path.clone())
        } else {
            candidate.path.clone()
        };
        let entry = by_path
            .entry(key.clone())
            .or_insert_with(|| AmbiguousProjectCandidate {
                path: candidate.path.clone(),
                sources: Vec::new(),
            });
        if !entry.sources.contains(&candidate.source) {
            entry.sources.push(candidate.source.clone());
        }
    }
    let mut values: Vec<_> = by_path.into_values().collect();
    values.sort_by(|a, b| a.path.cmp(&b.path));
    values
}

// --- Scan ---

fn scan_for_project_candidates(name: &str) -> Vec<ResolvedProject> {
    let mut results = Vec::new();
    for root in SCAN_ROOTS {
        let root_path = expand_tilde(root);
        let candidate = root_path.join(name);
        if candidate.is_dir() && candidate.join(".git").exists() {
            results.push(ResolvedProject {
                id: name.to_string(),
                path: candidate.clone(),
                source: ResolveSource::LazyScan,
                stack: detect_stack(&candidate),
            });
        }
    }
    results
}

pub fn scan_all_roots() -> Vec<ResolvedProject> {
    let mut found: HashMap<String, ResolvedProject> = HashMap::new();

    for root in SCAN_ROOTS {
        let root_path = expand_tilde(root);
        let entries = match fs::read_dir(&root_path) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join(".git").exists() {
                let id = entry.file_name().to_string_lossy().to_string();
                if !found.contains_key(&id) {
                    found.insert(
                        id.clone(),
                        ResolvedProject {
                            id,
                            path: path.clone(),
                            source: ResolveSource::LazyScan,
                            stack: detect_stack(&path),
                        },
                    );
                }
            }
        }
    }

    let mut projects: Vec<_> = found.into_values().collect();
    projects.sort_by(|a, b| a.id.cmp(&b.id));
    projects
}

/// Merge all known projects from scan + cache + TOML. Dedupe by id while
/// preserving runtime resolution precedence: pinned > TOML > cached scan > lazy scan.
pub fn list_known(config: Option<&Config>) -> Vec<ResolvedProject> {
    merge_known_projects(config, &load_cache(), scan_all_roots())
}

fn merge_known_projects(
    config: Option<&Config>,
    cache: &ProjectCache,
    scanned: Vec<ResolvedProject>,
) -> Vec<ResolvedProject> {
    let mut by_id: HashMap<String, ResolvedProject> = HashMap::new();

    // Scan roots (lowest priority)
    for p in scanned {
        by_id.entry(p.id.clone()).or_insert(p);
    }

    // Cache (medium priority for non-pinned entries)
    for entry in &cache.projects {
        if entry.pinned {
            continue;
        }
        let path = PathBuf::from(&entry.path);
        if path.is_dir() {
            by_id.insert(
                entry.id.clone(),
                ResolvedProject {
                    id: entry.id.clone(),
                    path,
                    source: ResolveSource::CachedScan,
                    stack: entry.stack.clone(),
                },
            );
        }
    }

    // TOML (highest priority)
    if let Some(cfg) = config {
        for proj in &cfg.projects.projects {
            if !proj.active {
                continue;
            }
            let path = expand_tilde(&proj.path);
            by_id.insert(
                proj.id.clone(),
                ResolvedProject {
                    id: proj.id.clone(),
                    path,
                    source: ResolveSource::Toml,
                    stack: if proj.stack.is_empty() {
                        None
                    } else {
                        Some(proj.stack.clone())
                    },
                },
            );
        }
    }

    // Pinned aliases (highest priority, consistent with runtime resolution)
    for entry in &cache.projects {
        if !entry.pinned {
            continue;
        }
        let path = PathBuf::from(&entry.path);
        if path.is_dir() {
            by_id.insert(
                entry.id.clone(),
                ResolvedProject {
                    id: entry.id.clone(),
                    path,
                    source: ResolveSource::Pinned,
                    stack: entry.stack.clone(),
                },
            );
        }
    }

    let mut projects: Vec<_> = by_id.into_values().collect();
    projects.sort_by(|a, b| a.id.cmp(&b.id));
    projects
}

// --- Helpers ---

fn detect_stack(path: &Path) -> Option<String> {
    if path.join("Cargo.toml").exists() {
        Some("rust".to_string())
    } else if path.join("pyproject.toml").exists() || path.join("setup.py").exists() {
        Some("python".to_string())
    } else if path.join("package.json").exists() {
        Some("node".to_string())
    } else if path.join("go.mod").exists() {
        Some("go".to_string())
    } else {
        None
    }
}

fn collect_suggestions(name: &str, config: Option<&Config>, cache: &ProjectCache) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();

    // From TOML
    if let Some(cfg) = config {
        for p in &cfg.projects.projects {
            candidates.push(p.id.clone());
        }
    }

    // From cache
    for p in &cache.projects {
        if !candidates.contains(&p.id) {
            candidates.push(p.id.clone());
        }
    }

    // Filter: prefix match or substring match
    let name_lower = name.to_lowercase();
    candidates
        .into_iter()
        .filter(|c| {
            let c_lower = c.to_lowercase();
            c_lower.starts_with(&name_lower)
                || c_lower.contains(&name_lower)
                || name_lower.contains(&c_lower)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_project_dir(parent: &Path, name: &str) -> PathBuf {
        let dir = parent.join(name);
        fs::create_dir_all(dir.join(".git")).unwrap();
        dir
    }

    #[test]
    fn resolve_cli_path() {
        let tmp = TempDir::new().unwrap();
        let proj = make_project_dir(tmp.path(), "myproj");

        let result = resolve("myproj", Some(&proj), None).unwrap();
        assert_eq!(result.source, ResolveSource::CliPath);
        assert_eq!(result.id, "myproj");
    }

    #[test]
    fn resolve_cli_path_not_found() {
        let result = resolve("x", Some(Path::new("/nonexistent/path")), None);
        assert!(matches!(result, Err(ResolveError::PathNotFound(_))));
    }

    #[test]
    fn stale_toml_path_is_an_explicit_error() {
        let missing = format!(
            "/tmp/specpunk-missing-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let cfg = crate::config::Config {
            projects: crate::config::ProjectsFile {
                projects: vec![crate::config::Project {
                    id: "stale-config-proj".to_string(),
                    path: missing.clone(),
                    stack: String::new(),
                    active: true,
                    budget_usd: 0.0,
                    checkpoint: String::new(),
                }],
            },
            agents: crate::config::AgentsFile {
                agents: HashMap::new(),
            },
            policy: crate::config::default_policy(),
            dir: PathBuf::new(),
        };

        let result = resolve("stale-config-proj", None, Some(&cfg));
        match result {
            Err(ResolveError::ConfiguredPathNotFound { name, path }) => {
                assert_eq!(name, "stale-config-proj");
                assert_eq!(path, PathBuf::from(missing));
            }
            other => panic!("expected configured path error, got {other:?}"),
        }
    }

    #[test]
    fn resolve_reports_ambiguity_for_conflicting_sources() {
        let tmp = TempDir::new().unwrap();
        let toml = make_project_dir(tmp.path(), "toml-proj");
        let cached = make_project_dir(tmp.path(), "cached-proj");

        let cache = ProjectCache {
            version: 1,
            updated_at: String::new(),
            projects: vec![CachedProject {
                id: "same-proj".to_string(),
                path: cached.to_string_lossy().to_string(),
                pinned: false,
                stack: Some("rust".to_string()),
                discovered_at: String::new(),
            }],
        };

        let cfg = crate::config::Config {
            projects: crate::config::ProjectsFile {
                projects: vec![crate::config::Project {
                    id: "same-proj".to_string(),
                    path: toml.to_string_lossy().to_string(),
                    stack: "rust".to_string(),
                    active: true,
                    budget_usd: 0.0,
                    checkpoint: String::new(),
                }],
            },
            agents: crate::config::AgentsFile {
                agents: HashMap::new(),
            },
            policy: crate::config::default_policy(),
            dir: PathBuf::new(),
        };

        let result = resolve_from_sources("same-proj", Some(&cfg), &cache, &[]);
        match result {
            Err(ResolveError::Ambiguous { name, candidates }) => {
                assert_eq!(name, "same-proj");
                assert_eq!(candidates.len(), 2);
                assert!(candidates.iter().any(|c| c.path == toml));
                assert!(candidates.iter().any(|c| c.path == cached));
            }
            other => panic!("expected ambiguity error, got {other:?}"),
        }
    }

    #[test]
    fn resolve_prefers_single_unique_path_even_when_sources_overlap() {
        let tmp = TempDir::new().unwrap();
        let shared = make_project_dir(tmp.path(), "shared-proj");

        let cache = ProjectCache {
            version: 1,
            updated_at: String::new(),
            projects: vec![CachedProject {
                id: "shared-proj".to_string(),
                path: shared.to_string_lossy().to_string(),
                pinned: false,
                stack: Some("rust".to_string()),
                discovered_at: String::new(),
            }],
        };
        let scanned = vec![ResolvedProject {
            id: "shared-proj".to_string(),
            path: shared.clone(),
            source: ResolveSource::LazyScan,
            stack: Some("rust".to_string()),
        }];

        let resolved = resolve_from_sources("shared-proj", None, &cache, &scanned)
            .unwrap()
            .expect("unique path should resolve");
        assert_eq!(resolved.source, ResolveSource::CachedScan);
        assert_eq!(resolved.path, shared);
    }

    #[test]
    fn list_known_keeps_pinned_aliases_ahead_of_toml_entries() {
        let tmp = TempDir::new().unwrap();
        let pinned = make_project_dir(tmp.path(), "pinned-proj");
        let toml = make_project_dir(tmp.path(), "toml-proj");

        let cache = ProjectCache {
            version: 1,
            updated_at: String::new(),
            projects: vec![CachedProject {
                id: "same-proj".to_string(),
                path: pinned.to_string_lossy().to_string(),
                pinned: true,
                stack: Some("rust".to_string()),
                discovered_at: String::new(),
            }],
        };

        let cfg = crate::config::Config {
            projects: crate::config::ProjectsFile {
                projects: vec![crate::config::Project {
                    id: "same-proj".to_string(),
                    path: toml.to_string_lossy().to_string(),
                    stack: "rust".to_string(),
                    active: true,
                    budget_usd: 0.0,
                    checkpoint: String::new(),
                }],
            },
            agents: crate::config::AgentsFile {
                agents: HashMap::new(),
            },
            policy: crate::config::default_policy(),
            dir: PathBuf::new(),
        };

        let listed = merge_known_projects(Some(&cfg), &cache, vec![]);
        let project = listed.into_iter().find(|p| p.id == "same-proj").unwrap();
        assert_eq!(project.source, ResolveSource::Pinned);
        assert_eq!(project.path, pinned);
    }

    #[test]
    fn pin_and_resolve() {
        let tmp = TempDir::new().unwrap();
        let proj = make_project_dir(tmp.path(), "testproj");

        // Use a unique cache path to avoid interfering with real cache
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        let cache_file = cache_dir.join("projects.json");

        // Pin manually via cache
        let now = chrono::Utc::now().to_rfc3339();
        let cache = ProjectCache {
            version: 1,
            updated_at: now.clone(),
            projects: vec![CachedProject {
                id: "testproj".to_string(),
                path: proj.to_string_lossy().to_string(),
                pinned: true,
                stack: None,
                discovered_at: now,
            }],
        };
        let data = serde_json::to_string_pretty(&cache).unwrap();
        fs::write(&cache_file, data).unwrap();

        // Verify cache round-trip
        let loaded: ProjectCache =
            serde_json::from_str(&fs::read_to_string(&cache_file).unwrap()).unwrap();
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].id, "testproj");
        assert!(loaded.projects[0].pinned);
    }

    #[test]
    fn detect_stack_rust() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]").unwrap();
        assert_eq!(detect_stack(tmp.path()), Some("rust".to_string()));
    }

    #[test]
    fn detect_stack_python() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("pyproject.toml"), "[project]").unwrap();
        assert_eq!(detect_stack(tmp.path()), Some("python".to_string()));
    }

    #[test]
    fn detect_stack_node() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_stack(tmp.path()), Some("node".to_string()));
    }

    #[test]
    fn detect_stack_go() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("go.mod"), "module x").unwrap();
        assert_eq!(detect_stack(tmp.path()), Some("go".to_string()));
    }

    #[test]
    fn detect_stack_unknown() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(detect_stack(tmp.path()), None);
    }

    #[test]
    fn suggestions_substring_match() {
        let cache = ProjectCache {
            version: 1,
            updated_at: String::new(),
            projects: vec![
                CachedProject {
                    id: "mycel".to_string(),
                    path: "/tmp/mycel".to_string(),
                    pinned: false,
                    stack: None,
                    discovered_at: String::new(),
                },
                CachedProject {
                    id: "signum".to_string(),
                    path: "/tmp/signum".to_string(),
                    pinned: false,
                    stack: None,
                    discovered_at: String::new(),
                },
            ],
        };
        let suggestions = collect_suggestions("myc", None, &cache);
        assert_eq!(suggestions, vec!["mycel"]);
    }

    #[test]
    fn suggestions_no_match() {
        let cache = ProjectCache::default();
        let suggestions = collect_suggestions("zzz", None, &cache);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn expand_tilde_works() {
        let expanded = expand_tilde("~/test");
        assert!(!expanded.to_string_lossy().starts_with('~'));
        assert!(expanded.to_string_lossy().ends_with("test"));
    }

    #[test]
    fn expand_tilde_no_tilde() {
        let expanded = expand_tilde("/absolute/path");
        assert_eq!(expanded, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn cache_round_trip() {
        let now = chrono::Utc::now().to_rfc3339();
        let cache = ProjectCache {
            version: 1,
            updated_at: now.clone(),
            projects: vec![CachedProject {
                id: "proj".to_string(),
                path: "/tmp/proj".to_string(),
                pinned: true,
                stack: Some("rust".to_string()),
                discovered_at: now,
            }],
        };
        let json = serde_json::to_string(&cache).unwrap();
        let loaded: ProjectCache = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].id, "proj");
        assert!(loaded.projects[0].pinned);
    }

    #[test]
    fn resolve_not_found_with_suggestions() {
        let cache = ProjectCache {
            version: 1,
            updated_at: String::new(),
            projects: vec![CachedProject {
                id: "signum".to_string(),
                path: "/nonexistent".to_string(),
                pinned: false,
                stack: None,
                discovered_at: String::new(),
            }],
        };
        // Direct resolve won't use this cache (it reads from disk).
        // But collect_suggestions works with the cache directly.
        let suggestions = collect_suggestions("sig", None, &cache);
        assert_eq!(suggestions, vec!["signum"]);
    }
}
