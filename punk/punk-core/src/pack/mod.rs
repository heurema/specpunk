//! Proofpack: self-contained verification artifact for CI gating.
//! Embeds all punk artifacts with SHA-256 envelopes.
//! Holdouts redacted in embedded contract (full hash preserved).

use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::audit::{AuditDecision, AuditReport, ConfidenceScores, ReleaseVerdict};
use crate::plan::sha256_hex;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// SHA-256 wrapped artifact envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub sha256: String,
    pub size_bytes: usize,
    pub status: String,
}

impl Envelope {
    /// Create an envelope from raw content. Omits content if >100KiB.
    pub fn from_content(raw: &str) -> Self {
        let size = raw.len();
        let sha = sha256_hex(raw.as_bytes());
        if size > 100 * 1024 {
            Envelope {
                content: None,
                sha256: sha,
                size_bytes: size,
                status: "omitted".to_string(),
            }
        } else {
            Envelope {
                content: Some(raw.to_string()),
                sha256: sha,
                size_bytes: size,
                status: "present".to_string(),
            }
        }
    }

    /// Create from a file path. Returns None envelope if file missing.
    pub fn from_file(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(raw) => Self::from_content(&raw),
            Err(_) => Envelope {
                content: None,
                sha256: String::new(),
                size_bytes: 0,
                status: "missing".to_string(),
            },
        }
    }
}

/// Holdout summary (counts only, scenarios redacted).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldoutSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
}

/// All verification checks bundled together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofChecks {
    pub scope: Envelope,
    pub mechanic: Envelope,
    pub holdouts: HoldoutSummary,
    pub audit: Envelope,
}

/// Self-contained proofpack — the final artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proofpack {
    pub schema_version: String,
    pub run_id: String,
    pub timestamp: String,
    pub decision: AuditDecision,
    pub release_verdict: ReleaseVerdict,
    pub confidence: ConfidenceScores,
    pub evidence_coverage: f64,
    /// Contract with holdouts stripped. Full hash preserved.
    pub contract: Envelope,
    /// Full contract hash (including holdouts) for verification.
    pub contract_full_sha256: String,
    pub diff: Envelope,
    pub checks: ProofChecks,
    pub punk_version: String,
}

// ---------------------------------------------------------------------------
// CI exit codes
// ---------------------------------------------------------------------------

pub const CI_PROMOTE: i32 = 0;
pub const CI_REJECT: i32 = 1;
pub const CI_HOLD: i32 = 2;

// ---------------------------------------------------------------------------
// Pack assembly
// ---------------------------------------------------------------------------

/// Assemble proofpack from contract artifacts directory.
pub fn assemble(
    contract_dir: &Path,
    contract_raw: &str,
    contract_stripped: &str,
    diff: &str,
    audit_report: &AuditReport,
    holdout_total: usize,
    holdout_passed: usize,
) -> Proofpack {
    let run_id = format!(
        "punk-{}-{}",
        Utc::now().format("%Y%m%d-%H%M%S"),
        &sha256_hex(contract_raw.as_bytes())[..6],
    );

    let checks = ProofChecks {
        scope: Envelope::from_file(&contract_dir.join("receipts").join("check.json")),
        mechanic: Envelope::from_file(&contract_dir.join("mechanic.json")),
        holdouts: HoldoutSummary {
            total: holdout_total,
            passed: holdout_passed,
            failed: holdout_total.saturating_sub(holdout_passed),
        },
        audit: Envelope::from_file(&contract_dir.join("audit.json")),
    };

    Proofpack {
        schema_version: "1.0".to_string(),
        run_id,
        timestamp: Utc::now().to_rfc3339(),
        decision: audit_report.decision.clone(),
        release_verdict: audit_report.release_verdict.clone(),
        confidence: audit_report.confidence.clone(),
        evidence_coverage: audit_report.evidence_coverage,
        contract: Envelope::from_content(contract_stripped),
        contract_full_sha256: sha256_hex(contract_raw.as_bytes()),
        diff: Envelope::from_content(diff),
        checks,
        punk_version: "0.1.0".to_string(),
    }
}

/// Save proofpack atomically.
pub fn save(proofpack: &Proofpack, contract_dir: &Path) -> Result<std::path::PathBuf, std::io::Error> {
    let json = serde_json::to_string_pretty(proofpack)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let target = contract_dir.join("proofpack.json");
    let mut tmp = tempfile::NamedTempFile::new_in(contract_dir)?;
    std::io::Write::write_all(&mut tmp, json.as_bytes())?;
    tmp.persist(&target).map_err(|e| e.error)?;
    Ok(target)
}

// ---------------------------------------------------------------------------
// CI gate
// ---------------------------------------------------------------------------

/// Read proofpack and determine CI exit code.
pub fn ci_gate(proofpack_path: &Path) -> Result<(i32, String), String> {
    let raw = std::fs::read_to_string(proofpack_path)
        .map_err(|e| format!("cannot read proofpack: {e}"))?;
    let pack: Proofpack = serde_json::from_str(&raw)
        .map_err(|e| format!("invalid proofpack JSON: {e}"))?;

    let (code, label) = match pack.release_verdict {
        ReleaseVerdict::Promote => (CI_PROMOTE, "PROMOTE"),
        ReleaseVerdict::Reject => (CI_REJECT, "REJECT"),
        ReleaseVerdict::Hold => (CI_HOLD, "HOLD"),
    };

    let summary = format!(
        "punk ci: {} (confidence={:.0}%, coverage={:.0}%, decision={:?})",
        label, pack.confidence.overall, pack.evidence_coverage * 100.0, pack.decision,
    );

    Ok((code, summary))
}

// ---------------------------------------------------------------------------
// Renderers
// ---------------------------------------------------------------------------

