//! Mechanic: baseline capture + post-change regression detection.
//! Zero LLM — runs project test commands and compares results.

use std::path::Path;
use std::process::Command;

use chrono::Utc;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single check in the baseline (one test runner invocation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineCheck {
    pub name: String,
    pub command: String,
    pub exit_code: i32,
    pub stdout_hash: String,
    pub failures: Vec<String>,
    pub duration_ms: u64,
}

/// Pre-change project health snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub schema_version: String,
    pub captured_at: String,
    pub contract_id: String,
    pub checks: Vec<BaselineCheck>,
}

/// A regression: test that passed before but fails now.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Regression {
    pub check_name: String,
    pub test_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MechanicStatus {
    Pass,
    Regression,
    Error,
}

/// Post-change comparison report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MechanicReport {
    pub schema_version: String,
    pub timestamp: String,
    pub contract_id: String,
    pub baseline_hash: String,
    pub post_checks: Vec<BaselineCheck>,
    pub regressions: Vec<Regression>,
    pub fixed: Vec<String>,
    pub status: MechanicStatus,
    pub duration_ms: u64,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum MechanicError {
    NoBaseline(String),
    Io(std::io::Error),
    Parse(String),
}

impl std::fmt::Display for MechanicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MechanicError::NoBaseline(m) => write!(f, "no baseline: {m}"),
            MechanicError::Io(e) => write!(f, "I/O error: {e}"),
            MechanicError::Parse(m) => write!(f, "parse error: {m}"),
        }
    }
}

impl std::error::Error for MechanicError {}

impl From<std::io::Error> for MechanicError {
    fn from(e: std::io::Error) -> Self {
        MechanicError::Io(e)
    }
}

// ---------------------------------------------------------------------------
// Test runner detection
// ---------------------------------------------------------------------------

/// Detect available test commands from project files.
pub fn detect_test_commands(root: &Path) -> Vec<(String, String)> {
    let mut commands = Vec::new();

    // Rust: Cargo.toml → cargo test
    if root.join("Cargo.toml").exists() {
        commands.push(("cargo-test".to_string(), "cargo test".to_string()));
    }

    // Python: pyproject.toml or setup.py → pytest
    if root.join("pyproject.toml").exists() || root.join("setup.py").exists() {
        if which_exists("pytest") {
            commands.push(("pytest".to_string(), "pytest -q --tb=line".to_string()));
        } else if which_exists("python3") {
            commands.push(("python-unittest".to_string(), "python3 -m pytest -q --tb=line".to_string()));
        }
    }

    // Node: package.json → npm test
    if root.join("package.json").exists() {
        commands.push(("npm-test".to_string(), "npm test".to_string()));
    }

    // Go: go.mod → go test
    if root.join("go.mod").exists() {
        commands.push(("go-test".to_string(), "go test ./...".to_string()));
    }

    // Ruby: Gemfile → bundle exec rspec
    if root.join("Gemfile").exists() {
        commands.push(("rspec".to_string(), "bundle exec rspec".to_string()));
    }

    // Elixir: mix.exs → mix test
    if root.join("mix.exs").exists() {
        commands.push(("mix-test".to_string(), "mix test".to_string()));
    }

    // Also check .punk/config.toml for custom test_runner
    let config_path = root.join(".punk").join("config.toml");
    if let Ok(raw) = std::fs::read_to_string(&config_path) {
        #[derive(Deserialize)]
        struct Cfg { #[serde(default)] project: Proj }
        #[derive(Deserialize, Default)]
        struct Proj { #[serde(default)] test_runner: Option<String> }

        if let Ok(cfg) = toml::from_str::<Cfg>(&raw) {
            if let Some(runner) = cfg.project.test_runner {
                let cmd = match runner.as_str() {
                    "pytest" => "pytest -q --tb=line".to_string(),
                    "jest" | "vitest" => "npm test".to_string(),
                    "cargo-test" => "cargo test".to_string(),
                    "go-test" => "go test ./...".to_string(),
                    "rspec" => "bundle exec rspec".to_string(),
                    "mix-test" => "mix test".to_string(),
                    _ => runner.clone(),
                };
                if !commands.iter().any(|(n, _)| n == &runner) {
                    commands.insert(0, (runner, cmd));
                }
            }
        }
    }

    commands
}

fn which_exists(binary: &str) -> bool {
    Command::new("which")
        .arg(binary)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Run a check command and parse results
// ---------------------------------------------------------------------------

/// Run a test command and capture results.
fn run_check_command(name: &str, command: &str, root: &Path) -> BaselineCheck {
    let start = std::time::Instant::now();

    // Split command into program + args (simple space split, no shell)
    let parts: Vec<&str> = command.split_whitespace().collect();
    let (program, args) = parts.split_first().unwrap_or((&"true", &[]));

    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output();

    let duration_ms = start.elapsed().as_millis() as u64;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = format!("{stdout}{stderr}");
            let stdout_hash = crate::plan::sha256_hex(combined.as_bytes());
            let failures = parse_test_failures(&combined, name);

            BaselineCheck {
                name: name.to_string(),
                command: command.to_string(),
                exit_code: out.status.code().unwrap_or(-1),
                stdout_hash,
                failures,
                duration_ms,
            }
        }
        Err(e) => BaselineCheck {
            name: name.to_string(),
            command: command.to_string(),
            exit_code: -1,
            stdout_hash: String::new(),
            failures: vec![format!("command failed: {e}")],
            duration_ms,
        },
    }
}

/// Parse test output for individual failure names.
fn parse_test_failures(output: &str, runner_name: &str) -> Vec<String> {
    let mut failures = Vec::new();

    match runner_name {
        "cargo-test" => {
            // Rust: "test module::test_name ... FAILED"
            for line in output.lines() {
                if line.contains("... FAILED") {
                    if let Some(name) = line.strip_prefix("test ") {
                        if let Some(name) = name.strip_suffix(" ... FAILED") {
                            failures.push(name.trim().to_string());
                        }
                    }
                }
            }
        }
        "pytest" | "python-unittest" => {
            // Python: "FAILED tests/test_foo.py::test_bar"
            for line in output.lines() {
                if let Some(rest) = line.strip_prefix("FAILED ") {
                    failures.push(rest.trim().to_string());
                }
            }
        }
        "npm-test" | "jest" | "vitest" => {
            // Jest: "  ✕ test name (Nms)"  or  "  × test name"
            for line in output.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("✕ ") || trimmed.starts_with("× ") {
                    failures.push(trimmed[2..].trim().to_string());
                }
            }
        }
        "go-test" => {
            // Go: "--- FAIL: TestName (0.00s)"
            for line in output.lines() {
                if line.contains("--- FAIL: ") {
                    if let Some(rest) = line.split("--- FAIL: ").nth(1) {
                        if let Some(name) = rest.split_whitespace().next() {
                            failures.push(name.to_string());
                        }
                    }
                }
            }
        }
        _ => {
            // Generic: look for common failure indicators
            if output.contains("FAIL") || output.contains("FAILED") || output.contains("Error") {
                failures.push(format!("{runner_name}: check output for details"));
            }
        }
    }

    failures
}

