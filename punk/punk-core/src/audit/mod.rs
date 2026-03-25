//! Multi-model audit: independent code review by Claude, Codex, and Gemini.
//! Synthesizer applies deterministic verdict rules — no LLM in the decision.
//!
//! Adversarial isolation: external models (Codex/Gemini) receive ONLY
//! goal + diff. Never the full contract, mechanic results, or holdout data.

use std::path::Path;
use std::process::Command;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::plan::sha256_hex;
use crate::risk::AssuranceTier;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Severity {
    Info,
    Minor,
    Major,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub provider: String,
    pub severity: Severity,
    pub category: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub message: String,
    pub fingerprint: String,
}

impl Finding {
    /// Compute stable fingerprint: sha256(category + file + normalized_message)[:8]
    pub fn compute_fingerprint(category: &str, file: Option<&str>, message: &str) -> String {
        let normalized = message.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
        let input = format!("{}{}{}", category, file.unwrap_or(""), normalized);
        sha256_hex(input.as_bytes())[..8].to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewVerdict {
    Approve,
    Conditional,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewResult {
    pub provider: String,
    pub available: bool,
    pub verdict: Option<ReviewVerdict>,
    pub findings: Vec<Finding>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

// ---------------------------------------------------------------------------
// Audit decision (deterministic)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuditDecision {
    AutoOk,
    AutoBlock,
    HumanReview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReleaseVerdict {
    Promote,
    Hold,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceScores {
    pub execution_health: f64,
    pub baseline_stability: f64,
    pub behavioral_evidence: f64,
    pub review_alignment: f64,
    pub overall: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    pub schema_version: String,
    pub timestamp: String,
    pub contract_id: String,
    pub tier: AssuranceTier,
    pub reviews: Vec<ReviewResult>,
    pub all_findings: Vec<Finding>,
    pub decision: AuditDecision,
    pub release_verdict: ReleaseVerdict,
    pub confidence: ConfidenceScores,
    pub evidence_coverage: f64,
    pub duration_ms: u64,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum AuditError {
    NoContract(String),
    NoDiff(String),
    Io(std::io::Error),
    Parse(String),
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditError::NoContract(m) => write!(f, "no contract: {m}"),
            AuditError::NoDiff(m) => write!(f, "no diff: {m}"),
            AuditError::Io(e) => write!(f, "I/O error: {e}"),
            AuditError::Parse(m) => write!(f, "parse error: {m}"),
        }
    }
}

impl std::error::Error for AuditError {}

impl From<std::io::Error> for AuditError {
    fn from(e: std::io::Error) -> Self {
        AuditError::Io(e)
    }
}

// ---------------------------------------------------------------------------
// Provider dispatch
// ---------------------------------------------------------------------------

/// Check if a CLI binary is available.
fn cli_available(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run Codex CLI review. Receives ONLY goal + diff (adversarial isolation).
fn run_codex_review(goal: &str, diff: &str) -> ReviewResult {
    let start = std::time::Instant::now();

    if !cli_available("codex") {
        return ReviewResult {
            provider: "codex".to_string(),
            available: false,
            verdict: None,
            findings: vec![],
            error: Some("codex CLI not found".to_string()),
            duration_ms: 0,
        };
    }

    let prompt = format!(
        "You are a security-focused code reviewer. Review this diff.\n\
         Report ONLY: bugs, security issues, performance problems.\n\
         Format each finding as: SEVERITY: [file:line] description\n\
         Where SEVERITY is one of: CRITICAL, MAJOR, MINOR, INFO\n\
         End with: VERDICT: APPROVE or REJECT or CONDITIONAL\n\n\
         Goal: {goal}\n\nDiff:\n{diff}"
    );

    let out_file = std::env::temp_dir().join("punk-codex-review.txt");
    let result = Command::new("codex")
        .args(["exec", "--ephemeral", "-p", "fast", "--output-last-message"])
        .arg(&out_file)
        .arg(&prompt)
        .output();

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(output) if output.status.success() => {
            let response = std::fs::read_to_string(&out_file).unwrap_or_default();
            let _ = std::fs::remove_file(&out_file);
            parse_review_response("codex", &response, duration_ms)
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            ReviewResult {
                provider: "codex".to_string(),
                available: true,
                verdict: None,
                findings: vec![],
                error: Some(format!("codex error: {stderr}")),
                duration_ms,
            }
        }
        Err(e) => ReviewResult {
            provider: "codex".to_string(),
            available: true,
            verdict: None,
            findings: vec![],
            error: Some(format!("codex exec failed: {e}")),
            duration_ms,
        },
    }
}

/// Run Gemini CLI review. Receives ONLY goal + diff (adversarial isolation).
fn run_gemini_review(goal: &str, diff: &str) -> ReviewResult {
    let start = std::time::Instant::now();

    if !cli_available("gemini") {
        return ReviewResult {
            provider: "gemini".to_string(),
            available: false,
            verdict: None,
            findings: vec![],
            error: Some("gemini CLI not found".to_string()),
            duration_ms: 0,
        };
    }

    let prompt = format!(
        "You are a performance-focused code reviewer. Review this diff.\n\
         Report ONLY: bugs, security issues, performance problems.\n\
         Format each finding as: SEVERITY: [file:line] description\n\
         Where SEVERITY is one of: CRITICAL, MAJOR, MINOR, INFO\n\
         End with: VERDICT: APPROVE or REJECT or CONDITIONAL\n\n\
         Goal: {goal}\n\nDiff:\n{diff}"
    );

    let err_file = std::env::temp_dir().join("punk-gemini-stderr.txt");
    let result = Command::new("gemini")
        .args(["-p", &prompt, "-o", "text"])
        .stderr(std::fs::File::create(&err_file).unwrap_or_else(|_| {
            std::fs::File::create("/dev/null").unwrap()
        }))
        .output();

    let duration_ms = start.elapsed().as_millis() as u64;
    let _ = std::fs::remove_file(&err_file);

    match result {
        Ok(output) if output.status.success() => {
            let response = String::from_utf8_lossy(&output.stdout).to_string();
            parse_review_response("gemini", &response, duration_ms)
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            ReviewResult {
                provider: "gemini".to_string(),
                available: true,
                verdict: None,
                findings: vec![],
                error: Some(if stderr.to_lowercase().contains("auth") {
                    "gemini auth error. Run: gemini login".to_string()
                } else {
                    format!("gemini error: {stderr}")
                }),
                duration_ms,
            }
        }
        Err(e) => ReviewResult {
            provider: "gemini".to_string(),
            available: true,
            verdict: None,
            findings: vec![],
            error: Some(format!("gemini exec failed: {e}")),
            duration_ms,
        },
    }
}

/// Parse a review response into findings + verdict.
fn parse_review_response(provider: &str, response: &str, duration_ms: u64) -> ReviewResult {
    let mut findings = Vec::new();
    let mut verdict = None;

    for line in response.lines() {
        let trimmed = line.trim();

        // Parse verdict
        if let Some(rest) = trimmed.strip_prefix("VERDICT:") {
            let v = rest.trim().to_uppercase();
            verdict = Some(match v.as_str() {
                "APPROVE" => ReviewVerdict::Approve,
                "REJECT" => ReviewVerdict::Reject,
                _ => ReviewVerdict::Conditional,
            });
            continue;
        }

        // Parse findings: "SEVERITY: [file:line] description" or "SEVERITY: description"
        for sev in &["CRITICAL:", "MAJOR:", "MINOR:", "INFO:"] {
            if let Some(rest) = trimmed.strip_prefix(sev) {
                let rest = rest.trim();
                let severity = match &sev[..sev.len() - 1] {
                    "CRITICAL" => Severity::Critical,
                    "MAJOR" => Severity::Major,
                    "MINOR" => Severity::Minor,
                    _ => Severity::Info,
                };

                let (file, line_num, message) = if rest.starts_with('[') {
                    if let Some(end) = rest.find(']') {
                        let loc = &rest[1..end];
                        let msg = rest[end + 1..].trim().to_string();
                        let parts: Vec<&str> = loc.splitn(2, ':').collect();
                        let file = Some(parts[0].to_string());
                        let line = parts.get(1).and_then(|s| s.parse().ok());
                        (file, line, msg)
                    } else {
                        (None, None, rest.to_string())
                    }
                } else {
                    (None, None, rest.to_string())
                };

                let fingerprint = Finding::compute_fingerprint(
                    sev, file.as_deref(), &message,
                );

                findings.push(Finding {
                    provider: provider.to_string(),
                    severity,
                    category: sev[..sev.len() - 1].to_string(),
                    file,
                    line: line_num,
                    message,
                    fingerprint,
                });
                break;
            }
        }
    }

    ReviewResult {
        provider: provider.to_string(),
        available: true,
        verdict,
        findings,
        error: None,
        duration_ms,
    }
}

// ---------------------------------------------------------------------------
// Synthesizer (deterministic)
// ---------------------------------------------------------------------------

/// Synthesize audit decision from review results.
/// NO LLM in this function — pure deterministic rules.
pub fn synthesize(
    reviews: &[ReviewResult],
    mechanic_regressions: usize,
    holdout_pass_rate: f64,
    ac_verified: usize,
    ac_total: usize,
    files_reviewed: usize,
    files_total: usize,
) -> (AuditDecision, ReleaseVerdict, ConfidenceScores, f64) {
    // AUTO_BLOCK conditions
    let has_regressions = mechanic_regressions > 0;
    let has_reject = reviews.iter().any(|r| r.verdict == Some(ReviewVerdict::Reject));
    let has_critical = reviews.iter().flat_map(|r| &r.findings)
        .any(|f| f.severity == Severity::Critical);

    if has_regressions || has_reject || has_critical {
        let confidence = compute_confidence(0.0, mechanic_regressions, holdout_pass_rate, reviews);
        let coverage = compute_evidence_coverage(ac_verified, ac_total, files_reviewed, files_total);
        return (AuditDecision::AutoBlock, ReleaseVerdict::Reject, confidence, coverage);
    }

    // AUTO_OK conditions
    let all_approve = reviews.iter()
        .filter(|r| r.available && r.error.is_none())
        .all(|r| r.verdict == Some(ReviewVerdict::Approve));
    let no_major = !reviews.iter().flat_map(|r| &r.findings)
        .any(|f| matches!(f.severity, Severity::Major | Severity::Critical));
    let valid_review_count = reviews.iter()
        .filter(|r| r.available && r.error.is_none() && r.verdict.is_some())
        .count();

    if all_approve && no_major && valid_review_count > 0 {
        let health = if ac_total > 0 {
            (ac_verified as f64 / ac_total as f64) * 100.0
        } else {
            75.0
        };
        let confidence = compute_confidence(health, mechanic_regressions, holdout_pass_rate, reviews);
        let coverage = compute_evidence_coverage(ac_verified, ac_total, files_reviewed, files_total);

        let verdict = if coverage >= 0.7 {
            ReleaseVerdict::Promote
        } else {
            ReleaseVerdict::Hold
        };
        return (AuditDecision::AutoOk, verdict, confidence, coverage);
    }

    // HUMAN_REVIEW: everything else
    let confidence = compute_confidence(50.0, mechanic_regressions, holdout_pass_rate, reviews);
    let coverage = compute_evidence_coverage(ac_verified, ac_total, files_reviewed, files_total);
    (AuditDecision::HumanReview, ReleaseVerdict::Hold, confidence, coverage)
}

fn compute_confidence(
    exec_health: f64,
    regressions: usize,
    holdout_rate: f64,
    reviews: &[ReviewResult],
) -> ConfidenceScores {
    let stability = if regressions == 0 { 100.0 } else { 0.0 };
    let behavioral = holdout_rate * 100.0;

    let review_score = if reviews.is_empty() {
        50.0
    } else {
        let approve_count = reviews.iter()
            .filter(|r| r.verdict == Some(ReviewVerdict::Approve))
            .count();
        (approve_count as f64 / reviews.len() as f64) * 100.0
    };

    let overall = 0.25 * exec_health + 0.15 * stability + 0.35 * behavioral + 0.25 * review_score;

    ConfidenceScores {
        execution_health: exec_health,
        baseline_stability: stability,
        behavioral_evidence: behavioral,
        review_alignment: review_score,
        overall,
    }
}

fn compute_evidence_coverage(
    ac_verified: usize,
    ac_total: usize,
    files_reviewed: usize,
    files_total: usize,
) -> f64 {
    let ac_score = if ac_total > 0 {
        (ac_verified as f64 / ac_total as f64) * 0.6
    } else {
        0.3 // neutral when no ACs
    };
    let file_score = if files_total > 0 {
        (files_reviewed as f64 / files_total as f64) * 0.4
    } else {
        0.2
    };
    ac_score + file_score
}

// ---------------------------------------------------------------------------
// Main audit orchestrator
// ---------------------------------------------------------------------------

/// Input parameters for run_audit.
pub struct AuditInput<'a> {
    pub goal: &'a str,
    pub diff: &'a str,
    pub contract_id: &'a str,
    pub tier: &'a AssuranceTier,
    pub mechanic_regressions: usize,
    pub holdout_pass_rate: f64,
    pub ac_verified: usize,
    pub ac_total: usize,
    pub root: &'a Path,
}

/// Run the full audit pipeline.
#[allow(clippy::too_many_arguments)]
pub fn run_audit(input: &AuditInput) -> Result<AuditReport, AuditError> {
    let start = std::time::Instant::now();

    if input.diff.trim().is_empty() {
        return Err(AuditError::NoDiff("no diff to review".into()));
    }

    // Dispatch reviews based on tier
    let mut reviews = Vec::new();

    // T2+: at least Codex
    if *input.tier >= AssuranceTier::T2 {
        reviews.push(run_codex_review(input.goal, input.diff));
    }

    // T3: also Gemini
    if *input.tier >= AssuranceTier::T3 {
        reviews.push(run_gemini_review(input.goal, input.diff));
    }

    // Collect all findings
    let all_findings: Vec<Finding> = reviews.iter()
        .flat_map(|r| r.findings.clone())
        .collect();

    let files_reviewed = all_findings.iter()
        .filter_map(|f| f.file.as_ref())
        .collect::<std::collections::HashSet<_>>()
        .len();

    // Count in-scope files from diff
    let files_total = input.diff.lines()
        .filter(|l| l.starts_with("+++ b/") || l.starts_with("--- a/"))
        .count() / 2;

    // Synthesize decision
    let (decision, release_verdict, confidence, evidence_coverage) = synthesize(
        &reviews,
        input.mechanic_regressions,
        input.holdout_pass_rate,
        input.ac_verified,
        input.ac_total,
        files_reviewed,
        files_total.max(1),
    );

    let duration_ms = start.elapsed().as_millis() as u64;

    let report = AuditReport {
        schema_version: "1.0".to_string(),
        timestamp: Utc::now().to_rfc3339(),
        contract_id: input.contract_id.to_string(),
        tier: input.tier.clone(),
        reviews,
        all_findings,
        decision,
        release_verdict,
        confidence,
        evidence_coverage,
        duration_ms,
    };

    // Save report
    let dir = input.root.join(".punk").join("contracts").join(input.contract_id);
    if dir.exists() {
        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| AuditError::Parse(e.to_string()))?;
        let target = dir.join("audit.json");
        let mut tmp = tempfile::NamedTempFile::new_in(&dir)?;
        std::io::Write::write_all(&mut tmp, json.as_bytes())?;
        tmp.persist(&target).map_err(|e| AuditError::Io(e.error))?;
    }

    Ok(report)
}

// ---------------------------------------------------------------------------
// Renderers
// ---------------------------------------------------------------------------

pub fn render_audit_short(report: &AuditReport) -> String {
    let mut out = format!(
        "punk audit: {:?} → {:?} (confidence: {:.0}%, coverage: {:.0}%)\n",
        report.decision, report.release_verdict,
        report.confidence.overall, report.evidence_coverage * 100.0,
    );

    for r in &report.reviews {
        let status = if let Some(v) = &r.verdict {
            format!("{v:?}")
        } else if let Some(e) = &r.error {
            format!("ERROR: {e}")
        } else {
            "N/A".to_string()
        };
        out.push_str(&format!("  {} ({}) — {} findings, {}ms\n",
            r.provider, status, r.findings.len(), r.duration_ms));
    }

    if !report.all_findings.is_empty() {
        out.push('\n');
        for f in &report.all_findings {
            let loc = match (&f.file, f.line) {
                (Some(file), Some(line)) => format!("[{file}:{line}]"),
                (Some(file), None) => format!("[{file}]"),
                _ => String::new(),
            };
            out.push_str(&format!("  {:?} {} {} ({})\n",
                f.severity, loc, f.message, f.fingerprint));
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

    #[test]
    fn parse_review_with_findings() {
        let response = "\
CRITICAL: [src/auth.rs:42] SQL injection in query builder
MAJOR: [src/api.rs:10] Missing rate limiting
MINOR: Consider using const for magic number
VERDICT: REJECT";

        let r = parse_review_response("test", response, 100);
        assert_eq!(r.verdict, Some(ReviewVerdict::Reject));
        assert_eq!(r.findings.len(), 3);
        assert_eq!(r.findings[0].severity, Severity::Critical);
        assert_eq!(r.findings[0].file, Some("src/auth.rs".to_string()));
        assert_eq!(r.findings[0].line, Some(42));
        assert_eq!(r.findings[1].severity, Severity::Major);
        assert_eq!(r.findings[2].severity, Severity::Minor);
        assert_eq!(r.findings[2].file, None);
    }

    #[test]
    fn parse_approve_verdict() {
        let response = "Looks good.\nVERDICT: APPROVE";
        let r = parse_review_response("test", response, 50);
        assert_eq!(r.verdict, Some(ReviewVerdict::Approve));
        assert!(r.findings.is_empty());
    }

    #[test]
    fn fingerprint_stable() {
        let f1 = Finding::compute_fingerprint("CRITICAL", Some("src/auth.rs"), "SQL injection");
        let f2 = Finding::compute_fingerprint("CRITICAL", Some("src/auth.rs"), "SQL injection");
        assert_eq!(f1, f2);
        assert_eq!(f1.len(), 8);
    }

    #[test]
    fn fingerprint_case_insensitive() {
        let f1 = Finding::compute_fingerprint("CRITICAL", Some("a.rs"), "SQL Injection");
        let f2 = Finding::compute_fingerprint("CRITICAL", Some("a.rs"), "sql injection");
        assert_eq!(f1, f2);
    }

    #[test]
    fn synthesize_auto_block_on_regression() {
        let reviews = vec![ReviewResult {
            provider: "codex".to_string(),
            available: true,
            verdict: Some(ReviewVerdict::Approve),
            findings: vec![],
            error: None,
            duration_ms: 100,
        }];
        let (decision, verdict, _, _) = synthesize(&reviews, 1, 1.0, 3, 3, 2, 2);
        assert_eq!(decision, AuditDecision::AutoBlock);
        assert_eq!(verdict, ReleaseVerdict::Reject);
    }

    #[test]
    fn synthesize_auto_block_on_critical() {
        let reviews = vec![ReviewResult {
            provider: "codex".to_string(),
            available: true,
            verdict: Some(ReviewVerdict::Approve),
            findings: vec![Finding {
                provider: "codex".to_string(),
                severity: Severity::Critical,
                category: "CRITICAL".to_string(),
                file: None, line: None,
                message: "bad".to_string(),
                fingerprint: "abc".to_string(),
            }],
            error: None,
            duration_ms: 100,
        }];
        let (decision, _, _, _) = synthesize(&reviews, 0, 1.0, 3, 3, 2, 2);
        assert_eq!(decision, AuditDecision::AutoBlock);
    }

    #[test]
    fn synthesize_auto_ok_promote() {
        let reviews = vec![ReviewResult {
            provider: "codex".to_string(),
            available: true,
            verdict: Some(ReviewVerdict::Approve),
            findings: vec![],
            error: None,
            duration_ms: 100,
        }];
        let (decision, verdict, confidence, coverage) = synthesize(&reviews, 0, 1.0, 3, 3, 2, 2);
        assert_eq!(decision, AuditDecision::AutoOk);
        assert_eq!(verdict, ReleaseVerdict::Promote);
        assert!(confidence.overall > 50.0);
        assert!(coverage >= 0.7);
    }

    #[test]
    fn synthesize_auto_ok_hold_low_coverage() {
        let reviews = vec![ReviewResult {
            provider: "codex".to_string(),
            available: true,
            verdict: Some(ReviewVerdict::Approve),
            findings: vec![],
            error: None,
            duration_ms: 100,
        }];
        let (decision, verdict, _, coverage) = synthesize(&reviews, 0, 1.0, 1, 5, 1, 10);
        assert_eq!(decision, AuditDecision::AutoOk);
        assert_eq!(verdict, ReleaseVerdict::Hold);
        assert!(coverage < 0.7);
    }

    #[test]
    fn synthesize_human_review_on_disagreement() {
        let reviews = vec![
            ReviewResult {
                provider: "codex".to_string(), available: true,
                verdict: Some(ReviewVerdict::Approve),
                findings: vec![], error: None, duration_ms: 100,
            },
            ReviewResult {
                provider: "gemini".to_string(), available: true,
                verdict: Some(ReviewVerdict::Conditional),
                findings: vec![Finding {
                    provider: "gemini".to_string(),
                    severity: Severity::Major,
                    category: "MAJOR".to_string(),
                    file: None, line: None,
                    message: "concern".to_string(),
                    fingerprint: "xyz".to_string(),
                }],
                error: None, duration_ms: 200,
            },
        ];
        let (decision, _, _, _) = synthesize(&reviews, 0, 1.0, 3, 3, 2, 2);
        assert_eq!(decision, AuditDecision::HumanReview);
    }

    #[test]
    fn evidence_coverage_computation() {
        // 3/3 ACs + 2/2 files = 0.6 + 0.4 = 1.0
        let c = compute_evidence_coverage(3, 3, 2, 2);
        assert!((c - 1.0).abs() < 0.01);

        // 1/4 ACs + 1/4 files = 0.15 + 0.1 = 0.25
        let c = compute_evidence_coverage(1, 4, 1, 4);
        assert!((c - 0.25).abs() < 0.01);
    }

    #[test]
    fn confidence_scoring() {
        let reviews = vec![ReviewResult {
            provider: "codex".to_string(), available: true,
            verdict: Some(ReviewVerdict::Approve),
            findings: vec![], error: None, duration_ms: 100,
        }];
        let c = compute_confidence(90.0, 0, 1.0, &reviews);
        // 0.25*90 + 0.15*100 + 0.35*100 + 0.25*100 = 22.5 + 15 + 35 + 25 = 97.5
        assert!((c.overall - 97.5).abs() < 0.01);
    }

    #[test]
    fn audit_report_roundtrip() {
        let report = AuditReport {
            schema_version: "1.0".to_string(),
            timestamp: "t".to_string(),
            contract_id: "c".to_string(),
            tier: AssuranceTier::T2,
            reviews: vec![],
            all_findings: vec![],
            decision: AuditDecision::AutoOk,
            release_verdict: ReleaseVerdict::Promote,
            confidence: ConfidenceScores {
                execution_health: 90.0,
                baseline_stability: 100.0,
                behavioral_evidence: 80.0,
                review_alignment: 100.0,
                overall: 92.5,
            },
            evidence_coverage: 0.85,
            duration_ms: 500,
        };
        let json = serde_json::to_string_pretty(&report).unwrap();
        let back: AuditReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.decision, AuditDecision::AutoOk);
        assert_eq!(back.tier, AssuranceTier::T2);
    }

    #[test]
    fn render_output() {
        let report = AuditReport {
            schema_version: "1.0".to_string(),
            timestamp: "t".to_string(),
            contract_id: "abc".to_string(),
            tier: AssuranceTier::T3,
            reviews: vec![ReviewResult {
                provider: "codex".to_string(), available: true,
                verdict: Some(ReviewVerdict::Approve),
                findings: vec![], error: None, duration_ms: 100,
            }],
            all_findings: vec![],
            decision: AuditDecision::AutoOk,
            release_verdict: ReleaseVerdict::Promote,
            confidence: ConfidenceScores {
                execution_health: 90.0, baseline_stability: 100.0,
                behavioral_evidence: 80.0, review_alignment: 100.0,
                overall: 92.0,
            },
            evidence_coverage: 0.85,
            duration_ms: 500,
        };
        let out = render_audit_short(&report);
        assert!(out.contains("AutoOk"));
        assert!(out.contains("Promote"));
        assert!(out.contains("codex"));
    }
}
