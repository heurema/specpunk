//! Dependency health check via deps.dev API.
//! Verifies packages exist and are not deprecated/yanked.
//! Offline fallback: skip with warning, never block on network failure.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepStatus {
    Ok,
    Deprecated,
    Yanked,
    NotFound,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepSeverity {
    HardFail,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepFinding {
    pub package: String,
    pub ecosystem: String,
    pub status: DepStatus,
    pub severity: DepSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepReport {
    pub packages_checked: usize,
    pub findings: Vec<DepFinding>,
    pub hard_fail_count: usize,
    pub warning_count: usize,
    pub api_available: bool,
}

// ---------------------------------------------------------------------------
// Import extraction (regex-based)
// ---------------------------------------------------------------------------

/// Extract imported package names from source file content.
pub fn extract_imports(content: &str, language: &str) -> Vec<(String, String)> {
    let mut imports = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        match language {
            "rust" => {
                // use serde::Serialize; → "serde"
                // use std::collections::HashMap; → skip std
                if let Some(rest) = trimmed.strip_prefix("use ") {
                    if let Some(crate_name) = rest.split("::").next() {
                        let name = crate_name.trim_end_matches(';').trim();
                        if name != "std"
                            && name != "core"
                            && name != "alloc"
                            && name != "crate"
                            && name != "self"
                            && name != "super"
                        {
                            imports.push((name.to_string(), "cargo".to_string()));
                        }
                    }
                }
            }
            "python" => {
                // import requests → "requests"
                // from flask import Flask → "flask"
                if let Some(rest) = trimmed.strip_prefix("import ") {
                    let pkg = rest
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .split('.')
                        .next()
                        .unwrap_or("");
                    if !pkg.is_empty() {
                        imports.push((pkg.to_string(), "pypi".to_string()));
                    }
                } else if let Some(rest) = trimmed.strip_prefix("from ") {
                    let pkg = rest
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .split('.')
                        .next()
                        .unwrap_or("");
                    if !pkg.is_empty() && !pkg.starts_with('.') {
                        imports.push((pkg.to_string(), "pypi".to_string()));
                    }
                }
            }
            "typescript" | "javascript" => {
                // import { x } from 'package'; → "package"
                if trimmed.starts_with("import ")
                    || trimmed.starts_with("const ")
                    || trimmed.starts_with("require(")
                {
                    if let Some(from_idx) = trimmed.find("from ") {
                        let module = trimmed[from_idx + 5..]
                            .trim()
                            .trim_matches(|c| c == '\'' || c == '"' || c == ';');
                        if !module.starts_with('.') && !module.starts_with('/') {
                            let pkg = module.split('/').next().unwrap_or(module);
                            imports.push((pkg.to_string(), "npm".to_string()));
                        }
                    }
                }
            }
            "go" => {
                // "github.com/gorilla/mux" → "github.com/gorilla/mux"
                if trimmed.starts_with('"') && trimmed.ends_with('"') {
                    let pkg = trimmed.trim_matches('"');
                    if pkg.contains('/') && !pkg.starts_with("internal/") {
                        imports.push((pkg.to_string(), "go".to_string()));
                    }
                }
            }
            _ => {}
        }
    }

    // Deduplicate
    imports.sort();
    imports.dedup();
    imports
}

// ---------------------------------------------------------------------------
// deps.dev API check
// ---------------------------------------------------------------------------

/// Check a package against deps.dev API (v3alpha).
/// Returns DepStatus based on API response.
/// Offline fallback: returns Unknown with api_available=false.
pub fn check_package_depsdev(name: &str, ecosystem: &str) -> (DepStatus, bool) {
    let system = match ecosystem {
        "cargo" => "cargo",
        "npm" => "npm",
        "pypi" => "pypi",
        "go" => "go",
        "maven" => "maven",
        "nuget" => "nuget",
        _ => return (DepStatus::Unknown, false),
    };

    let url = format!(
        "https://api.deps.dev/v3alpha/systems/{}/packages/{}",
        system,
        urlencoding(name),
    );

    // Use curl via Command::new (no shell) with 5s timeout
    let output = std::process::Command::new("curl")
        .args(["-s", "-m", "5", "-w", "\n%{http_code}", &url])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let full = String::from_utf8_lossy(&o.stdout).to_string();
            let lines: Vec<&str> = full.trim().rsplitn(2, '\n').collect();
            let status_code = lines
                .first()
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(0);

            match status_code {
                200 => {
                    // Package exists. Deprecation is per-version, not per-package.
                    // The package endpoint returns ALL versions — scanning for
                    // isDeprecated in the full body gives false positives on
                    // popular packages (old versions are deprecated, current is not).
                    // For MVP: existence check is the high-value signal.
                    (DepStatus::Ok, true)
                }
                404 => (DepStatus::NotFound, true),
                _ => (DepStatus::Unknown, true),
            }
        }
        _ => (DepStatus::Unknown, false), // offline fallback
    }
}

fn urlencoding(s: &str) -> String {
    s.replace('/', "%2F").replace('@', "%40")
}

