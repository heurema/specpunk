//! Typed verify DSL — sandboxed step execution for holdouts and obligations.
//!
//! Three step types:
//! - Http: API checks (localhost only)
//! - Exec: whitelisted binaries only (no shell)
//! - Expect: assertions on captured results

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// DSL types
// ---------------------------------------------------------------------------

/// HTTP method for API checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
}

/// A single step in the verify DSL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DslStep {
    Http {
        method: HttpMethod,
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        headers: Vec<(String, String)>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capture: Option<String>,
    },
    Exec {
        argv: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capture: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
    },
    Expect {
        source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        json_path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        equals: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        contains: Option<String>,
    },
}

/// Result of executing a single step.
#[derive(Debug, Clone)]
pub struct StepResult {
    pub body: String,
    pub exit_code: i32,
    pub status_code: Option<u16>,
}

/// Result of running a complete DSL sequence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DslRunResult {
    pub passed: bool,
    pub steps_run: usize,
    pub failed_at_step: Option<usize>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

// ---------------------------------------------------------------------------
// Security: whitelists
// ---------------------------------------------------------------------------

/// Binaries allowed in exec steps. No shell interpreters.
const EXEC_WHITELIST: &[&str] = &[
    "test", "ls", "wc", "cat", "jq", "grep", "head", "tail",
    "diff", "stat", "file", "echo", "true", "false", "sort", "uniq",
    "curl",
];

/// Validate that a URL targets localhost only.
fn is_localhost_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    if let Some(after_scheme) = lower.strip_prefix("http://").or_else(|| lower.strip_prefix("https://")) {
        let host_port = after_scheme.split('/').next().unwrap_or("");
        let host = host_port.split(':').next().unwrap_or("");
        matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]" | "0.0.0.0")
    } else {
        false
    }
}

