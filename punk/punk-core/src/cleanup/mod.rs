//! Phase 14: Cleanup + removal verification.
//! Execute contract removals, verify boundaries, prevent reintroduction.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::dsl;
use crate::plan::contract::{CleanupObligation, Removal};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalStatus {
    Removed,
    Missing,
    Failed,
    Reintroduced,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemovalResult {
    pub id: String,
    pub path: String,
    pub status: RemovalStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObligationResult {
    pub id: String,
    pub action: String,
    pub passed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupReport {
    pub removals: Vec<RemovalResult>,
    pub obligations: Vec<ObligationResult>,
    pub all_passed: bool,
}

// ---------------------------------------------------------------------------
// Execute removals
// ---------------------------------------------------------------------------

/// Verify that contract removals have been executed.
pub fn verify_removals(root: &Path, removals: &[Removal]) -> Vec<RemovalResult> {
    removals
        .iter()
        .map(|r| {
            let target = root.join(&r.path);
            if target.exists() {
                RemovalResult {
                    id: r.id.clone(),
                    path: r.path.clone(),
                    status: RemovalStatus::Failed,
                    error: Some(format!("{} still exists", r.path)),
                }
            } else {
                // Check for reintroduction: grep for references
                if r.prevent_reintroduction {
                    let refs = grep_references(root, &r.path);
                    if !refs.is_empty() {
                        return RemovalResult {
                            id: r.id.clone(),
                            path: r.path.clone(),
                            status: RemovalStatus::Reintroduced,
                            error: Some(format!(
                                "{} references found: {}",
                                refs.len(),
                                refs.join(", ")
                            )),
                        };
                    }
                }
                RemovalResult {
                    id: r.id.clone(),
                    path: r.path.clone(),
                    status: RemovalStatus::Removed,
                    error: None,
                }
            }
        })
        .collect()
}

/// Grep for references to a removed path in source files.
fn grep_references(root: &Path, path: &str) -> Vec<String> {
    let basename = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    if basename.is_empty() {
        return vec![];
    }

    let output = Command::new("grep")
        .args([
            "-rl",
            "--include=*.rs",
            "--include=*.py",
            "--include=*.ts",
            "--include=*.js",
            "--include=*.go",
            &basename,
            ".",
        ])
        .current_dir(root)
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect(),
        _ => vec![],
    }
}

/// Verify cleanup obligations via DSL engine.
pub fn verify_obligations(root: &Path, obligations: &[CleanupObligation]) -> Vec<ObligationResult> {
    obligations
        .iter()
        .map(|o| {
            if o.verify.is_empty() {
                return ObligationResult {
                    id: o.id.clone(),
                    action: o.action.clone(),
                    passed: true,
                    error: Some("no verify steps".into()),
                };
            }
            let result = dsl::run_steps(&o.verify, root);
            ObligationResult {
                id: o.id.clone(),
                action: o.action.clone(),
                passed: result.passed,
                error: result.error,
            }
        })
        .collect()
}

/// Run full cleanup verification.
pub fn run_cleanup(
    root: &Path,
    removals: &[Removal],
    obligations: &[CleanupObligation],
) -> CleanupReport {
    let removal_results = verify_removals(root, removals);
    let obligation_results = verify_obligations(root, obligations);

    let all_passed = removal_results
        .iter()
        .all(|r| r.status == RemovalStatus::Removed)
        && obligation_results.iter().all(|o| o.passed);

    CleanupReport {
        removals: removal_results,
        obligations: obligation_results,
        all_passed,
    }
}

pub fn render_cleanup(report: &CleanupReport) -> String {
    let verdict = if report.all_passed { "PASS" } else { "FAIL" };
    let mut out = format!("punk cleanup: {verdict}\n");

    for r in &report.removals {
        let icon = if r.status == RemovalStatus::Removed {
            "OK"
        } else {
            "FAIL"
        };
        out.push_str(&format!("  [{icon}] {} — {:?}\n", r.path, r.status));
        if let Some(e) = &r.error {
            out.push_str(&format!("       {e}\n"));
        }
    }
    for o in &report.obligations {
        let icon = if o.passed { "OK" } else { "FAIL" };
        out.push_str(&format!("  [{icon}] {} — {}\n", o.id, o.action));
        if let Some(e) = &o.error {
            out.push_str(&format!("       {e}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn removal_verified() {
        let tmp = TempDir::new().unwrap();
        // File doesn't exist = removed OK
        let results = verify_removals(
            tmp.path(),
            &[Removal {
                id: "RM-01".into(),
                path: "old.rs".into(),
                removal_type: "file".into(),
                reason: "replaced".into(),
                prevent_reintroduction: false,
            }],
        );
        assert_eq!(results[0].status, RemovalStatus::Removed);
    }

    #[test]
    fn removal_not_done() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("old.rs"), "still here").unwrap();

        let results = verify_removals(
            tmp.path(),
            &[Removal {
                id: "RM-01".into(),
                path: "old.rs".into(),
                removal_type: "file".into(),
                reason: "replaced".into(),
                prevent_reintroduction: false,
            }],
        );
        assert_eq!(results[0].status, RemovalStatus::Failed);
    }

    #[test]
    fn obligation_empty_verify() {
        let tmp = TempDir::new().unwrap();
        let results = verify_obligations(
            tmp.path(),
            &[CleanupObligation {
                id: "CO-01".into(),
                action: "remove_refs".into(),
                target: "src/".into(),
                blocking: true,
                verify: vec![],
            }],
        );
        assert!(results[0].passed);
    }

    #[test]
    fn full_cleanup_pass() {
        let tmp = TempDir::new().unwrap();
        let report = run_cleanup(tmp.path(), &[], &[]);
        assert!(report.all_passed);
    }

    #[test]
    fn render_output() {
        let report = CleanupReport {
            removals: vec![RemovalResult {
                id: "RM-01".into(),
                path: "old.rs".into(),
                status: RemovalStatus::Removed,
                error: None,
            }],
            obligations: vec![],
            all_passed: true,
        };
        let out = render_cleanup(&report);
        assert!(out.contains("PASS"));
        assert!(out.contains("[OK]"));
    }
}
