//! Holdout testing: blind acceptance criteria verification.
//!
//! Holdouts are hidden test scenarios the implementer never sees.
//! After implementation, punk runs each holdout's DSL steps against
//! the live code to verify it actually works — not just that tests pass.
//! All execution uses Command::new (no shell, no injection risk).

use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::dsl;
use crate::plan::contract::{Contract, Holdout, RiskLevel};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HoldoutStatus {
    Pass,
    Fail,
    Error,
    Timeout,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldoutResult {
    pub id: String,
    pub description: String,
    pub status: HoldoutStatus,
    pub steps_run: usize,
    pub failed_at_step: Option<usize>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldoutReport {
    pub schema_version: String,
    pub timestamp: String,
    pub contract_id: String,
    pub risk_level: RiskLevel,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub pass_rate: f64,
    pub meets_threshold: bool,
    pub results: Vec<HoldoutResult>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum HoldoutError {
    NoContract(String),
    NoHoldouts,
    Io(std::io::Error),
    Parse(String),
}

impl std::fmt::Display for HoldoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HoldoutError::NoContract(m) => write!(f, "no contract: {m}"),
            HoldoutError::NoHoldouts => write!(f, "contract has no holdout scenarios"),
            HoldoutError::Io(e) => write!(f, "I/O error: {e}"),
            HoldoutError::Parse(m) => write!(f, "parse error: {m}"),
        }
    }
}

impl std::error::Error for HoldoutError {}

impl From<std::io::Error> for HoldoutError {
    fn from(e: std::io::Error) -> Self {
        HoldoutError::Io(e)
    }
}

// ---------------------------------------------------------------------------
// Thresholds
// ---------------------------------------------------------------------------

pub fn min_holdouts_for_risk(risk: &RiskLevel) -> usize {
    match risk {
        RiskLevel::Low => 0,
        RiskLevel::Medium => 2,
        RiskLevel::High => 5,
    }
}

fn required_pass_rate(risk: &RiskLevel) -> f64 {
    match risk {
        RiskLevel::Low => 0.0,
        RiskLevel::Medium => 1.0,
        RiskLevel::High => 1.0,
    }
}

// ---------------------------------------------------------------------------
// Engineer view
// ---------------------------------------------------------------------------

/// Strip holdouts from contract for implementer blinding.
pub fn strip_holdouts(contract: &Contract) -> Contract {
    let mut view = contract.clone();
    view.holdout_scenarios = vec![];
    view.approval_hash = None;
    view
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

/// Run all holdout scenarios. Uses DSL engine (Command::new, no shell).
pub fn run_holdouts(
    contract: &Contract,
    working_dir: &Path,
) -> Result<HoldoutReport, HoldoutError> {
    if contract.holdout_scenarios.is_empty() {
        let min = min_holdouts_for_risk(&contract.risk_level);
        if min > 0 {
            return Err(HoldoutError::NoHoldouts);
        }
        return Ok(HoldoutReport {
            schema_version: "1.0".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            contract_id: contract.change_id.clone(),
            risk_level: contract.risk_level.clone(),
            total: 0,
            passed: 0,
            failed: 0,
            pass_rate: 1.0,
            meets_threshold: true,
            results: vec![],
        });
    }

    let results: Vec<HoldoutResult> = contract
        .holdout_scenarios
        .iter()
        .map(|h| run_single(h, working_dir))
        .collect();

    let total = results.len();
    let passed = results
        .iter()
        .filter(|r| r.status == HoldoutStatus::Pass)
        .count();
    let failed = total - passed;
    let pass_rate = if total > 0 {
        passed as f64 / total as f64
    } else {
        1.0
    };
    let meets_threshold = pass_rate >= required_pass_rate(&contract.risk_level);

    let report = HoldoutReport {
        schema_version: "1.0".to_string(),
        timestamp: Utc::now().to_rfc3339(),
        contract_id: contract.change_id.clone(),
        risk_level: contract.risk_level.clone(),
        total,
        passed,
        failed,
        pass_rate,
        meets_threshold,
        results,
    };

    // Save report
    let dir = working_dir
        .join(".punk")
        .join("contracts")
        .join(&contract.change_id);
    if dir.exists() {
        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| HoldoutError::Parse(e.to_string()))?;
        let target = dir.join("holdout.json");
        let mut tmp = tempfile::NamedTempFile::new_in(&dir)?;
        std::io::Write::write_all(&mut tmp, json.as_bytes())?;
        tmp.persist(&target)
            .map_err(|e| HoldoutError::Io(e.error))?;
    }

    Ok(report)
}

