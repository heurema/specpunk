use std::path::Path;

use serde::{Deserialize, Serialize};

use super::InitError;

// ---------------------------------------------------------------------------
// Convention types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConversionConfidence {
    Authoritative,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Convention {
    pub name: String,
    pub value: String,
    pub confidence: ConversionConfidence,
    pub source: String,
}

// ---------------------------------------------------------------------------
// ArtifactSet
// ---------------------------------------------------------------------------

/// All generated artifacts for a punk init run.
#[derive(Debug)]
pub struct ArtifactSet {
    pub config_toml: String,
    pub intent_md: String,
    pub conventions_json: String,
    /// Only present in brownfield mode.
    pub scan_json: Option<String>,
}

impl ArtifactSet {
    /// Returns the names of artifacts that will be written.
    pub fn artifact_names(&self) -> Vec<String> {
        let mut names = vec![
            ".punk/config.toml".to_string(),
            ".punk/intent.md".to_string(),
            ".punk/conventions.json".to_string(),
        ];
        if self.scan_json.is_some() {
            names.push(".punk/scan.json".to_string());
        }
        names
    }
}

// ---------------------------------------------------------------------------
// Write artifacts to disk
// ---------------------------------------------------------------------------

/// Write all artifacts to `root/.punk/`.
/// - config.toml: always overwritten
/// - intent.md: preserved if user-edited (content hash differs from generated hash)
/// - conventions.json: always overwritten
/// - scan.json: always overwritten (brownfield only)
/// - .gitignore: created/updated to include scan.json
pub fn write_artifacts(
    root: &Path,
    artifacts: &ArtifactSet,
    is_brownfield: bool,
) -> Result<(), InitError> {
    let punk_dir = root.join(".punk");

    // Security: reject if .punk is a symlink (path-traversal defense)
    if punk_dir.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Err(InitError::Scan(
            ".punk is a symlink — refusing to write artifacts (possible path traversal)".into(),
        ));
    }
    std::fs::create_dir_all(&punk_dir)?;

    // config.toml — always overwrite
    safe_write(&punk_dir, "config.toml", &artifacts.config_toml)?;

    // intent.md — idempotent: only overwrite if not user-edited
    let intent_path = punk_dir.join("intent.md");
    if intent_path.exists() {
        let existing = std::fs::read_to_string(&intent_path)?;
        if existing == artifacts.intent_md {
            safe_write(&punk_dir, "intent.md", &artifacts.intent_md)?;
        }
        // else: user edited — preserve
    } else {
        safe_write(&punk_dir, "intent.md", &artifacts.intent_md)?;
    }

    // conventions.json — always overwrite
    safe_write(&punk_dir, "conventions.json", &artifacts.conventions_json)?;

    // scan.json — brownfield only, always overwrite
    if is_brownfield {
        if let Some(scan) = &artifacts.scan_json {
            safe_write(&punk_dir, "scan.json", scan)?;
        }
    }

    // .gitignore — ensure scan.json is gitignored
    write_gitignore(&punk_dir)?;

    Ok(())
}

/// Write a file inside punk_dir, rejecting symlink targets (path-traversal defense).
fn safe_write(punk_dir: &Path, name: &str, content: &str) -> Result<(), InitError> {
    let target = punk_dir.join(name);
    if target
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(InitError::Scan(format!(
            ".punk/{name} is a symlink — refusing to write (possible path traversal)"
        )));
    }
    std::fs::write(&target, content)?;
    Ok(())
}

fn write_gitignore(punk_dir: &Path) -> Result<(), InitError> {
    let gitignore_path = punk_dir.join(".gitignore");
    let entry = "scan.json\n";

    if gitignore_path.exists() {
        let existing = std::fs::read_to_string(&gitignore_path)?;
        if !existing.contains("scan.json") {
            let mut content = existing;
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(entry);
            std::fs::write(&gitignore_path, content)?;
        }
    } else {
        std::fs::write(&gitignore_path, entry)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_artifacts(with_scan: bool) -> ArtifactSet {
        ArtifactSet {
            config_toml: "[project]\nprimary_language = \"rust\"\n".to_string(),
            intent_md: "# Intent\ngenerated\n".to_string(),
            conventions_json: "[]".to_string(),
            scan_json: if with_scan {
                Some("{\"scanned_at\":\"2026-01-01\"}".to_string())
            } else {
                None
            },
        }
    }

    #[test]
    fn init_idempotent() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        let artifacts = make_artifacts(true);

        // First run
        write_artifacts(dir, &artifacts, true).unwrap();
        let scan1 = fs::read_to_string(dir.join(".punk/scan.json")).unwrap();
        let intent1 = fs::read_to_string(dir.join(".punk/intent.md")).unwrap();

        // Second run with different scan content
        let artifacts2 = ArtifactSet {
            config_toml: artifacts.config_toml.clone(),
            intent_md: artifacts.intent_md.clone(),
            conventions_json: artifacts.conventions_json.clone(),
            scan_json: Some("{\"scanned_at\":\"2026-01-02\"}".to_string()),
        };
        write_artifacts(dir, &artifacts2, true).unwrap();
        let scan2 = fs::read_to_string(dir.join(".punk/scan.json")).unwrap();
        let intent2 = fs::read_to_string(dir.join(".punk/intent.md")).unwrap();

        // scan.json should be overwritten
        assert_ne!(scan1, scan2, "scan.json should be overwritten");
        // intent.md should be preserved (same generated content)
        assert_eq!(intent1, intent2, "intent.md should be preserved");
    }

    #[test]
    fn init_idempotent_user_edited() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        let artifacts = make_artifacts(true);
        write_artifacts(dir, &artifacts, true).unwrap();

        // Simulate user editing intent.md
        fs::write(dir.join(".punk/intent.md"), "# My custom intent\nUser edited\n").unwrap();

        // Second run
        write_artifacts(dir, &artifacts, true).unwrap();
        let intent = fs::read_to_string(dir.join(".punk/intent.md")).unwrap();

        // User edit should be preserved
        assert!(
            intent.contains("User edited"),
            "user-edited intent.md should not be overwritten"
        );
    }

    #[test]
    fn gitignore_created() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        let punk_dir = dir.join(".punk");
        fs::create_dir_all(&punk_dir).unwrap();

        write_gitignore(&punk_dir).unwrap();
        let content = fs::read_to_string(punk_dir.join(".gitignore")).unwrap();
        assert!(content.contains("scan.json"));
    }

    #[test]
    fn gitignore_not_duplicated() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        let punk_dir = dir.join(".punk");
        fs::create_dir_all(&punk_dir).unwrap();

        write_gitignore(&punk_dir).unwrap();
        write_gitignore(&punk_dir).unwrap();

        let content = fs::read_to_string(punk_dir.join(".gitignore")).unwrap();
        let count = content.matches("scan.json").count();
        assert_eq!(count, 1, "scan.json should appear only once in .gitignore");
    }
}
