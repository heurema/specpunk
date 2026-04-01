//! Repair loop: generate fix briefs from audit findings, track iterations.
//!
//! Flow: audit fails → repair brief (no holdout details) → fix → re-audit.
//! Best-of-N selection: keep highest confidence across iterations.
//! Early stop: 2 consecutive non-improving iterations → halt.

use serde::{Deserialize, Serialize};

use crate::audit::{AuditDecision, AuditReport, Finding, Severity};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A repair brief: tells the implementer what to fix without revealing holdouts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairBrief {
    pub iteration: u32,
    pub contract_id: String,
    pub findings_to_fix: Vec<RepairFinding>,
    pub constraints: Vec<String>,
}

/// A finding simplified for the repair brief (no provider attribution).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairFinding {
    pub severity: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub message: String,
    pub fingerprint: String,
}

/// Tracks state across repair iterations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairState {
    pub schema_version: String,
    pub contract_id: String,
    pub max_iterations: u32,
    pub current_iteration: u32,
    pub iterations: Vec<IterationRecord>,
    pub best_iteration: u32,
    pub best_confidence: f64,
    pub status: RepairStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationRecord {
    pub iteration: u32,
    pub decision: AuditDecision,
    pub confidence: f64,
    pub findings_count: usize,
    pub critical_count: usize,
    pub major_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairStatus {
    /// More iterations possible.
    InProgress,
    /// Audit passed — no more repairs needed.
    Resolved,
    /// Max iterations reached without resolution.
    MaxReached,
    /// 2 consecutive non-improving iterations.
    EarlyStopped,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default max repair iterations.
pub const DEFAULT_MAX_ITERATIONS: u32 = 3;

/// Consecutive non-improving iterations before early stop.
const EARLY_STOP_THRESHOLD: usize = 2;

// ---------------------------------------------------------------------------
// Brief generation
// ---------------------------------------------------------------------------

/// Generate a repair brief from audit findings.
/// NEVER includes holdout details — prevents inference by implementer.
pub fn generate_brief(
    contract_id: &str,
    iteration: u32,
    findings: &[Finding],
    scope_files: &[String],
) -> RepairBrief {
    // Only include MAJOR and CRITICAL findings
    let to_fix: Vec<RepairFinding> = findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::Major | Severity::Critical))
        .map(|f| RepairFinding {
            severity: format!("{:?}", f.severity),
            file: f.file.clone(),
            line: f.line,
            message: f.message.clone(),
            fingerprint: f.fingerprint.clone(),
        })
        .collect();

    // Constraints: only modify files in scope
    let mut constraints = vec![
        "Fix ONLY the listed findings".to_string(),
        "Do NOT modify test files unless a finding specifically references them".to_string(),
    ];
    if !scope_files.is_empty() {
        constraints.push(format!("Stay within scope: {}", scope_files.join(", ")));
    }

    RepairBrief {
        iteration,
        contract_id: contract_id.to_string(),
        findings_to_fix: to_fix,
        constraints,
    }
}

// ---------------------------------------------------------------------------
// Iteration tracking
// ---------------------------------------------------------------------------

/// Create initial repair state.
pub fn init_state(contract_id: &str, max_iterations: u32) -> RepairState {
    RepairState {
        schema_version: "1.0".to_string(),
        contract_id: contract_id.to_string(),
        max_iterations,
        current_iteration: 0,
        iterations: vec![],
        best_iteration: 0,
        best_confidence: 0.0,
        status: RepairStatus::InProgress,
    }
}

