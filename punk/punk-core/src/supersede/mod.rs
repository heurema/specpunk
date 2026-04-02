//! Ghost function detection via jj-supersede (D-004).
//! Shells out to jj CLI for predecessor chain analysis.
//! Graceful degradation: if jj or jj-supersede unavailable, returns empty.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhostFunction {
    pub file: String,
    pub name: String,
    pub reason: String,
    pub last_modified_change: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupersedeReport {
    pub jj_available: bool,
    pub ghosts: Vec<GhostFunction>,
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Check if jj CLI is available.
pub fn jj_available() -> bool {
    Command::new("jj")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Detect superseded (ghost) functions using jj evolog.
/// Returns empty report if jj is not available (graceful degradation per D-004).
pub fn detect_ghosts(root: &Path) -> SupersedeReport {
    if !jj_available() {
        return SupersedeReport {
            jj_available: false,
            ghosts: vec![],
        };
    }

    // Check if this is a jj repo
    let is_jj = Command::new("jj")
        .arg("root")
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !is_jj {
        return SupersedeReport {
            jj_available: true,
            ghosts: vec![],
        };
    }

    // Get predecessor chain for current change
    let evolog = Command::new("jj")
        .args(["evolog", "--no-graph", "-T", "change_id ++ \"\\n\""])
        .current_dir(root)
        .output();

    let predecessors: Vec<String> = match evolog {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect(),
        _ => {
            return SupersedeReport {
                jj_available: true,
                ghosts: vec![],
            }
        }
    };

    // For each predecessor, check what files were modified
    let mut ghosts = Vec::new();

    for pred in predecessors.iter().skip(1) {
        // Get files that were in predecessor but not in current
        let diff_output = Command::new("jj")
            .args(["diff", "--name-only", "--from", pred, "--to", "@"])
            .current_dir(root)
            .output();

        if let Ok(o) = diff_output {
            if o.status.success() {
                for file in String::from_utf8_lossy(&o.stdout).lines() {
                    if !file.is_empty() && !root.join(file).exists() {
                        // File existed in predecessor but not in current → potential ghost
                        ghosts.push(GhostFunction {
                            file: file.to_string(),
                            name: file.to_string(), // simplified: use file as name
                            reason: format!(
                                "file removed since change {}",
                                &pred[..8.min(pred.len())]
                            ),
                            last_modified_change: pred.clone(),
                        });
                    }
                }
            }
        }
    }

    SupersedeReport {
        jj_available: true,
        ghosts,
    }
}

pub fn render_ghosts(report: &SupersedeReport) -> String {
    if !report.jj_available {
        return "punk supersede: jj not available (graceful skip)\n".to_string();
    }
    if report.ghosts.is_empty() {
        return "punk supersede: no ghost functions detected\n".to_string();
    }

    let mut out = format!(
        "punk supersede: {} ghosts detected\n\n",
        report.ghosts.len()
    );
    for g in &report.ghosts {
        out.push_str(&format!(
            "  {} — {}\n    {}\n",
            g.file, g.reason, g.last_modified_change
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graceful_no_jj() {
        // On systems without jj, should return gracefully
        let tmp = tempfile::TempDir::new().unwrap();
        let report = detect_ghosts(tmp.path());
        // Either jj_available is false (no jj) or ghosts is empty (no jj repo)
        assert!(report.ghosts.is_empty());
    }

    #[test]
    fn render_no_jj() {
        let report = SupersedeReport {
            jj_available: false,
            ghosts: vec![],
        };
        let out = render_ghosts(&report);
        assert!(out.contains("not available"));
    }

    #[test]
    fn render_no_ghosts() {
        let report = SupersedeReport {
            jj_available: true,
            ghosts: vec![],
        };
        let out = render_ghosts(&report);
        assert!(out.contains("no ghost"));
    }

    #[test]
    fn render_with_ghosts() {
        let report = SupersedeReport {
            jj_available: true,
            ghosts: vec![GhostFunction {
                file: "src/old_module.rs".into(),
                name: "old_function".into(),
                reason: "removed since change abc12345".into(),
                last_modified_change: "abc12345".into(),
            }],
        };
        let out = render_ghosts(&report);
        assert!(out.contains("1 ghosts"));
        assert!(out.contains("old_module"));
    }

    #[test]
    fn report_roundtrip() {
        let r = SupersedeReport {
            jj_available: true,
            ghosts: vec![],
        };
        let j = serde_json::to_string(&r).unwrap();
        let back: SupersedeReport = serde_json::from_str(&j).unwrap();
        assert!(back.jj_available);
    }
}