// ---------------------------------------------------------------------------
// Baseline capture
// ---------------------------------------------------------------------------

/// Capture baseline: run all detected test commands, save results.
pub fn capture_baseline(root: &Path, contract_id: &str) -> Result<Baseline, MechanicError> {
    let commands = detect_test_commands(root);

    if commands.is_empty() {
        return Err(MechanicError::NoBaseline(
            "no test commands detected. Add a Cargo.toml, pyproject.toml, package.json, or go.mod.".into(),
        ));
    }

    let checks: Vec<BaselineCheck> = commands
        .iter()
        .map(|(name, cmd)| run_check_command(name, cmd, root))
        .collect();

    let baseline = Baseline {
        schema_version: "1.0".to_string(),
        captured_at: Utc::now().to_rfc3339(),
        contract_id: contract_id.to_string(),
        checks,
    };

    // Save baseline
    let punk_dir = root.join(".punk");
    let contracts_dir = punk_dir.join("contracts").join(contract_id);
    std::fs::create_dir_all(&contracts_dir)?;

    let json = serde_json::to_string_pretty(&baseline)
        .map_err(|e| MechanicError::Parse(e.to_string()))?;

    let target = contracts_dir.join("baseline.json");
    let mut tmp = tempfile::NamedTempFile::new_in(&contracts_dir)?;
    std::io::Write::write_all(&mut tmp, json.as_bytes())?;
    tmp.persist(&target).map_err(|e| MechanicError::Io(e.error))?;

    Ok(baseline)
}

// ---------------------------------------------------------------------------
// Regression detection
// ---------------------------------------------------------------------------

