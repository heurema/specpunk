use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// A summary of the project context loaded from .punk/ artifacts.
/// Source file contents are NEVER included — only file inventory, conventions, and intent text.
#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub intent: String,
    pub conventions_summary: String,
    pub file_inventory: Vec<String>,
    pub scanned_at: Option<String>,
    pub staleness_warnings: Vec<String>,
}

/// Minimal shape needed from conventions.json.
#[derive(Debug, Deserialize)]
struct ConventionItem {
    name: String,
    value: String,
}

/// Minimal shape needed from scan.json.
#[derive(Debug, Deserialize)]
struct ScanJson {
    #[serde(default)]
    dir_map: std::collections::HashMap<String, Vec<String>>,
    #[serde(default)]
    scanned_at: String,
}

/// Errors that can occur while loading context.
#[derive(Debug)]
pub enum ContextError {
    MissingPunkDir,
    Io(std::io::Error),
    Parse(String),
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextError::MissingPunkDir => {
                write!(f, ".punk/ directory not found — run `punk init` first")
            }
            ContextError::Io(e) => write!(f, "I/O error: {e}"),
            ContextError::Parse(s) => write!(f, "parse error: {s}"),
        }
    }
}

impl std::error::Error for ContextError {}

impl From<std::io::Error> for ContextError {
    fn from(e: std::io::Error) -> Self {
        ContextError::Io(e)
    }
}

/// Staleness threshold in days.
const STALENESS_DAYS: i64 = 90;

/// Load project context from `root/.punk/`.
/// Returns `ContextError::MissingPunkDir` if the directory is absent.
pub fn load_context(root: &Path) -> Result<ProjectContext, ContextError> {
    let punk_dir = root.join(".punk");
    if !punk_dir.is_dir() {
        return Err(ContextError::MissingPunkDir);
    }

    let intent = load_intent(&punk_dir)?;
    let (conventions_summary, scanned_at_opt) = load_conventions_and_scan(&punk_dir)?;
    let file_inventory = load_file_inventory(&punk_dir)?;
    let staleness_warnings = check_staleness(&punk_dir, &scanned_at_opt, &file_inventory);

    Ok(ProjectContext {
        intent,
        conventions_summary,
        file_inventory,
        scanned_at: scanned_at_opt,
        staleness_warnings,
    })
}

fn load_intent(punk_dir: &Path) -> Result<String, ContextError> {
    let path = punk_dir.join("intent.md");
    if path.exists() {
        Ok(std::fs::read_to_string(&path)?)
    } else {
        Ok(String::from("(no intent.md found)"))
    }
}

