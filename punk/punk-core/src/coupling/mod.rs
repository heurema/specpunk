//! Phase 16: Temporal coupling — "when you change X, you usually also change Y."
//! Mine co-change patterns from git/jj log.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouplingResult {
    pub file: String,
    pub coupled_files: Vec<CoupledFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoupledFile {
    pub path: String,
    pub co_changes: usize,
    pub total_changes: usize,
    pub confidence: f64,
}

// ---------------------------------------------------------------------------
// Mining
// ---------------------------------------------------------------------------

/// Mine co-change patterns for a given file from git log.
pub fn find_coupling(root: &Path, file: &str, min_confidence: f64) -> CouplingResult {
    let commits = get_commits_for_file(root, file);
    if commits.is_empty() {
        return CouplingResult {
            file: file.to_string(),
            coupled_files: vec![],
        };
    }

    let mut co_change_counts: HashMap<String, usize> = HashMap::new();
    let total_commits = commits.len();

    for commit in &commits {
        let files = get_files_in_commit(root, commit);
        for f in files {
            if f != file && !is_noise_file(&f) {
                *co_change_counts.entry(f).or_insert(0) += 1;
            }
        }
    }

    let mut coupled: Vec<CoupledFile> = co_change_counts
        .into_iter()
        .map(|(path, count)| {
            let confidence = count as f64 / total_commits as f64;
            CoupledFile {
                path,
                co_changes: count,
                total_changes: total_commits,
                confidence,
            }
        })
        .filter(|c| c.confidence >= min_confidence)
        .collect();

    coupled.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    coupled.truncate(10);

    CouplingResult {
        file: file.to_string(),
        coupled_files: coupled,
    }
}

fn get_commits_for_file(root: &Path, file: &str) -> Vec<String> {
    let output = Command::new("git")
        .args([
            "-c",
            "core.quotepath=false",
            "log",
            "--format=%H",
            "--follow",
            "-n",
            "100",
            "--",
            file,
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

fn get_files_in_commit(root: &Path, commit: &str) -> Vec<String> {
    let output = Command::new("git")
        .args([
            "-c",
            "core.quotepath=false",
            "diff-tree",
            "--no-commit-id",
            "-r",
            "--name-only",
            commit,
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

/// Filter out generated/noise files.
fn is_noise_file(file: &str) -> bool {
    let noise = [
        "Cargo.lock",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "go.sum",
        "poetry.lock",
        "Pipfile.lock",
        "Gemfile.lock",
    ];
    noise.iter().any(|n| file.ends_with(n))
        || file.contains("generated")
        || file.contains("__pycache__")
}

pub fn render_coupling(result: &CouplingResult) -> String {
    if result.coupled_files.is_empty() {
        return format!(
            "punk coupling: no co-change patterns found for {}\n",
            result.file
        );
    }

    let mut out = format!(
        "punk coupling: {} — {} coupled files\n\n",
        result.file,
        result.coupled_files.len()
    );
    for c in &result.coupled_files {
        out.push_str(&format!(
            "  {:.0}% {} ({}/{} commits)\n",
            c.confidence * 100.0,
            c.path,
            c.co_changes,
            c.total_changes,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_filter() {
        assert!(is_noise_file("Cargo.lock"));
        assert!(is_noise_file("node_modules/package-lock.json"));
        assert!(!is_noise_file("src/auth.rs"));
    }

    #[test]
    fn empty_coupling() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = find_coupling(tmp.path(), "nonexistent.rs", 0.1);
        assert!(result.coupled_files.is_empty());
    }

    #[test]
    fn render_empty() {
        let result = CouplingResult {
            file: "x.rs".into(),
            coupled_files: vec![],
        };
        let out = render_coupling(&result);
        assert!(out.contains("no co-change"));
    }

    #[test]
    fn render_with_results() {
        let result = CouplingResult {
            file: "src/auth.rs".into(),
            coupled_files: vec![CoupledFile {
                path: "src/middleware.rs".into(),
                co_changes: 8,
                total_changes: 10,
                confidence: 0.8,
            }],
        };
        let out = render_coupling(&result);
        assert!(out.contains("80%"));
        assert!(out.contains("middleware.rs"));
    }

    #[test]
    fn coupling_roundtrip() {
        let result = CouplingResult {
            file: "a.rs".into(),
            coupled_files: vec![CoupledFile {
                path: "b.rs".into(),
                co_changes: 5,
                total_changes: 10,
                confidence: 0.5,
            }],
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: CouplingResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.coupled_files[0].confidence, 0.5);
    }
}