pub fn render_pack_short(pack: &Proofpack) -> String {
    let mut out = format!(
        "punk pack: {:?} → {:?} (confidence={:.0}%, coverage={:.0}%)\n",
        pack.decision, pack.release_verdict,
        pack.confidence.overall, pack.evidence_coverage * 100.0,
    );
    out.push_str(&format!("  run:      {}\n", pack.run_id));
    out.push_str(&format!("  contract: {} ({})\n", &pack.contract.sha256[..16], pack.contract.status));
    out.push_str(&format!("  diff:     {} ({})\n", &pack.diff.sha256[..16], pack.diff.status));
    out.push_str(&format!("  scope:    {}\n", pack.checks.scope.status));
    out.push_str(&format!("  mechanic: {}\n", pack.checks.mechanic.status));
    out.push_str(&format!(
        "  holdouts: {}/{} passed\n",
        pack.checks.holdouts.passed, pack.checks.holdouts.total,
    ));
    out.push_str(&format!("  audit:    {}\n", pack.checks.audit.status));
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
    use tempfile::TempDir;

    fn make_audit() -> AuditReport {
        AuditReport {
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
                overall: 92.0,
            },
            evidence_coverage: 0.85,
            duration_ms: 100,
        }
    }

    #[test]
    fn envelope_from_small_content() {
        let e = Envelope::from_content("hello world");
        assert_eq!(e.status, "present");
        assert!(e.content.is_some());
        assert_eq!(e.size_bytes, 11);
        assert_eq!(e.sha256.len(), 64);
    }

    #[test]
    fn envelope_from_large_content() {
        let big = "x".repeat(200 * 1024);
        let e = Envelope::from_content(&big);
        assert_eq!(e.status, "omitted");
        assert!(e.content.is_none());
        assert_eq!(e.size_bytes, 200 * 1024);
    }

    #[test]
    fn envelope_from_missing_file() {
        let e = Envelope::from_file(Path::new("/nonexistent/file.json"));
        assert_eq!(e.status, "missing");
        assert!(e.sha256.is_empty());
    }

    #[test]
    fn assemble_proofpack() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("receipts")).unwrap();
        std::fs::write(dir.join("receipts/check.json"), r#"{"status":"PASS"}"#).unwrap();

        let audit = make_audit();
        let pack = assemble(dir, "{}", "{}", "diff content", &audit, 3, 3);

        assert_eq!(pack.schema_version, "1.0");
        assert!(pack.run_id.starts_with("punk-"));
        assert_eq!(pack.decision, AuditDecision::AutoOk);
        assert_eq!(pack.release_verdict, ReleaseVerdict::Promote);
        assert_eq!(pack.checks.scope.status, "present");
        assert_eq!(pack.checks.mechanic.status, "missing");
        assert_eq!(pack.checks.holdouts.passed, 3);
    }

    #[test]
    fn save_and_read_proofpack() {
        let tmp = TempDir::new().unwrap();
        let audit = make_audit();
        let pack = assemble(tmp.path(), "{}", "{}", "diff", &audit, 2, 2);

        let path = save(&pack, tmp.path()).unwrap();
        assert!(path.exists());

        let raw = std::fs::read_to_string(&path).unwrap();
        let back: Proofpack = serde_json::from_str(&raw).unwrap();
        assert_eq!(back.run_id, pack.run_id);
        assert_eq!(back.decision, AuditDecision::AutoOk);
    }

    #[test]
    fn ci_gate_promote() {
        let tmp = TempDir::new().unwrap();
        let audit = make_audit();
        let pack = assemble(tmp.path(), "{}", "{}", "diff", &audit, 2, 2);
        let path = save(&pack, tmp.path()).unwrap();

        let (code, summary) = ci_gate(&path).unwrap();
        assert_eq!(code, CI_PROMOTE);
        assert!(summary.contains("PROMOTE"));
    }

    #[test]
    fn ci_gate_reject() {
        let tmp = TempDir::new().unwrap();
        let mut audit = make_audit();
        audit.decision = AuditDecision::AutoBlock;
        audit.release_verdict = ReleaseVerdict::Reject;
        let pack = assemble(tmp.path(), "{}", "{}", "diff", &audit, 0, 0);
        let path = save(&pack, tmp.path()).unwrap();

        let (code, _) = ci_gate(&path).unwrap();
        assert_eq!(code, CI_REJECT);
    }

    #[test]
    fn ci_gate_hold() {
        let tmp = TempDir::new().unwrap();
        let mut audit = make_audit();
        audit.decision = AuditDecision::HumanReview;
        audit.release_verdict = ReleaseVerdict::Hold;
        let pack = assemble(tmp.path(), "{}", "{}", "diff", &audit, 1, 0);
        let path = save(&pack, tmp.path()).unwrap();

        let (code, _) = ci_gate(&path).unwrap();
        assert_eq!(code, CI_HOLD);
    }

    #[test]
    fn proofpack_roundtrip() {
        let audit = make_audit();
        let pack = assemble(Path::new("/tmp"), "{}", "{}", "diff", &audit, 5, 4);
        let json = serde_json::to_string_pretty(&pack).unwrap();
        let back: Proofpack = serde_json::from_str(&json).unwrap();
        assert_eq!(back.checks.holdouts.total, 5);
        assert_eq!(back.checks.holdouts.passed, 4);
        assert_eq!(back.contract_full_sha256.len(), 64);
    }

    #[test]
    fn render_output() {
        let audit = make_audit();
        let pack = assemble(Path::new("/tmp"), "{}", "{}", "diff", &audit, 3, 3);
        let out = render_pack_short(&pack);
        assert!(out.contains("AutoOk"));
        assert!(out.contains("Promote"));
        assert!(out.contains("3/3 passed"));
    }
}