fn load_conventions_and_scan(
    punk_dir: &Path,
) -> Result<(String, Option<String>), ContextError> {
    // Load conventions.json
    let conv_path = punk_dir.join("conventions.json");
    let conventions_summary = if conv_path.exists() {
        let raw = std::fs::read_to_string(&conv_path)?;
        let items: Vec<ConventionItem> = serde_json::from_str(&raw)
            .map_err(|e| ContextError::Parse(format!("conventions.json: {e}")))?;
        items
            .iter()
            .map(|c| format!("- {}: {}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::from("(no conventions.json found)")
    };

    // Load scanned_at from scan.json if present
    let scan_path = punk_dir.join("scan.json");
    let scanned_at = if scan_path.exists() {
        let raw = std::fs::read_to_string(&scan_path)?;
        let scan: ScanJson = serde_json::from_str(&raw)
            .map_err(|e| ContextError::Parse(format!("scan.json: {e}")))?;
        if scan.scanned_at.is_empty() {
            None
        } else {
            Some(scan.scanned_at)
        }
    } else {
        None
    };

    Ok((conventions_summary, scanned_at))
}

fn load_file_inventory(punk_dir: &Path) -> Result<Vec<String>, ContextError> {
    let scan_path = punk_dir.join("scan.json");
    if !scan_path.exists() {
        return Ok(vec![]);
    }
    let raw = std::fs::read_to_string(&scan_path)?;
    let scan: ScanJson = serde_json::from_str(&raw)
        .map_err(|e| ContextError::Parse(format!("scan.json: {e}")))?;

    // Flatten dir_map into a file list — no source content, just paths
    let mut files: Vec<String> = scan
        .dir_map
        .iter()
        .flat_map(|(dir, names)| {
            names
                .iter()
                .map(move |n| format!("{dir}/{n}"))
        })
        .collect();
    files.sort();
    Ok(files)
}

fn check_staleness(
    punk_dir: &Path,
    scanned_at: &Option<String>,
    file_inventory: &[String],
) -> Vec<String> {
    let mut warnings = Vec::new();

    // Check age of conventions.json
    if let Ok(meta) = punk_dir.join("conventions.json").metadata() {
        if let Ok(modified) = meta.modified() {
            let age = std::time::SystemTime::now()
                .duration_since(modified)
                .unwrap_or_default();
            let age_days = age.as_secs() / 86400;
            if age_days as i64 > STALENESS_DAYS {
                warnings.push(format!(
                    "conventions.json is {age_days} days old (>{STALENESS_DAYS}d) — run `punk init --refresh`"
                ));
            }
        }
    }

    // Check if scanned_at timestamp is >90 days old
    if let Some(ts) = scanned_at {
        if let Ok(dt) = ts.parse::<DateTime<Utc>>() {
            let age_days = (Utc::now() - dt).num_days();
            if age_days > STALENESS_DAYS {
                warnings.push(format!(
                    "scan.json scanned_at is {age_days} days old (>{STALENESS_DAYS}d) — run `punk init --refresh`"
                ));
            }
        }
    }

    // Check if >20% of tracked files have changed (heuristic: count missing files)
    // Resolve paths against project root (parent of .punk/), not CWD
    let project_root = punk_dir.parent().unwrap_or(punk_dir);
    if !file_inventory.is_empty() {
        let missing = file_inventory
            .iter()
            .filter(|f| !project_root.join(f).exists())
            .count();
        let ratio = missing as f64 / file_inventory.len() as f64;
        if ratio > 0.2 {
            warnings.push(format!(
                "{:.0}% of tracked files missing from last scan — run `punk init --refresh`",
                ratio * 100.0
            ));
        }
    }

    warnings
}

/// Build a concise LLM prompt context string from the loaded project context.
/// Never includes source file contents — only metadata and intent text.
pub fn build_prompt_context(ctx: &ProjectContext) -> String {
    let mut parts = Vec::new();

    parts.push(format!("## Project Intent\n{}", ctx.intent.trim()));

    if !ctx.conventions_summary.is_empty()
        && ctx.conventions_summary != "(no conventions.json found)"
    {
        parts.push(format!(
            "## Conventions\n{}",
            ctx.conventions_summary
        ));
    }

    if !ctx.file_inventory.is_empty() {
        let preview: Vec<_> = ctx.file_inventory.iter().take(50).collect();
        parts.push(format!(
            "## File Inventory ({} files, showing up to 50)\n{}",
            ctx.file_inventory.len(),
            preview.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n")
        ));
    }

    if !ctx.staleness_warnings.is_empty() {
        parts.push(format!(
            "## Staleness Warnings\n{}",
            ctx.staleness_warnings
                .iter()
                .map(|w| format!("- {w}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_punk_dir(tmp: &TempDir) -> std::path::PathBuf {
        let punk = tmp.path().join(".punk");
        fs::create_dir_all(&punk).unwrap();
        punk
    }

    #[test]
    fn plan_without_init() {
        let tmp = TempDir::new().unwrap();
        // No .punk dir
        let result = load_context(tmp.path());
        assert!(
            matches!(result, Err(ContextError::MissingPunkDir)),
            "expected MissingPunkDir error"
        );
    }

    #[test]
    fn no_source_code_in_prompt() {
        let tmp = TempDir::new().unwrap();
        let punk = make_punk_dir(&tmp);

        fs::write(punk.join("intent.md"), "# Intent\nBuild auth").unwrap();
        fs::write(
            punk.join("conventions.json"),
            r#"[{"name":"error_handling","value":"Result<T,E>","confidence":"high","source":"scan"}]"#,
        )
        .unwrap();

        // Create a scan.json with dir_map referencing .rs files (but their content not read)
        let scan = serde_json::json!({
            "scanned_at": "2026-03-23T00:00:00Z",
            "dir_map": {
                "src": ["main.rs", "lib.rs"]
            },
            "languages": {},
            "frameworks": [],
            "test_runner": null,
            "test_count": 0,
            "ci_detected": false,
            "ci_files": [],
            "container_detected": false,
            "build_system": null,
            "entry_points": [],
            "dependencies": {},
            "conventions": [],
            "never_touch": [],
            "archaeology": {"commit_style":"","contributor_count":0,"branch_count":0,"conventional_commit_ratio":0.0},
            "error_crate": null,
            "unwrap_density": 0.0,
            "logging_crate": null
        });
        fs::write(punk.join("scan.json"), serde_json::to_string_pretty(&scan).unwrap()).unwrap();

        let ctx = load_context(tmp.path()).unwrap();
        let prompt = build_prompt_context(&ctx);

        // Prompt must not contain any .rs source code constructs
        assert!(
            !prompt.contains("fn main"),
            "prompt must not contain source code"
        );
        assert!(
            !prompt.contains("pub fn"),
            "prompt must not contain source code"
        );
        // But file names are fine
        assert!(prompt.contains("main.rs") || prompt.contains("src/main.rs"));
    }

    #[test]
    fn convention_staleness() {
        let tmp = TempDir::new().unwrap();
        let punk = make_punk_dir(&tmp);

        fs::write(punk.join("intent.md"), "# Intent").unwrap();
        fs::write(punk.join("conventions.json"), "[]").unwrap();

        // Write a scan.json with a timestamp >90 days old
        let old_ts = "2020-01-01T00:00:00Z";
        let scan = serde_json::json!({
            "scanned_at": old_ts,
            "dir_map": {},
            "languages": {},
            "frameworks": [],
            "test_runner": null,
            "test_count": 0,
            "ci_detected": false,
            "ci_files": [],
            "container_detected": false,
            "build_system": null,
            "entry_points": [],
            "dependencies": {},
            "conventions": [],
            "never_touch": [],
            "archaeology": {"commit_style":"","contributor_count":0,"branch_count":0,"conventional_commit_ratio":0.0},
            "error_crate": null,
            "unwrap_density": 0.0,
            "logging_crate": null
        });
        fs::write(punk.join("scan.json"), serde_json::to_string_pretty(&scan).unwrap()).unwrap();

        let ctx = load_context(tmp.path()).unwrap();
        assert!(
            !ctx.staleness_warnings.is_empty(),
            "expected staleness warning for old scan.json"
        );
        let all = ctx.staleness_warnings.join(" ");
        assert!(
            all.contains("90") || all.contains("days"),
            "warning should mention age: {all}"
        );
    }
}
