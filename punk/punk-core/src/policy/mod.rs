//! Policy scanner: deterministic security pattern detection on diffs.
//! Zero LLM, string matching only, <50ms. Scans addition lines only.
//! This module DETECTS dangerous patterns in source code diffs.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PolicySeverity { Critical, Major }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyFinding {
    pub id: String,
    pub severity: PolicySeverity,
    pub pattern_name: String,
    pub file: String,
    pub line_number: usize,
    pub line_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyReport {
    pub findings: Vec<PolicyFinding>,
    pub critical_count: usize,
    pub major_count: usize,
}

struct Pattern { name: &'static str, severity: PolicySeverity, needles: &'static [&'static str], case_insensitive: bool }

const PATTERNS: &[Pattern] = &[
    Pattern { name: "hardcoded_secret", severity: PolicySeverity::Critical,
        needles: &["password =", "secret =", "api_key ="], case_insensitive: true },
    Pattern { name: "dangerous_function", severity: PolicySeverity::Critical,
        needles: &["eval(", "exec("], case_insensitive: false },
    Pattern { name: "shell_injection_risk", severity: PolicySeverity::Critical,
        needles: &["shell=True", "shell = True"], case_insensitive: false },
    Pattern { name: "sql_format_string", severity: PolicySeverity::Critical,
        needles: &[".format(", "f\"SELECT", "f'SELECT"], case_insensitive: false },
    Pattern { name: "unsafe_block", severity: PolicySeverity::Critical,
        needles: &["unsafe {"], case_insensitive: false },
    Pattern { name: "private_key_literal", severity: PolicySeverity::Critical,
        needles: &["PRIVATE KEY-----"], case_insensitive: false },
    Pattern { name: "todo_fixme", severity: PolicySeverity::Major,
        needles: &["TODO", "FIXME", "HACK", "XXX"], case_insensitive: false },
    Pattern { name: "broad_exception", severity: PolicySeverity::Major,
        needles: &["except:"], case_insensitive: false },
    Pattern { name: "debug_output", severity: PolicySeverity::Major,
        needles: &["console.log", "dbg!"], case_insensitive: false },
    Pattern { name: "disabled_auth", severity: PolicySeverity::Major,
        needles: &["auth = false", "authentication = false"], case_insensitive: true },
    Pattern { name: "cors_wildcard", severity: PolicySeverity::Major,
        needles: &["Allow-Origin: *", "allow_origin(\"*\")"], case_insensitive: true },
    Pattern { name: "ssl_verify_disabled", severity: PolicySeverity::Major,
        needles: &["verify=False", "verify = False", "--no-check-certificate"], case_insensitive: true },
];

pub fn scan_diff(diff: &str) -> PolicyReport {
    let mut findings = Vec::new();
    let mut current_file = String::new();
    let mut line_number = 0usize;

    for line in diff.lines() {
        if let Some(file) = line.strip_prefix("+++ b/") { current_file = file.to_string(); continue; }
        if line.starts_with("@@ ") {
            if let Some(plus) = line.split('+').nth(1) {
                if let Some(num) = plus.split(',').next() {
                    line_number = num.parse().unwrap_or(0);
                }
            }
            continue;
        }
        if line.starts_with('+') && !line.starts_with("+++") {
            let content = &line[1..];
            for pattern in PATTERNS {
                if matches_pattern(content, pattern) {
                    findings.push(PolicyFinding {
                        id: format!("POL-{:03}", findings.len() + 1),
                        severity: pattern.severity.clone(),
                        pattern_name: pattern.name.to_string(),
                        file: current_file.clone(),
                        line_number,
                        line_content: content.trim().to_string(),
                    });
                }
            }
            line_number += 1;
        } else if !line.starts_with('-') { line_number += 1; }
    }

    let critical_count = findings.iter().filter(|f| f.severity == PolicySeverity::Critical).count();
    let major_count = findings.iter().filter(|f| f.severity == PolicySeverity::Major).count();
    PolicyReport { findings, critical_count, major_count }
}

fn matches_pattern(text: &str, pattern: &Pattern) -> bool {
    for needle in pattern.needles {
        if pattern.case_insensitive {
            if text.to_lowercase().contains(&needle.to_lowercase()) { return true; }
        } else if text.contains(needle) { return true; }
    }
    false
}

pub fn render_policy(report: &PolicyReport) -> String {
    if report.findings.is_empty() { return "punk policy: clean (0 findings)\n".to_string(); }
    let mut out = format!("punk policy: {} findings ({} critical, {} major)\n\n",
        report.findings.len(), report.critical_count, report.major_count);
    for f in &report.findings {
        out.push_str(&format!("  {:?} [{}] {}:{} — {}\n    {}\n",
            f.severity, f.id, f.file, f.line_number, f.pattern_name, f.line_content));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_diff(file: &str, additions: &[&str]) -> String {
        let mut d = format!("--- a/{file}\n+++ b/{file}\n@@ -1,1 +1,{} @@\n", additions.len());
        for l in additions { d.push_str(&format!("+{l}\n")); }
        d
    }

    #[test]
    fn detect_secret() {
        let r = scan_diff(&make_diff("c.py", &["PASSWORD = \"supersecret123\""]));
        assert!(r.critical_count > 0);
    }

    #[test]
    fn detect_dangerous_fn() {
        let r = scan_diff(&make_diff("h.py", &["result = eval(user_input)"]));
        assert!(r.findings.iter().any(|f| f.pattern_name == "dangerous_function"));
    }

    #[test]
    fn detect_shell() {
        let r = scan_diff(&make_diff("r.py", &["subprocess.run(cmd, shell=True)"]));
        assert!(r.findings.iter().any(|f| f.pattern_name == "shell_injection_risk"));
    }

    #[test]
    fn detect_unsafe() {
        assert!(scan_diff(&make_diff("l.rs", &["unsafe {"])).critical_count > 0);
    }

    #[test]
    fn detect_todo() {
        assert!(scan_diff(&make_diff("m.rs", &["// TODO: fix"])).major_count > 0);
    }

    #[test]
    fn clean() {
        assert_eq!(scan_diff(&make_diff("l.rs", &["fn ok() {}"])).findings.len(), 0);
    }

    #[test]
    fn only_additions() {
        let d = "--- a/c.py\n+++ b/c.py\n@@ -1,2 +1,1 @@\n-PASSWORD = \"old\"\n+# clean\n";
        assert_eq!(scan_diff(d).critical_count, 0);
    }
}