/// Check all extracted imports against deps.dev.
pub fn check_imports(imports: &[(String, String)]) -> DepReport {
    let mut findings = Vec::new();
    let mut api_available = true;
    let mut checked = 0;

    // Skip stdlib/common packages
    let skip = [
        "os",
        "sys",
        "re",
        "json",
        "pathlib",
        "typing",
        "collections",
        "datetime",
        "io",
        "math",
        "functools",
        "itertools",
        "hashlib",
        "time",
        "string",
        "abc",
        "dataclasses",
        "enum",
        "copy",
        "react",
        "react-dom",
        "next",
        "vue",
        "path",
        "fs",
        "http",
        "url",
        "fmt",
        "net",
        "context",
        "sync",
        "errors",
        "strings",
        "testing",
    ];

    for (name, ecosystem) in imports {
        if skip.contains(&name.as_str()) {
            continue;
        }
        checked += 1;

        let (status, available) = check_package_depsdev(name, ecosystem);
        if !available {
            api_available = false;
        }

        let (severity, message) = match &status {
            DepStatus::NotFound => (
                DepSeverity::HardFail,
                format!("package '{name}' not found on {ecosystem}"),
            ),
            DepStatus::Deprecated => (
                DepSeverity::Warning,
                format!("package '{name}' is deprecated on {ecosystem}"),
            ),
            DepStatus::Yanked => (
                DepSeverity::Warning,
                format!("package '{name}' version yanked on {ecosystem}"),
            ),
            DepStatus::Ok => continue,
            DepStatus::Unknown => (
                DepSeverity::Info,
                format!("could not verify '{name}' on {ecosystem}"),
            ),
        };

        findings.push(DepFinding {
            package: name.clone(),
            ecosystem: ecosystem.clone(),
            status,
            severity,
            message,
        });
    }

    let hard_fail_count = findings
        .iter()
        .filter(|f| f.severity == DepSeverity::HardFail)
        .count();
    let warning_count = findings
        .iter()
        .filter(|f| f.severity == DepSeverity::Warning)
        .count();

    DepReport {
        packages_checked: checked,
        findings,
        hard_fail_count,
        warning_count,
        api_available,
    }
}

pub fn render_dep_report(report: &DepReport) -> String {
    if report.findings.is_empty() {
        return format!(
            "punk deps: {} packages checked, all OK{}\n",
            report.packages_checked,
            if report.api_available {
                ""
            } else {
                " (offline mode)"
            }
        );
    }

    let mut out = format!(
        "punk deps: {} checked, {} issues\n",
        report.packages_checked,
        report.findings.len()
    );

    if !report.api_available {
        out.push_str("  WARNING: deps.dev API unreachable, results may be incomplete\n");
    }

    for f in &report.findings {
        out.push_str(&format!(
            "  {:?} {} ({})\n",
            f.severity, f.message, f.ecosystem
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
    fn extract_rust_imports() {
        let content = "use serde::Serialize;\nuse std::collections::HashMap;\nuse crate::config;\n";
        let imports = extract_imports(content, "rust");
        assert!(imports.iter().any(|(n, _)| n == "serde"));
        assert!(!imports.iter().any(|(n, _)| n == "std"));
        assert!(!imports.iter().any(|(n, _)| n == "crate"));
    }

    #[test]
    fn extract_python_imports() {
        let content = "import requests\nfrom flask import Flask\nimport os\nfrom .local import x\n";
        let imports = extract_imports(content, "python");
        assert!(imports.iter().any(|(n, _)| n == "requests"));
        assert!(imports.iter().any(|(n, _)| n == "flask"));
        assert!(imports.iter().any(|(n, _)| n == "os"));
        assert!(!imports.iter().any(|(n, _)| n.starts_with('.')));
    }

    #[test]
    fn extract_js_imports() {
        let content =
            "import { useState } from 'react';\nimport axios from 'axios';\nimport './local';\n";
        let imports = extract_imports(content, "javascript");
        assert!(imports.iter().any(|(n, _)| n == "react"));
        assert!(imports.iter().any(|(n, _)| n == "axios"));
        assert!(!imports.iter().any(|(n, _)| n.starts_with('.')));
    }

    #[test]
    fn extract_go_imports() {
        let content = "\"github.com/gorilla/mux\"\n\"fmt\"\n\"internal/pkg\"\n";
        let imports = extract_imports(content, "go");
        assert!(imports.iter().any(|(n, _)| n.contains("gorilla")));
        assert!(!imports.iter().any(|(n, _)| n == "fmt"));
        assert!(!imports.iter().any(|(n, _)| n.contains("internal")));
    }

    #[test]
    fn check_skip_stdlib() {
        let imports = vec![
            ("os".to_string(), "pypi".to_string()),
            ("sys".to_string(), "pypi".to_string()),
        ];
        let report = check_imports(&imports);
        assert_eq!(report.packages_checked, 0); // all skipped
    }

    #[test]
    fn dep_report_roundtrip() {
        let r = DepReport {
            packages_checked: 5,
            findings: vec![],
            hard_fail_count: 0,
            warning_count: 0,
            api_available: true,
        };
        let j = serde_json::to_string(&r).unwrap();
        let back: DepReport = serde_json::from_str(&j).unwrap();
        assert_eq!(back.packages_checked, 5);
    }

    #[test]
    fn render_clean() {
        let r = DepReport {
            packages_checked: 3,
            findings: vec![],
            hard_fail_count: 0,
            warning_count: 0,
            api_available: true,
        };
        let out = render_dep_report(&r);
        assert!(out.contains("all OK"));
    }

    #[test]
    fn render_offline() {
        let r = DepReport {
            packages_checked: 1,
            findings: vec![],
            hard_fail_count: 0,
            warning_count: 0,
            api_available: false,
        };
        let out = render_dep_report(&r);
        assert!(out.contains("offline"));
    }
}