/// Validate that an exec argv[0] is whitelisted.
fn is_whitelisted_binary(bin: &str) -> bool {
    let basename = bin.rsplit('/').next().unwrap_or(bin);
    EXEC_WHITELIST.contains(&basename)
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate DSL steps before execution.
pub fn validate_steps(steps: &[DslStep]) -> Result<(), DslError> {
    let mut captures: Vec<String> = Vec::new();

    for (i, step) in steps.iter().enumerate() {
        match step {
            DslStep::Http { url, capture, .. } => {
                if !is_localhost_url(url) {
                    return Err(DslError::SecurityViolation(format!(
                        "step {}: URL must target localhost, got '{url}'", i + 1
                    )));
                }
                if let Some(name) = capture {
                    captures.push(name.clone());
                }
            }
            DslStep::Exec { argv, capture, .. } => {
                if argv.is_empty() {
                    return Err(DslError::InvalidStep(format!("step {}: exec argv is empty", i + 1)));
                }
                if !is_whitelisted_binary(&argv[0]) {
                    return Err(DslError::SecurityViolation(format!(
                        "step {}: binary '{}' not in whitelist. Allowed: {:?}",
                        i + 1, argv[0], EXEC_WHITELIST
                    )));
                }
                if let Some(name) = capture {
                    captures.push(name.clone());
                }
            }
            DslStep::Expect { source, .. } => {
                if !captures.contains(source) {
                    return Err(DslError::InvalidStep(format!(
                        "step {}: expect references capture '{source}' which hasn't been defined yet. \
                         Available: {:?}", i + 1, captures
                    )));
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum DslError {
    SecurityViolation(String),
    InvalidStep(String),
    ExecutionFailed(String),
    Timeout(usize),
    ExpectFailed { step: usize, expected: String, actual: String },
}

impl std::fmt::Display for DslError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DslError::SecurityViolation(m) => write!(f, "security: {m}"),
            DslError::InvalidStep(m) => write!(f, "invalid step: {m}"),
            DslError::ExecutionFailed(m) => write!(f, "execution failed: {m}"),
            DslError::Timeout(step) => write!(f, "step {step} timed out"),
            DslError::ExpectFailed { step, expected, actual } => {
                write!(f, "step {step}: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for DslError {}

// ---------------------------------------------------------------------------
// Executor
// ---------------------------------------------------------------------------

/// Run a sequence of DSL steps. Returns overall result.
/// Uses only whitelisted binaries via Command::new (no shell, no injection).
pub fn run_steps(steps: &[DslStep], working_dir: &Path) -> DslRunResult {
    let start = std::time::Instant::now();

    if let Err(e) = validate_steps(steps) {
        return DslRunResult {
            passed: false,
            steps_run: 0,
            failed_at_step: Some(0),
            error: Some(e.to_string()),
            duration_ms: start.elapsed().as_millis() as u64,
        };
    }

    let mut captures: HashMap<String, StepResult> = HashMap::new();

    for (i, step) in steps.iter().enumerate() {
        let step_num = i + 1;
        match execute_step(step, &captures, working_dir) {
            Ok(result) => {
                let capture_name = match step {
                    DslStep::Http { capture, .. } => capture.clone(),
                    DslStep::Exec { capture, .. } => capture.clone(),
                    DslStep::Expect { .. } => None,
                };
                if let Some(name) = capture_name {
                    captures.insert(name, result);
                }
            }
            Err(e) => {
                return DslRunResult {
                    passed: false,
                    steps_run: step_num,
                    failed_at_step: Some(step_num),
                    error: Some(e.to_string()),
                    duration_ms: start.elapsed().as_millis() as u64,
                };
            }
        }
    }

    DslRunResult {
        passed: true,
        steps_run: steps.len(),
        failed_at_step: None,
        error: None,
        duration_ms: start.elapsed().as_millis() as u64,
    }
}

fn execute_step(
    step: &DslStep,
    captures: &HashMap<String, StepResult>,
    working_dir: &Path,
) -> Result<StepResult, DslError> {
    match step {
        DslStep::Http { method, url, body, .. } => {
            execute_http(method, url, body.as_deref())
        }
        DslStep::Exec { argv, .. } => {
            execute_exec(argv, working_dir)
        }
        DslStep::Expect { source, json_path, exit_code, equals, contains } => {
            let captured = captures.get(source).ok_or_else(|| {
                DslError::InvalidStep(format!("capture '{source}' not found"))
            })?;
            execute_expect(captured, json_path.as_deref(), *exit_code, equals.as_ref(), contains.as_deref())
        }
    }
}

/// Execute HTTP step via curl (Command::new, no shell).
fn execute_http(
    method: &HttpMethod,
    url: &str,
    body: Option<&str>,
) -> Result<StepResult, DslError> {
    let method_str = match method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Patch => "PATCH",
        HttpMethod::Head => "HEAD",
    };

    let mut cmd = Command::new("curl");
    cmd.args(["-s", "-w", "\n%{http_code}", "-X", method_str]);

    if let Some(b) = body {
        cmd.args(["-H", "Content-Type: application/json", "-d", b]);
    }

    cmd.arg(url);

    let output = cmd.output()
        .map_err(|e| DslError::ExecutionFailed(format!("curl: {e}")))?;

    let full_output = String::from_utf8_lossy(&output.stdout).to_string();
    let lines: Vec<&str> = full_output.trim_end().rsplitn(2, '\n').collect();
    let (status_str, body_str) = if lines.len() == 2 {
        (lines[0], lines[1])
    } else {
        (lines[0], "")
    };
    let status_code: u16 = status_str.trim().parse().unwrap_or(0);

    Ok(StepResult {
        body: body_str.to_string(),
        exit_code: output.status.code().unwrap_or(-1),
        status_code: Some(status_code),
    })
}

/// Execute whitelisted binary via Command::new (no shell).
fn execute_exec(argv: &[String], working_dir: &Path) -> Result<StepResult, DslError> {
    let mut cmd = Command::new(&argv[0]);
    if argv.len() > 1 {
        cmd.args(&argv[1..]);
    }
    cmd.current_dir(working_dir);

    let output = cmd.output()
        .map_err(|e| DslError::ExecutionFailed(format!("{}: {e}", argv[0])))?;

    Ok(StepResult {
        body: String::from_utf8_lossy(&output.stdout).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
        status_code: None,
    })
}

fn execute_expect(
    captured: &StepResult,
    json_path: Option<&str>,
    expected_exit: Option<i32>,
    equals: Option<&serde_json::Value>,
    contains: Option<&str>,
) -> Result<StepResult, DslError> {
    if let Some(expected) = expected_exit {
        if captured.exit_code != expected {
            return Err(DslError::ExpectFailed {
                step: 0,
                expected: format!("exit_code={expected}"),
                actual: format!("exit_code={}", captured.exit_code),
            });
        }
    }

    if let Some(equals_val) = equals {
        if let Some(path) = json_path {
            let parsed: serde_json::Value = serde_json::from_str(&captured.body)
                .map_err(|e| DslError::ExecutionFailed(format!("JSON parse: {e}")))?;

            let extracted = navigate_json_path(&parsed, path)
                .ok_or_else(|| DslError::ExpectFailed {
                    step: 0,
                    expected: format!("value at {path}"),
                    actual: "path not found".to_string(),
                })?;

            if extracted != equals_val {
                return Err(DslError::ExpectFailed {
                    step: 0,
                    expected: format!("{equals_val}"),
                    actual: format!("{extracted}"),
                });
            }
        } else if let Some(status) = captured.status_code {
            if let Some(expected_num) = equals_val.as_u64() {
                if status as u64 != expected_num {
                    return Err(DslError::ExpectFailed {
                        step: 0,
                        expected: format!("{expected_num}"),
                        actual: format!("{status}"),
                    });
                }
            }
        }
    }

    if let Some(needle) = contains {
        if !captured.body.contains(needle) {
            return Err(DslError::ExpectFailed {
                step: 0,
                expected: format!("body contains '{needle}'"),
                actual: format!("body: '{}'", &captured.body[..captured.body.len().min(200)]),
            });
        }
    }

    Ok(StepResult {
        body: captured.body.clone(),
        exit_code: captured.exit_code,
        status_code: captured.status_code,
    })
}

/// Simple JSON path navigator. Supports `$.field.nested[0].key` syntax.
fn navigate_json_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let path = path.strip_prefix("$.").unwrap_or(path.strip_prefix("$").unwrap_or(path));
    let mut current = value;

    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        if let Some(bracket_pos) = segment.find('[') {
            let field = &segment[..bracket_pos];
            let idx_str = &segment[bracket_pos + 1..segment.len() - 1];
            if !field.is_empty() {
                current = current.get(field)?;
            }
            let idx: usize = idx_str.parse().ok()?;
            current = current.get(idx)?;
        } else {
            current = current.get(segment)?;
        }
    }

    Some(current)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn localhost_validation() {
        assert!(is_localhost_url("http://localhost:8000/api"));
        assert!(is_localhost_url("http://127.0.0.1:3000/health"));
        assert!(is_localhost_url("http://0.0.0.0:8080/"));
        assert!(!is_localhost_url("http://example.com/api"));
        assert!(!is_localhost_url("http://evil.localhost.com/"));
        assert!(!is_localhost_url("ftp://localhost/"));
    }

    #[test]
    fn whitelist_validation() {
        assert!(is_whitelisted_binary("test"));
        assert!(is_whitelisted_binary("jq"));
        assert!(is_whitelisted_binary("/usr/bin/grep"));
        assert!(!is_whitelisted_binary("bash"));
        assert!(!is_whitelisted_binary("sh"));
        assert!(!is_whitelisted_binary("rm"));
        assert!(!is_whitelisted_binary("eval"));
    }

    #[test]
    fn validate_rejects_external_url() {
        let steps = vec![DslStep::Http {
            method: HttpMethod::Get,
            url: "http://evil.com/steal".to_string(),
            body: None,
            headers: vec![],
            capture: None,
        }];
        assert!(validate_steps(&steps).is_err());
    }

    #[test]
    fn validate_rejects_unwhitelisted_binary() {
        let steps = vec![DslStep::Exec {
            argv: vec!["rm".to_string(), "-rf".to_string(), "/".to_string()],
            capture: None,
            timeout_ms: None,
        }];
        assert!(validate_steps(&steps).is_err());
    }

    #[test]
    fn validate_rejects_forward_reference() {
        let steps = vec![DslStep::Expect {
            source: "nonexistent".to_string(),
            json_path: None,
            exit_code: Some(0),
            equals: None,
            contains: None,
        }];
        assert!(validate_steps(&steps).is_err());
    }

    #[test]
    fn validate_accepts_valid_sequence() {
        let steps = vec![
            DslStep::Exec {
                argv: vec!["echo".to_string(), "hello".to_string()],
                capture: Some("r".to_string()),
                timeout_ms: None,
            },
            DslStep::Expect {
                source: "r".to_string(),
                json_path: None,
                exit_code: Some(0),
                equals: None,
                contains: Some("hello".to_string()),
            },
        ];
        assert!(validate_steps(&steps).is_ok());
    }

    #[test]
    fn exec_echo_and_expect() {
        let tmp = TempDir::new().unwrap();
        let steps = vec![
            DslStep::Exec {
                argv: vec!["echo".to_string(), "hello world".to_string()],
                capture: Some("r".to_string()),
                timeout_ms: None,
            },
            DslStep::Expect {
                source: "r".to_string(),
                json_path: None,
                exit_code: Some(0),
                equals: None,
                contains: Some("hello".to_string()),
            },
        ];
        let result = run_steps(&steps, tmp.path());
        assert!(result.passed, "should pass: {:?}", result.error);
        assert_eq!(result.steps_run, 2);
    }

    #[test]
    fn exec_test_file_exists() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("exists.txt"), "data").unwrap();

        let steps = vec![
            DslStep::Exec {
                argv: vec!["test".to_string(), "-f".to_string(), "exists.txt".to_string()],
                capture: Some("f".to_string()),
                timeout_ms: None,
            },
            DslStep::Expect {
                source: "f".to_string(),
                json_path: None,
                exit_code: Some(0),
                equals: None,
                contains: None,
            },
        ];
        let result = run_steps(&steps, tmp.path());
        assert!(result.passed);
    }

    #[test]
    fn exec_test_file_missing_fails() {
        let tmp = TempDir::new().unwrap();
        let steps = vec![
            DslStep::Exec {
                argv: vec!["test".to_string(), "-f".to_string(), "missing.txt".to_string()],
                capture: Some("f".to_string()),
                timeout_ms: None,
            },
            DslStep::Expect {
                source: "f".to_string(),
                json_path: None,
                exit_code: Some(0),
                equals: None,
                contains: None,
            },
        ];
        let result = run_steps(&steps, tmp.path());
        assert!(!result.passed);
    }

    #[test]
    fn json_path_navigation() {
        let val = serde_json::json!({
            "users": [{"id": 1, "name": "alice"}, {"id": 2, "name": "bob"}],
            "status": "ok"
        });
        assert_eq!(navigate_json_path(&val, "$.status"), Some(&serde_json::json!("ok")));
        assert_eq!(navigate_json_path(&val, "$.users[0].name"), Some(&serde_json::json!("alice")));
        assert_eq!(navigate_json_path(&val, "$.users[1].id"), Some(&serde_json::json!(2)));
        assert_eq!(navigate_json_path(&val, "$.nonexistent"), None);
    }

    #[test]
    fn dsl_step_roundtrip() {
        let steps = vec![
            DslStep::Exec {
                argv: vec!["echo".to_string(), "test".to_string()],
                capture: Some("r".to_string()),
                timeout_ms: Some(3000),
            },
            DslStep::Expect {
                source: "r".to_string(),
                json_path: None,
                exit_code: Some(0),
                equals: None,
                contains: Some("test".to_string()),
            },
        ];
        let json = serde_json::to_string_pretty(&steps).unwrap();
        let back: Vec<DslStep> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn security_rejects_shell_interpreters() {
        for bin in &["bash", "sh", "zsh", "cmd", "powershell", "python3", "node", "ruby"] {
            assert!(!is_whitelisted_binary(bin), "{bin} should be rejected");
        }
    }
}