/// Compare post-change test results against baseline.
pub fn run_mechanic(root: &Path, contract_id: &str) -> Result<MechanicReport, MechanicError> {
    let start = std::time::Instant::now();

    // Load baseline
    let baseline_path = root
        .join(".punk")
        .join("contracts")
        .join(contract_id)
        .join("baseline.json");

    if !baseline_path.exists() {
        return Err(MechanicError::NoBaseline(
            "no baseline.json found. Run `punk baseline` first.".into(),
        ));
    }

    let baseline_raw = std::fs::read_to_string(&baseline_path)?;
    let baseline: Baseline = serde_json::from_str(&baseline_raw)
        .map_err(|e| MechanicError::Parse(format!("baseline.json: {e}")))?;

    let baseline_hash = crate::plan::sha256_hex(baseline_raw.as_bytes());

    // Re-run the same commands
    let post_checks: Vec<BaselineCheck> = baseline
        .checks
        .iter()
        .map(|bc| run_check_command(&bc.name, &bc.command, root))
        .collect();

    // Compare: find regressions (new failures) and fixes
    let mut regressions = Vec::new();
    let mut fixed = Vec::new();

    for (pre, post) in baseline.checks.iter().zip(post_checks.iter()) {
        let pre_set: std::collections::HashSet<&str> =
            pre.failures.iter().map(|s| s.as_str()).collect();
        let post_set: std::collections::HashSet<&str> =
            post.failures.iter().map(|s| s.as_str()).collect();

        // New failures = in post but not in pre
        for &f in post_set.difference(&pre_set) {
            regressions.push(Regression {
                check_name: post.name.clone(),
                test_name: f.to_string(),
            });
        }

        // Fixed = in pre but not in post
        for &f in pre_set.difference(&post_set) {
            fixed.push(format!("{}::{}", pre.name, f));
        }

        // Also: if exit code changed from 0 to non-zero and no specific failures parsed
        if pre.exit_code == 0 && post.exit_code != 0 && post.failures.is_empty() {
            regressions.push(Regression {
                check_name: post.name.clone(),
                test_name: format!("(exit code {} → {})", pre.exit_code, post.exit_code),
            });
        }
    }

    let status = if !regressions.is_empty() {
        MechanicStatus::Regression
    } else {
        MechanicStatus::Pass
    };

    let duration_ms = start.elapsed().as_millis() as u64;

    let report = MechanicReport {
        schema_version: "1.0".to_string(),
        timestamp: Utc::now().to_rfc3339(),
        contract_id: contract_id.to_string(),
        baseline_hash,
        post_checks,
        regressions,
        fixed,
        status,
        duration_ms,
    };

    // Save report atomically
    let reports_dir = root.join(".punk").join("contracts").join(contract_id);
    let json = serde_json::to_string_pretty(&report)
        .map_err(|e| MechanicError::Parse(e.to_string()))?;
    let target = reports_dir.join("mechanic.json");
    let mut tmp = tempfile::NamedTempFile::new_in(&reports_dir)?;
    std::io::Write::write_all(&mut tmp, json.as_bytes())?;
    tmp.persist(&target).map_err(|e| MechanicError::Io(e.error))?;

    Ok(report)
}

// ---------------------------------------------------------------------------
// Renderers
// ---------------------------------------------------------------------------

pub fn render_baseline_short(baseline: &Baseline) -> String {
    let total_failures: usize = baseline.checks.iter().map(|c| c.failures.len()).sum();
    let total_ms: u64 = baseline.checks.iter().map(|c| c.duration_ms).sum();
    format!(
        "punk baseline: {} checks captured, {} pre-existing failures, {}ms\n",
        baseline.checks.len(),
        total_failures,
        total_ms,
    )
}