/// Record an iteration result and determine next action.
pub fn record_iteration(state: &mut RepairState, report: &AuditReport) {
    let iteration = state.current_iteration + 1;
    state.current_iteration = iteration;

    let critical_count = report
        .all_findings
        .iter()
        .filter(|f| f.severity == Severity::Critical)
        .count();
    let major_count = report
        .all_findings
        .iter()
        .filter(|f| f.severity == Severity::Major)
        .count();

    let record = IterationRecord {
        iteration,
        decision: report.decision.clone(),
        confidence: report.confidence.overall,
        findings_count: report.all_findings.len(),
        critical_count,
        major_count,
    };

    // Update best
    if report.confidence.overall > state.best_confidence {
        state.best_confidence = report.confidence.overall;
        state.best_iteration = iteration;
    }

    state.iterations.push(record);

    // Determine status
    if report.decision == AuditDecision::AutoOk {
        state.status = RepairStatus::Resolved;
        return;
    }

    if iteration >= state.max_iterations {
        state.status = RepairStatus::MaxReached;
        return;
    }

    // Early stop: 2 consecutive non-improving iterations
    if state.iterations.len() >= EARLY_STOP_THRESHOLD {
        let recent: Vec<_> = state
            .iterations
            .iter()
            .rev()
            .take(EARLY_STOP_THRESHOLD)
            .collect();
        let all_non_improving = recent
            .windows(2)
            .all(|w| w[0].confidence <= w[1].confidence);
        // Check if none improved over best
        let none_improved = recent
            .iter()
            .all(|r| r.confidence <= state.best_confidence - 0.01);
        if (all_non_improving || none_improved) && state.iterations.len() > EARLY_STOP_THRESHOLD {
            state.status = RepairStatus::EarlyStopped;
            return;
        }
    }

    state.status = RepairStatus::InProgress;
}

/// Check if more iterations are allowed.
pub fn can_continue(state: &RepairState) -> bool {
    state.status == RepairStatus::InProgress
}

// ---------------------------------------------------------------------------
// Renderers
// ---------------------------------------------------------------------------

pub fn render_brief(brief: &RepairBrief) -> String {
    let mut out = format!("punk repair: iteration {} brief\n\n", brief.iteration);

    if brief.findings_to_fix.is_empty() {
        out.push_str("  No MAJOR/CRITICAL findings to fix.\n");
        return out;
    }

    out.push_str(&format!(
        "  {} findings to fix:\n",
        brief.findings_to_fix.len()
    ));
    for f in &brief.findings_to_fix {
        let loc = match (&f.file, f.line) {
            (Some(file), Some(line)) => format!("[{file}:{line}]"),
            (Some(file), None) => format!("[{file}]"),
            _ => String::new(),
        };
        out.push_str(&format!(
            "    {} {} {} ({})\n",
            f.severity, loc, f.message, f.fingerprint
        ));
    }

    out.push_str("\n  Constraints:\n");
    for c in &brief.constraints {
        out.push_str(&format!("    - {c}\n"));
    }

    out
}

