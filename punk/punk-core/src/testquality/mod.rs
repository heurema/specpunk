//! Static test quality checks: assertion count, mock diversity,
//! tautological patterns, fabrication detection. Regex-based, <100ms.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestIssueSeverity { Error, Warning, Info }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestIssue {
    pub file: String,
    pub test_name: String,
    pub severity: TestIssueSeverity,
    pub check: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestQualityReport {
    pub files_scanned: usize,
    pub tests_found: usize,
    pub issues: Vec<TestIssue>,
    pub zero_assertion_count: usize,
    pub tautology_count: usize,
    pub mock_heavy_count: usize,
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

const ASSERTION_PATTERNS: &[&str] = &[
    "assert!", "assert_eq!", "assert_ne!", "debug_assert!",
    "assert ", "assertEqual", "assertRaises", "assertIn",
    "expect(", ".to_equal", ".to_be", ".toBe(", ".toEqual(",
    ".should.", "assert.equal", "assert.ok",
    "require.Equal", "require.NoError",
];

const MOCK_PATTERNS: &[&str] = &[
    "mock(", "Mock(", "MagicMock", "patch(", "mock!(",
    "jest.mock", "vi.mock", "sinon.stub", "mockall",
    "gomock", "testify/mock",
];

const TAUTOLOGY_PATTERNS: &[&str] = &[
    "assert!(true)", "assert_eq!(true, true)", "assert!(1 == 1)",
    "assertEqual(True, True)", "assert True",
    "expect(true).toBe(true)", "expect(1).toBe(1)",
];

const FABRICATION_PATTERNS: &[&str] = &[
    "assert!(err.is_err())",  // always true if err is Err
    "assert!(result.is_ok())", // might be vacuous
    "pass  # test",           // empty test body
    "return;  // skip",       // test that returns immediately
];

// ---------------------------------------------------------------------------
// Scanner
// ---------------------------------------------------------------------------

/// Scan changed test files for quality issues.
pub fn scan_test_files(root: &std::path::Path, files: &[String]) -> TestQualityReport {
    let mut issues = Vec::new();
    let mut files_scanned = 0;
    let mut tests_found = 0;
    let mut zero_assertion_count = 0;
    let mut tautology_count = 0;
    let mut mock_heavy_count = 0;

    for file in files {
        if !is_test_file(file) { continue; }

        let path = root.join(file);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        files_scanned += 1;
        let language = detect_language(file);
        let test_functions = extract_test_functions(&content, &language);
        tests_found += test_functions.len();

        for (name, body) in &test_functions {
            // Check 1: Zero assertions
            let assertion_count = count_patterns(body, ASSERTION_PATTERNS);
            if assertion_count == 0 {
                zero_assertion_count += 1;
                issues.push(TestIssue {
                    file: file.clone(), test_name: name.clone(),
                    severity: TestIssueSeverity::Error,
                    check: "zero_assertions".into(),
                    message: "test function has 0 assertions — likely vacuous".into(),
                });
            }

            // Check 2: Mock-heavy (more mock setup than assertions)
            let mock_count = count_patterns(body, MOCK_PATTERNS);
            if mock_count > 0 && mock_count > assertion_count {
                mock_heavy_count += 1;
                issues.push(TestIssue {
                    file: file.clone(), test_name: name.clone(),
                    severity: TestIssueSeverity::Warning,
                    check: "mock_heavy".into(),
                    message: format!("{mock_count} mocks vs {assertion_count} assertions — mock-dominated test"),
                });
            }

            // Check 3: Tautological assertions
            for pattern in TAUTOLOGY_PATTERNS {
                if body.contains(pattern) {
                    tautology_count += 1;
                    issues.push(TestIssue {
                        file: file.clone(), test_name: name.clone(),
                        severity: TestIssueSeverity::Warning,
                        check: "tautology".into(),
                        message: format!("tautological assertion: {pattern}"),
                    });
                    break;
                }
            }

            // Check 4: Fabrication patterns
            for pattern in FABRICATION_PATTERNS {
                if body.contains(pattern) {
                    issues.push(TestIssue {
                        file: file.clone(), test_name: name.clone(),
                        severity: TestIssueSeverity::Info,
                        check: "fabrication_suspect".into(),
                        message: format!("suspicious pattern: {pattern}"),
                    });
                    break;
                }
            }
        }
    }

    TestQualityReport {
        files_scanned, tests_found, issues,
        zero_assertion_count, tautology_count, mock_heavy_count,
    }
}

fn is_test_file(file: &str) -> bool {
    let lower = file.to_lowercase();
    lower.contains("test") || lower.contains("spec")
        || lower.ends_with("_test.go") || lower.ends_with("_test.rs")
}

fn detect_language(file: &str) -> String {
    if file.ends_with(".rs") { "rust".into() }
    else if file.ends_with(".py") { "python".into() }
    else if file.ends_with(".ts") || file.ends_with(".tsx") { "typescript".into() }
    else if file.ends_with(".js") || file.ends_with(".jsx") { "javascript".into() }
    else if file.ends_with(".go") { "go".into() }
    else { "unknown".into() }
}

/// Extract test function names and bodies from source code.
fn extract_test_functions(content: &str, language: &str) -> Vec<(String, String)> {
    let mut tests = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let is_test = match language {
            "rust" => trimmed == "#[test]" || trimmed == "#[tokio::test]",
            "python" => trimmed.starts_with("def test_") || trimmed.starts_with("async def test_"),
            "go" => trimmed.starts_with("func Test"),
            "typescript" | "javascript" => {
                trimmed.starts_with("it(") || trimmed.starts_with("test(")
                    || trimmed.starts_with("it('") || trimmed.starts_with("test('")
            }
            _ => false,
        };

        if is_test {
            // Collect function name
            let name = match language {
                "rust" => {
                    // Next line after #[test] has fn name
                    lines.get(i + 1)
                        .and_then(|l| l.trim().strip_prefix("fn "))
                        .and_then(|l| l.split('(').next())
                        .unwrap_or("unknown")
                        .to_string()
                }
                "python" => {
                    trimmed.strip_prefix("def ").or(trimmed.strip_prefix("async def "))
                        .and_then(|l| l.split('(').next())
                        .unwrap_or("unknown")
                        .to_string()
                }
                "go" => {
                    trimmed.strip_prefix("func ")
                        .and_then(|l| l.split('(').next())
                        .unwrap_or("unknown")
                        .to_string()
                }
                _ => {
                    // JS/TS: extract string from it('name') or test('name')
                    trimmed.split('\'').nth(1)
                        .or(trimmed.split('"').nth(1))
                        .unwrap_or("unknown")
                        .to_string()
                }
            };

            // Collect body until next test marker or end of file
            let body_start = if language == "rust" { i + 2 } else { i + 1 };
            let mut body_end = (body_start + 50).min(lines.len());
            for (j, ln) in lines.iter().enumerate().skip(body_start).take(body_end - body_start) {
                let t = ln.trim();
                if t == "#[test]" || t == "#[tokio::test]"
                    || t.starts_with("def test_") || t.starts_with("func Test")
                    || t.starts_with("it(") || t.starts_with("test(")
                {
                    body_end = j;
                    break;
                }
            }
            let body: String = lines[body_start..body_end].join("\n");
            tests.push((name, body));
        }
    }

    tests
}