fn run_single(holdout: &Holdout, working_dir: &Path) -> HoldoutResult {
    if holdout.steps.is_empty() {
        return HoldoutResult {
            id: holdout.id.clone(),
            description: holdout.description.clone(),
            status: HoldoutStatus::Skipped,
            steps_run: 0,
            failed_at_step: None,
            error: Some("no verify steps".to_string()),
            duration_ms: 0,
        };
    }

    let r = dsl::run_steps(&holdout.steps, working_dir);

    let status = if r.passed {
        HoldoutStatus::Pass
    } else {
        HoldoutStatus::Fail
    };

    HoldoutResult {
        id: holdout.id.clone(),
        description: holdout.description.clone(),
        status,
        steps_run: r.steps_run,
        failed_at_step: r.failed_at_step,
        error: r.error,
        duration_ms: r.duration_ms,
    }
}

// ---------------------------------------------------------------------------
// Renderers
// ---------------------------------------------------------------------------

pub fn render_holdout_short(report: &HoldoutReport) -> String {
    if report.total == 0 {
        return format!(
            "punk holdout: no holdouts (risk={:?}, min={})\n",
            report.risk_level,
            min_holdouts_for_risk(&report.risk_level),
        );
    }

    let verdict = if report.meets_threshold {
        "PASS"
    } else {
        "FAIL"
    };
    let mut out = format!(
        "punk holdout: {} ({}/{} passed, {:.0}% rate, risk={:?})\n",
        verdict,
        report.passed,
        report.total,
        report.pass_rate * 100.0,
        report.risk_level,
    );

    for r in &report.results {
        let icon = match r.status {
            HoldoutStatus::Pass => "  PASS",
            HoldoutStatus::Fail => "  FAIL",
            HoldoutStatus::Error => "  ERR ",
            HoldoutStatus::Timeout => "  TIME",
            HoldoutStatus::Skipped => "  SKIP",
        };
        out.push_str(&format!(
            "  {} [{}] {} ({}ms)\n",
            icon, r.id, r.description, r.duration_ms
        ));
        if let Some(err) = &r.error {
            out.push_str(&format!("       {err}\n"));
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
    use crate::dsl::DslStep;
    use crate::plan::ceremony::{CeremonyLevel, ModelTier};
    use crate::plan::contract::*;
    use tempfile::TempDir;

    fn make_contract(risk: RiskLevel, holdouts: Vec<Holdout>) -> Contract {
        Contract {
            version: CONTRACT_VERSION.to_string(),
            goal: "test".to_string(),
            scope: Scope {
                touch: vec![],
                dont_touch: vec![],
            },
            acceptance_criteria: vec![],
            assumptions: vec![],
            warnings: vec![],
            ceremony_level: CeremonyLevel::Skip,
            created_at: "2026-03-25".to_string(),
            change_id: "holdout-test".to_string(),
            approval_hash: None,
            routing_metadata: RoutingMetadata {
                complexity_score: 1,
                ceremony_level: CeremonyLevel::Skip,
                suggested_model_tier: ModelTier::Haiku,
                latency_ms: 0,
                token_estimate: 0,
                router_policy_version: "1.0".to_string(),
                unfamiliarity_ratio: 0.0,
            },
            task_id: "tid".to_string(),
            attempt_number: 1,
            risk_level: risk,
            holdout_scenarios: holdouts,
            removals: vec![],
            cleanup_obligations: vec![],
            context_inheritance: ContextInheritance::default(),
        }
    }

    #[test]
    fn low_risk_no_holdouts_passes() {
        let c = make_contract(RiskLevel::Low, vec![]);
        let tmp = TempDir::new().unwrap();
        let r = run_holdouts(&c, tmp.path()).unwrap();
        assert!(r.meets_threshold);
        assert_eq!(r.total, 0);
    }

    #[test]
    fn medium_risk_no_holdouts_fails() {
        let c = make_contract(RiskLevel::Medium, vec![]);
        let tmp = TempDir::new().unwrap();
        assert!(matches!(
            run_holdouts(&c, tmp.path()),
            Err(HoldoutError::NoHoldouts)
        ));
    }

    #[test]
    fn holdout_passes() {
        let h = Holdout {
            id: "HO-1".to_string(),
            description: "file exists".to_string(),
            steps: vec![
                DslStep::Exec {
                    argv: vec!["test".to_string(), "-f".to_string(), "t.txt".to_string()],
                    capture: Some("r".to_string()),
                    timeout_ms: None,
                },
                DslStep::Expect {
                    source: "r".to_string(),
                    json_path: None,
                    exit_code: Some(0),
                    equals: None,
                    contains: None,
                },
            ],
            timeout_ms: 5000,
        };
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("t.txt"), "x").unwrap();
        let r = run_holdouts(&make_contract(RiskLevel::Medium, vec![h]), tmp.path()).unwrap();
        assert_eq!(r.passed, 1);
        assert!(r.meets_threshold);
    }

    #[test]
    fn holdout_fails() {
        let h = Holdout {
            id: "HO-1".to_string(),
            description: "missing file".to_string(),
            steps: vec![
                DslStep::Exec {
                    argv: vec!["test".to_string(), "-f".to_string(), "nope".to_string()],
                    capture: Some("r".to_string()),
                    timeout_ms: None,
                },
                DslStep::Expect {
                    source: "r".to_string(),
                    json_path: None,
                    exit_code: Some(0),
                    equals: None,
                    contains: None,
                },
            ],
            timeout_ms: 5000,
        };
        let tmp = TempDir::new().unwrap();
        let r = run_holdouts(&make_contract(RiskLevel::Medium, vec![h]), tmp.path()).unwrap();
        assert_eq!(r.failed, 1);
        assert!(!r.meets_threshold);
    }

    #[test]
    fn mixed_results() {
        let pass = Holdout {
            id: "HO-1".to_string(),
            description: "echo".to_string(),
            steps: vec![
                DslStep::Exec {
                    argv: vec!["echo".to_string(), "ok".to_string()],
                    capture: Some("r".to_string()),
                    timeout_ms: None,
                },
                DslStep::Expect {
                    source: "r".to_string(),
                    json_path: None,
                    exit_code: Some(0),
                    equals: None,
                    contains: Some("ok".to_string()),
                },
            ],
            timeout_ms: 5000,
        };
        let fail = Holdout {
            id: "HO-2".to_string(),
            description: "nope".to_string(),
            steps: vec![
                DslStep::Exec {
                    argv: vec!["test".to_string(), "-f".to_string(), "x".to_string()],
                    capture: Some("r".to_string()),
                    timeout_ms: None,
                },
                DslStep::Expect {
                    source: "r".to_string(),
                    json_path: None,
                    exit_code: Some(0),
                    equals: None,
                    contains: None,
                },
            ],
            timeout_ms: 5000,
        };
        let tmp = TempDir::new().unwrap();
        let r = run_holdouts(
            &make_contract(RiskLevel::High, vec![pass, fail]),
            tmp.path(),
        )
        .unwrap();
        assert_eq!(r.pass_rate, 0.5);
        assert!(!r.meets_threshold);
    }

    #[test]
    fn strip_removes_holdouts() {
        let c = make_contract(
            RiskLevel::Medium,
            vec![Holdout {
                id: "HO-1".to_string(),
                description: "secret".to_string(),
                steps: vec![],
                timeout_ms: 5000,
            }],
        );
        let view = strip_holdouts(&c);
        assert!(view.holdout_scenarios.is_empty());
        assert!(view.approval_hash.is_none());
    }

    #[test]
    fn report_roundtrip() {
        let r = HoldoutReport {
            schema_version: "1.0".to_string(),
            timestamp: "t".to_string(),
            contract_id: "c".to_string(),
            risk_level: RiskLevel::High,
            total: 5,
            passed: 4,
            failed: 1,
            pass_rate: 0.8,
            meets_threshold: false,
            results: vec![],
        };
        let j = serde_json::to_string(&r).unwrap();
        let back: HoldoutReport = serde_json::from_str(&j).unwrap();
        assert_eq!(back.risk_level, RiskLevel::High);
    }

    #[test]
    fn thresholds() {
        assert_eq!(min_holdouts_for_risk(&RiskLevel::Low), 0);
        assert_eq!(min_holdouts_for_risk(&RiskLevel::Medium), 2);
        assert_eq!(min_holdouts_for_risk(&RiskLevel::High), 5);
    }

    #[test]
    fn render_output() {
        let r = HoldoutReport {
            schema_version: "1.0".to_string(),
            timestamp: "t".to_string(),
            contract_id: "abc".to_string(),
            risk_level: RiskLevel::Medium,
            total: 2,
            passed: 2,
            failed: 0,
            pass_rate: 1.0,
            meets_threshold: true,
            results: vec![
                HoldoutResult {
                    id: "HO-1".to_string(),
                    description: "api ok".to_string(),
                    status: HoldoutStatus::Pass,
                    steps_run: 2,
                    failed_at_step: None,
                    error: None,
                    duration_ms: 30,
                },
                HoldoutResult {
                    id: "HO-2".to_string(),
                    description: "file ok".to_string(),
                    status: HoldoutStatus::Pass,
                    steps_run: 1,
                    failed_at_step: None,
                    error: None,
                    duration_ms: 5,
                },
            ],
        };
        let out = render_holdout_short(&r);
        assert!(out.contains("PASS"));
        assert!(out.contains("2/2"));
    }
}