pub fn render_state(state: &RepairState) -> String {
    let mut out = format!(
        "punk repair: {:?} (iteration {}/{}, best=#{} confidence={:.0}%)\n",
        state.status,
        state.current_iteration,
        state.max_iterations,
        state.best_iteration,
        state.best_confidence,
    );

    for r in &state.iterations {
        out.push_str(&format!(
            "  #{}: {:?} confidence={:.0}% findings={} ({}C/{}M)\n",
            r.iteration,
            r.decision,
            r.confidence,
            r.findings_count,
            r.critical_count,
            r.major_count,
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
    use crate::audit::*;
    use crate::risk::AssuranceTier;

    fn make_report(
        decision: AuditDecision,
        confidence: f64,
        findings: Vec<Finding>,
    ) -> AuditReport {
        AuditReport {
            schema_version: "1.0".to_string(),
            timestamp: "t".to_string(),
            contract_id: "c".to_string(),
            tier: AssuranceTier::T2,
            reviews: vec![],
            all_findings: findings,
            decision,
            release_verdict: ReleaseVerdict::Hold,
            confidence: ConfidenceScores {
                execution_health: confidence,
                baseline_stability: 100.0,
                behavioral_evidence: confidence,
                review_alignment: confidence,
                overall: confidence,
            },
            evidence_coverage: 0.5,
            duration_ms: 100,
        }
    }

    #[test]
    fn brief_filters_major_critical() {
        let findings = vec![
            Finding {
                provider: "codex".into(),
                severity: Severity::Critical,
                category: "CRITICAL".into(),
                file: Some("a.rs".into()),
                line: Some(10),
                message: "SQL injection".into(),
                fingerprint: "abc".into(),
            },
            Finding {
                provider: "codex".into(),
                severity: Severity::Minor,
                category: "MINOR".into(),
                file: None,
                line: None,
                message: "style".into(),
                fingerprint: "def".into(),
            },
            Finding {
                provider: "gemini".into(),
                severity: Severity::Major,
                category: "MAJOR".into(),
                file: Some("b.rs".into()),
                line: None,
                message: "no error handling".into(),
                fingerprint: "ghi".into(),
            },
        ];

        let brief = generate_brief("c1", 1, &findings, &["src/".into()]);
        assert_eq!(brief.findings_to_fix.len(), 2); // Only CRITICAL + MAJOR
        assert_eq!(brief.findings_to_fix[0].severity, "Critical");
        assert_eq!(brief.findings_to_fix[1].severity, "Major");
    }

    #[test]
    fn brief_no_holdout_info() {
        let brief = generate_brief("c1", 1, &[], &[]);
        let json = serde_json::to_string(&brief).unwrap();
        assert!(!json.contains("holdout"));
        assert!(!json.contains("HO-"));
    }

    #[test]
    fn state_resolves_on_auto_ok() {
        let mut state = init_state("c1", 3);
        let report = make_report(AuditDecision::AutoOk, 90.0, vec![]);
        record_iteration(&mut state, &report);
        assert_eq!(state.status, RepairStatus::Resolved);
        assert_eq!(state.current_iteration, 1);
    }

    #[test]
    fn state_max_reached() {
        let mut state = init_state("c1", 2);

        let r1 = make_report(AuditDecision::AutoBlock, 40.0, vec![]);
        record_iteration(&mut state, &r1);
        assert_eq!(state.status, RepairStatus::InProgress);

        let r2 = make_report(AuditDecision::AutoBlock, 50.0, vec![]);
        record_iteration(&mut state, &r2);
        assert_eq!(state.status, RepairStatus::MaxReached);
    }

    #[test]
    fn state_tracks_best() {
        let mut state = init_state("c1", 5);

        record_iteration(
            &mut state,
            &make_report(AuditDecision::HumanReview, 60.0, vec![]),
        );
        record_iteration(
            &mut state,
            &make_report(AuditDecision::HumanReview, 80.0, vec![]),
        );
        record_iteration(
            &mut state,
            &make_report(AuditDecision::HumanReview, 70.0, vec![]),
        );

        assert_eq!(state.best_iteration, 2);
        assert!((state.best_confidence - 80.0).abs() < 0.01);
    }

    #[test]
    fn can_continue_checks() {
        let mut state = init_state("c1", 3);
        assert!(can_continue(&state));

        record_iteration(
            &mut state,
            &make_report(AuditDecision::AutoOk, 90.0, vec![]),
        );
        assert!(!can_continue(&state));
    }

    #[test]
    fn render_brief_output() {
        let brief = generate_brief(
            "c1",
            2,
            &[Finding {
                provider: "codex".into(),
                severity: Severity::Critical,
                category: "CRITICAL".into(),
                file: Some("auth.rs".into()),
                line: Some(42),
                message: "buffer overflow".into(),
                fingerprint: "abc12345".into(),
            }],
            &["src/".into()],
        );

        let out = render_brief(&brief);
        assert!(out.contains("iteration 2"));
        assert!(out.contains("buffer overflow"));
        assert!(out.contains("[auth.rs:42]"));
        assert!(out.contains("Stay within scope"));
    }

    #[test]
    fn render_state_output() {
        let mut state = init_state("c1", 3);
        record_iteration(
            &mut state,
            &make_report(AuditDecision::AutoBlock, 40.0, vec![]),
        );
        record_iteration(
            &mut state,
            &make_report(AuditDecision::HumanReview, 60.0, vec![]),
        );

        let out = render_state(&state);
        assert!(out.contains("iteration 2/3"));
        assert!(out.contains("#1: AutoBlock"));
        assert!(out.contains("#2: HumanReview"));
    }

    #[test]
    fn state_roundtrip() {
        let mut state = init_state("c1", 3);
        record_iteration(
            &mut state,
            &make_report(AuditDecision::AutoBlock, 50.0, vec![]),
        );

        let json = serde_json::to_string_pretty(&state).unwrap();
        let back: RepairState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.current_iteration, 1);
        assert_eq!(back.status, RepairStatus::InProgress);
    }
}