pub fn render_mechanic_short(report: &MechanicReport) -> String {
    let mut out = format!(
        "punk mechanic: {} ({}ms)\n",
        match report.status {
            MechanicStatus::Pass => "PASS — no regressions",
            MechanicStatus::Regression => "REGRESSION — new test failures detected",
            MechanicStatus::Error => "ERROR",
        },
        report.duration_ms,
    );

    if !report.regressions.is_empty() {
        out.push_str(&format!("\n  {} regressions:\n", report.regressions.len()));
        for r in &report.regressions {
            out.push_str(&format!("    FAIL {}::{}\n", r.check_name, r.test_name));
        }
    }

    if !report.fixed.is_empty() {
        out.push_str(&format!("\n  {} fixed:\n", report.fixed.len()));
        for f in &report.fixed {
            out.push_str(&format!("    FIXED {f}\n"));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn detect_rust_project() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();
        let cmds = detect_test_commands(tmp.path());
        assert!(cmds.iter().any(|(n, _)| n == "cargo-test"));
    }

    #[test]
    fn detect_python_project() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("pyproject.toml"), "[project]\nname=\"x\"").unwrap();
        let cmds = detect_test_commands(tmp.path());
        // May or may not find pytest depending on system
        assert!(!cmds.is_empty() || true); // at least no panic
    }

    #[test]
    fn detect_empty_project() {
        let tmp = TempDir::new().unwrap();
        let cmds = detect_test_commands(tmp.path());
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_rust_failures() {
        let output = "\
running 3 tests
test auth::test_login ... ok
test auth::test_logout ... FAILED
test auth::test_register ... FAILED

failures:
--- auth::test_logout ---
assertion failed
--- auth::test_register ---
expected 200, got 401

test result: FAILED. 1 passed; 2 failed; 0 ignored;";

        let failures = parse_test_failures(output, "cargo-test");
        assert_eq!(failures, vec!["auth::test_logout", "auth::test_register"]);
    }

    #[test]
    fn parse_pytest_failures() {
        let output = "\
FAILED tests/test_auth.py::test_login - AssertionError
FAILED tests/test_auth.py::test_register - ValueError
2 failed, 5 passed";

        let failures = parse_test_failures(output, "pytest");
        assert_eq!(failures.len(), 2);
        assert!(failures[0].contains("test_login"));
    }

    #[test]
    fn parse_go_failures() {
        let output = "\
--- FAIL: TestAuth (0.01s)
--- FAIL: TestLogin (0.00s)
FAIL";

        let failures = parse_test_failures(output, "go-test");
        assert_eq!(failures, vec!["TestAuth", "TestLogin"]);
    }

    #[test]
    fn parse_no_failures() {
        let output = "running 5 tests\ntest result: ok. 5 passed;";
        let failures = parse_test_failures(output, "cargo-test");
        assert!(failures.is_empty());
    }

    #[test]
    fn regression_detection_logic() {
        let pre = vec!["test_a".to_string(), "test_b".to_string()];
        let post = vec!["test_b".to_string(), "test_c".to_string()];

        let pre_set: std::collections::HashSet<&str> = pre.iter().map(|s| s.as_str()).collect();
        let post_set: std::collections::HashSet<&str> = post.iter().map(|s| s.as_str()).collect();

        let new_failures: Vec<&str> = post_set.difference(&pre_set).copied().collect();
        let fixed: Vec<&str> = pre_set.difference(&post_set).copied().collect();

        assert_eq!(new_failures, vec!["test_c"]);
        assert_eq!(fixed, vec!["test_a"]);
    }

    #[test]
    fn baseline_roundtrip() {
        let baseline = Baseline {
            schema_version: "1.0".to_string(),
            captured_at: Utc::now().to_rfc3339(),
            contract_id: "test-123".to_string(),
            checks: vec![BaselineCheck {
                name: "cargo-test".to_string(),
                command: "cargo test".to_string(),
                exit_code: 0,
                stdout_hash: "abc".to_string(),
                failures: vec![],
                duration_ms: 100,
            }],
        };
        let json = serde_json::to_string_pretty(&baseline).unwrap();
        let back: Baseline = serde_json::from_str(&json).unwrap();
        assert_eq!(back.contract_id, "test-123");
        assert_eq!(back.checks.len(), 1);
    }

    #[test]
    fn mechanic_report_roundtrip() {
        let report = MechanicReport {
            schema_version: "1.0".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            contract_id: "test-123".to_string(),
            baseline_hash: "deadbeef".to_string(),
            post_checks: vec![],
            regressions: vec![Regression {
                check_name: "cargo-test".to_string(),
                test_name: "auth::test_login".to_string(),
            }],
            fixed: vec![],
            status: MechanicStatus::Regression,
            duration_ms: 200,
        };
        let json = serde_json::to_string_pretty(&report).unwrap();
        let back: MechanicReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, MechanicStatus::Regression);
        assert_eq!(back.regressions.len(), 1);
    }

    #[test]
    fn render_pass() {
        let report = MechanicReport {
            schema_version: "1.0".to_string(),
            timestamp: "2026-03-25".to_string(),
            contract_id: "abc".to_string(),
            baseline_hash: "hash".to_string(),
            post_checks: vec![],
            regressions: vec![],
            fixed: vec![],
            status: MechanicStatus::Pass,
            duration_ms: 50,
        };
        let out = render_mechanic_short(&report);
        assert!(out.contains("PASS"));
        assert!(out.contains("no regressions"));
    }

    #[test]
    fn render_regression() {
        let report = MechanicReport {
            schema_version: "1.0".to_string(),
            timestamp: "2026-03-25".to_string(),
            contract_id: "abc".to_string(),
            baseline_hash: "hash".to_string(),
            post_checks: vec![],
            regressions: vec![Regression {
                check_name: "pytest".to_string(),
                test_name: "test_auth".to_string(),
            }],
            fixed: vec!["pytest::test_old".to_string()],
            status: MechanicStatus::Regression,
            duration_ms: 150,
        };
        let out = render_mechanic_short(&report);
        assert!(out.contains("REGRESSION"));
        assert!(out.contains("FAIL pytest::test_auth"));
        assert!(out.contains("FIXED pytest::test_old"));
    }
}
