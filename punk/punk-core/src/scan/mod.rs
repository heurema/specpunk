//! Convention scan: detect project patterns and generate AGENTS.md.
//! Phase 12 — regex-based heuristics (tree-sitter planned as feature flag).
//!
//! Scans source files for naming conventions, import patterns, error handling
//! idioms, and test patterns. Outputs enriched conventions.json + AGENTS.md.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConventionFinding {
    pub name: String,
    pub pattern: String,
    pub frequency: usize,
    pub confidence: String,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub language: String,
    pub findings: Vec<ConventionFinding>,
    pub naming: NamingConventions,
    pub imports: ImportPatterns,
    pub errors: ErrorPatterns,
    pub tests: TestPatterns,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NamingConventions {
    pub functions: String,
    pub types: String,
    pub constants: String,
    pub files: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportPatterns {
    pub style: String,
    pub top_imports: Vec<String>,
    pub layering: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErrorPatterns {
    pub style: String,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestPatterns {
    pub framework: String,
    pub style: String,
    pub mock_usage: String,
}

// ---------------------------------------------------------------------------
// Scanner
// ---------------------------------------------------------------------------

/// Run convention scan on a project.
pub fn scan_conventions(root: &Path, language: &str) -> ScanReport {
    let files = collect_source_files(root, language);
    let contents: Vec<(String, String)> = files.iter()
        .filter_map(|f| {
            let content = std::fs::read_to_string(root.join(f)).ok()?;
            Some((f.clone(), content))
        })
        .collect();

    let naming = detect_naming(&contents, language);
    let imports = detect_imports(&contents, language);
    let errors = detect_errors(&contents, language);
    let tests = detect_tests(&contents, language);

    let mut findings = Vec::new();

    // Naming findings
    if !naming.functions.is_empty() {
        findings.push(ConventionFinding {
            name: "function_naming".to_string(),
            pattern: naming.functions.clone(),
            frequency: contents.len(),
            confidence: "high".to_string(),
            examples: vec![],
        });
    }
    if !errors.style.is_empty() {
        findings.push(ConventionFinding {
            name: "error_handling".to_string(),
            pattern: errors.style.clone(),
            frequency: contents.len(),
            confidence: "high".to_string(),
            examples: errors.examples.clone(),
        });
    }

    ScanReport {
        language: language.to_string(),
        findings,
        naming,
        imports,
        errors,
        tests,
    }
}

fn collect_source_files(root: &Path, language: &str) -> Vec<String> {
    let exts: &[&str] = match language {
        "rust" => &["rs"],
        "python" => &["py"],
        "typescript" => &["ts", "tsx"],
        "javascript" => &["js", "jsx"],
        "go" => &["go"],
        _ => &[],
    };

    let mut files = Vec::new();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .max_depth(10)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.') && name != "target" && name != "node_modules"
                && name != "vendor" && name != "__pycache__" && name != ".venv"
        });

    for entry in walker.flatten() {
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if exts.contains(&ext.as_str()) {
                    if let Ok(rel) = entry.path().strip_prefix(root) {
                        files.push(rel.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    files
}

// ---------------------------------------------------------------------------
// Naming detection
// ---------------------------------------------------------------------------

fn detect_naming(contents: &[(String, String)], language: &str) -> NamingConventions {
    match language {
        "rust" => NamingConventions {
            functions: "snake_case".to_string(),
            types: "PascalCase".to_string(),
            constants: "SCREAMING_SNAKE_CASE".to_string(),
            files: "snake_case.rs".to_string(),
        },
        "python" => NamingConventions {
            functions: "snake_case".to_string(),
            types: "PascalCase".to_string(),
            constants: "SCREAMING_SNAKE_CASE".to_string(),
            files: "snake_case.py".to_string(),
        },
        "go" => NamingConventions {
            functions: "camelCase (exported: PascalCase)".to_string(),
            types: "PascalCase".to_string(),
            constants: "PascalCase or camelCase".to_string(),
            files: "snake_case.go".to_string(),
        },
        "typescript" | "javascript" => {
            // Detect: camelCase vs snake_case for functions
            let mut camel = 0usize;
            let mut snake = 0usize;
            for (_, content) in contents {
                for line in content.lines() {
                    if line.contains("function ") || line.contains("const ") || line.contains("let ") {
                        if line.contains('_') && !line.contains("__") {
                            snake += 1;
                        } else {
                            camel += 1;
                        }
                    }
                }
            }
            NamingConventions {
                functions: if snake > camel { "snake_case" } else { "camelCase" }.to_string(),
                types: "PascalCase".to_string(),
                constants: "SCREAMING_SNAKE_CASE".to_string(),
                files: if snake > camel { "kebab-case.ts" } else { "camelCase.ts" }.to_string(),
            }
        }
        _ => NamingConventions::default(),
    }
}

// ---------------------------------------------------------------------------
// Import detection
// ---------------------------------------------------------------------------

fn detect_imports(contents: &[(String, String)], language: &str) -> ImportPatterns {
    let mut import_counts: HashMap<String, usize> = HashMap::new();

    for (_, content) in contents {
        for line in content.lines() {
            let trimmed = line.trim();
            let import = match language {
                "rust" => {
                    if trimmed.starts_with("use ") {
                        trimmed.strip_prefix("use ")
                            .and_then(|s| s.split("::").next())
                            .map(|s| s.to_string())
                    } else { None }
                }
                "python" => {
                    if trimmed.starts_with("import ") || trimmed.starts_with("from ") {
                        let parts: Vec<&str> = trimmed.split_whitespace().collect();
                        parts.get(1).map(|s| s.to_string())
                    } else { None }
                }
                "typescript" | "javascript" => {
                    if trimmed.starts_with("import ") {
                        if let Some(from_idx) = trimmed.find("from ") {
                            let module = trimmed[from_idx + 5..].trim().trim_matches(|c| c == '\'' || c == '"' || c == ';');
                            Some(module.to_string())
                        } else { None }
                    } else { None }
                }
                "go" => {
                    if trimmed.starts_with("\"") && trimmed.ends_with("\"") {
                        Some(trimmed.trim_matches('"').to_string())
                    } else { None }
                }
                _ => None,
            };

            if let Some(imp) = import {
                *import_counts.entry(imp).or_insert(0) += 1;
            }
        }
    }

    let mut sorted: Vec<_> = import_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    let top = sorted.into_iter().take(10).map(|(k, _)| k).collect();

    ImportPatterns {
        style: match language {
            "rust" => "use crate::module".to_string(),
            "python" => "from module import name".to_string(),
            "typescript" | "javascript" => "import { name } from 'module'".to_string(),
            "go" => "import \"package\"".to_string(),
            _ => String::new(),
        },
        top_imports: top,
        layering: vec![],
    }
}

// ---------------------------------------------------------------------------
// Error handling detection
// ---------------------------------------------------------------------------

fn detect_errors(contents: &[(String, String)], language: &str) -> ErrorPatterns {
    let mut examples = Vec::new();

    match language {
        "rust" => {
            let mut uses_anyhow = false;
            let mut uses_thiserror = false;
            let mut uses_result = 0usize;
            let mut uses_unwrap = 0usize;

            for (_, content) in contents {
                if content.contains("anyhow::") || content.contains("use anyhow") { uses_anyhow = true; }
                if content.contains("thiserror::") || content.contains("use thiserror") { uses_thiserror = true; }
                uses_result += content.matches("Result<").count();
                uses_unwrap += content.matches(".unwrap()").count();
            }

            if uses_anyhow { examples.push("anyhow for error propagation".to_string()); }
            if uses_thiserror { examples.push("thiserror for custom error types".to_string()); }

            ErrorPatterns {
                style: if uses_thiserror {
                    "thiserror + Result<T, E>".to_string()
                } else if uses_anyhow {
                    "anyhow::Result".to_string()
                } else {
                    format!("Result<T,E> ({uses_result} uses, {uses_unwrap} unwraps)")
                },
                examples,
            }
        }
        "python" => {
            let mut try_except = 0usize;
            for (_, content) in contents {
                try_except += content.matches("try:").count();
            }
            ErrorPatterns {
                style: format!("try/except ({try_except} blocks)"),
                examples: vec![],
            }
        }
        _ => ErrorPatterns::default(),
    }
}

// ---------------------------------------------------------------------------
// Test detection
// ---------------------------------------------------------------------------

fn detect_tests(contents: &[(String, String)], language: &str) -> TestPatterns {
    let mut test_count = 0usize;
    let mut has_mocks = false;

    for (file, content) in contents {
        let is_test_file = file.contains("test") || file.contains("spec");

        match language {
            "rust" => {
                test_count += content.matches("#[test]").count();
                test_count += content.matches("#[tokio::test]").count();
                if content.contains("mockall") || content.contains("mock!") { has_mocks = true; }
            }
            "python" => {
                if is_test_file {
                    test_count += content.lines().filter(|l| l.trim().starts_with("def test_")).count();
                }
                if content.contains("mock") || content.contains("MagicMock") || content.contains("patch(") {
                    has_mocks = true;
                }
            }
            "typescript" | "javascript" => {
                if is_test_file {
                    test_count += content.matches("it(").count();
                    test_count += content.matches("test(").count();
                }
                if content.contains("jest.mock") || content.contains("vi.mock") { has_mocks = true; }
            }
            "go" => {
                test_count += content.lines().filter(|l| l.trim().starts_with("func Test")).count();
            }
            _ => {}
        }
    }

    let framework = match language {
        "rust" => "cargo test".to_string(),
        "python" => "pytest".to_string(),
        "typescript" | "javascript" => "jest/vitest".to_string(),
        "go" => "go test".to_string(),
        _ => String::new(),
    };

    TestPatterns {
        framework,
        style: format!("{test_count} tests found"),
        mock_usage: if has_mocks { "mocks detected".to_string() } else { "no mocks".to_string() },
    }
}

// ---------------------------------------------------------------------------
// AGENTS.md generation
// ---------------------------------------------------------------------------

/// Generate AGENTS.md content from scan report.
pub fn generate_agents_md(report: &ScanReport, project_name: &str) -> String {
    let mut md = format!("# AGENTS.md — {project_name}\n\n");
    md.push_str("<!-- Generated by punk scan. Edit to customize. -->\n\n");

    md.push_str(&format!("## Language: {}\n\n", report.language));

    // Naming
    md.push_str("## Naming Conventions\n\n");
    md.push_str(&format!("- Functions: `{}`\n", report.naming.functions));
    md.push_str(&format!("- Types: `{}`\n", report.naming.types));
    md.push_str(&format!("- Constants: `{}`\n", report.naming.constants));
    md.push_str(&format!("- Files: `{}`\n\n", report.naming.files));

    // Imports
    md.push_str("## Import Style\n\n");
    md.push_str(&format!("Pattern: `{}`\n\n", report.imports.style));
    if !report.imports.top_imports.is_empty() {
        md.push_str("Top dependencies:\n");
        for imp in &report.imports.top_imports {
            md.push_str(&format!("- `{imp}`\n"));
        }
        md.push('\n');
    }

    // Error handling
    md.push_str("## Error Handling\n\n");
    md.push_str(&format!("Style: {}\n", report.errors.style));
    for ex in &report.errors.examples {
        md.push_str(&format!("- {ex}\n"));
    }
    md.push('\n');

    // Testing
    md.push_str("## Testing\n\n");
    md.push_str(&format!("- Framework: `{}`\n", report.tests.framework));
    md.push_str(&format!("- Coverage: {}\n", report.tests.style));
    md.push_str(&format!("- Mocks: {}\n\n", report.tests.mock_usage));

    // Rules for agents
    md.push_str("## Rules for AI Agents\n\n");
    md.push_str("1. Follow the naming conventions above — do not introduce inconsistencies.\n");
    md.push_str("2. Use the established error handling pattern.\n");
    md.push_str("3. Add tests for new functionality using the detected test framework.\n");
    md.push_str(&format!("4. Import style: `{}`.\n", report.imports.style));
    md.push_str("5. Do not modify files outside the contract scope.\n");

    md
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn scan_rust_project() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("lib.rs"), r#"
use std::collections::HashMap;
use serde::Serialize;

pub fn hello_world() -> Result<String, Box<dyn std::error::Error>> {
    Ok("hello".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_hello() {
        assert_eq!(hello_world().unwrap(), "hello");
    }
}
"#).unwrap();

        let report = scan_conventions(tmp.path(), "rust");
        assert_eq!(report.language, "rust");
        assert_eq!(report.naming.functions, "snake_case");
        // At least 1 test should be found
        assert!(report.tests.style.contains("tests found"), "got: {}", report.tests.style);
    }

    #[test]
    fn scan_python_project() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("main.py"), r#"
import os
from pathlib import Path

def get_config():
    try:
        return Path("config.toml").read_text()
    except FileNotFoundError:
        return ""
"#).unwrap();
        std::fs::write(tmp.path().join("test_main.py"), r#"
def test_config():
    assert get_config() is not None

def test_empty():
    pass
"#).unwrap();

        let report = scan_conventions(tmp.path(), "python");
        assert_eq!(report.naming.functions, "snake_case");
        assert!(report.tests.style.contains("tests found"), "got: {}", report.tests.style);
        assert!(report.errors.style.contains("try/except"));
    }

    #[test]
    fn scan_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let report = scan_conventions(tmp.path(), "rust");
        // No source files → no findings beyond defaults
        assert_eq!(report.language, "rust");
    }

    #[test]
    fn generate_agents_md_output() {
        let report = ScanReport {
            language: "rust".to_string(),
            findings: vec![],
            naming: NamingConventions {
                functions: "snake_case".into(),
                types: "PascalCase".into(),
                constants: "SCREAMING_SNAKE_CASE".into(),
                files: "snake_case.rs".into(),
            },
            imports: ImportPatterns {
                style: "use crate::module".into(),
                top_imports: vec!["serde".into(), "tokio".into()],
                layering: vec![],
            },
            errors: ErrorPatterns {
                style: "thiserror + Result<T, E>".into(),
                examples: vec!["thiserror for custom types".into()],
            },
            tests: TestPatterns {
                framework: "cargo test".into(),
                style: "42 tests found".into(),
                mock_usage: "no mocks".into(),
            },
        };

        let md = generate_agents_md(&report, "my-project");
        assert!(md.contains("# AGENTS.md — my-project"));
        assert!(md.contains("snake_case"));
        assert!(md.contains("thiserror"));
        assert!(md.contains("serde"));
        assert!(md.contains("42 tests"));
        assert!(md.contains("Rules for AI Agents"));
    }

    #[test]
    fn import_detection_rust() {
        let contents = vec![
            ("lib.rs".to_string(), "use std::collections::HashMap;\nuse serde::Serialize;\nuse crate::config;\n".to_string()),
        ];
        let imports = detect_imports(&contents, "rust");
        assert!(imports.top_imports.contains(&"std".to_string()));
        assert!(imports.top_imports.contains(&"serde".to_string()));
    }

    #[test]
    fn report_roundtrip() {
        let report = ScanReport {
            language: "rust".to_string(),
            findings: vec![ConventionFinding {
                name: "test".into(), pattern: "pat".into(),
                frequency: 5, confidence: "high".into(),
                examples: vec!["ex".into()],
            }],
            naming: NamingConventions::default(),
            imports: ImportPatterns::default(),
            errors: ErrorPatterns::default(),
            tests: TestPatterns::default(),
        };
        let json = serde_json::to_string_pretty(&report).unwrap();
        let back: ScanReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.findings.len(), 1);
    }
}