fn count_patterns(text: &str, patterns: &[&str]) -> usize {
    patterns.iter().map(|p| text.matches(p).count()).sum()
}

pub fn render_test_quality(report: &TestQualityReport) -> String {
    let mut out = format!(
        "punk test-quality: {} files, {} tests, {} issues\n",
        report.files_scanned, report.tests_found, report.issues.len(),
    );

    if report.zero_assertion_count > 0 {
        out.push_str(&format!("  ERROR: {} tests with zero assertions\n", report.zero_assertion_count));
    }
    if report.tautology_count > 0 {
        out.push_str(&format!("  WARN:  {} tautological assertions\n", report.tautology_count));
    }
    if report.mock_heavy_count > 0 {
        out.push_str(&format!("  WARN:  {} mock-dominated tests\n", report.mock_heavy_count));
    }

    for issue in &report.issues {
        out.push_str(&format!("  {:?} {}::{} — {}\n",
            issue.severity, issue.file, issue.test_name, issue.message));
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
    fn detect_zero_assertions_rust() {
        let tmp = TempDir::new().unwrap();
        // No leading newline — first line is #[test]
        std::fs::write(tmp.path().join("test_auth.rs"), "#[test]\nfn test_empty() {\n    let x = 1 + 1;\n}\n\n#[test]\nfn test_good() {\n    assert_eq!(1 + 1, 2);\n}\n").unwrap();

        let report = scan_test_files(tmp.path(), &["test_auth.rs".to_string()]);
        assert_eq!(report.tests_found, 2);
        assert_eq!(report.zero_assertion_count, 1);
    }

    #[test]
    fn detect_tautology() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("test_bad.rs"), r#"
#[test]
fn test_taut() {
    assert!(true);
}
"#).unwrap();

        let report = scan_test_files(tmp.path(), &["test_bad.rs".to_string()]);
        assert_eq!(report.tautology_count, 1);
    }

    #[test]
    fn detect_mock_heavy_python() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("test_service.py"), r#"
def test_service():
    mock1 = Mock()
    mock2 = Mock()
    mock3 = MagicMock()
    result = service.run(mock1, mock2, mock3)
    assert result is not None
"#).unwrap();

        let report = scan_test_files(tmp.path(), &["test_service.py".to_string()]);
        assert_eq!(report.mock_heavy_count, 1);
    }

    #[test]
    fn clean_tests_pass() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("test_good.rs"), r#"
#[test]
fn test_addition() {
    assert_eq!(1 + 1, 2);
    assert_ne!(1, 2);
}
"#).unwrap();

        let report = scan_test_files(tmp.path(), &["test_good.rs".to_string()]);
        assert_eq!(report.issues.len(), 0);
    }

    #[test]
    fn non_test_files_skipped() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
        let report = scan_test_files(tmp.path(), &["main.rs".to_string()]);
        assert_eq!(report.files_scanned, 0);
    }

    #[test]
    fn report_roundtrip() {
        let r = TestQualityReport {
            files_scanned: 2, tests_found: 5, issues: vec![],
            zero_assertion_count: 0, tautology_count: 0, mock_heavy_count: 0,
        };
        let j = serde_json::to_string(&r).unwrap();
        let back: TestQualityReport = serde_json::from_str(&j).unwrap();
        assert_eq!(back.tests_found, 5);
    }
}
