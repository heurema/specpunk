use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::plan::contract::Contract;
use crate::plan::sha256_hex;
use crate::vcs::{self, VcsType};

/// punk version embedded in receipts.
const PUNK_VERSION: &str = "0.1.0";
const RECEIPT_SCHEMA: &str = "0.1.0";

// ---------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------

pub const EXIT_PASS: i32 = 0;
pub const EXIT_VIOLATION: i32 = 1;
pub const EXIT_NO_CONTRACT: i32 = 2;
pub const EXIT_NOT_APPROVED: i32 = 3;
pub const EXIT_INTERNAL: i32 = 4;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViolationType {
    /// File is in contract's dont_touch list.
    DontTouch,
    /// File is in project-level never_touch (from scan.json or config).
    NeverTouch,
    /// File not declared in contract scope.touch.
    Undeclared,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeViolation {
    pub file: String,
    pub violation_type: ViolationType,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeResult {
    pub declared_files: Vec<String>,
    pub actual_files: Vec<String>,
    pub undeclared_files: Vec<String>,
    pub violations: Vec<ScopeViolation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcsInfo {
    #[serde(rename = "type")]
    pub vcs_type: String,
    pub change_id: String,
}

/// Check receipt — always written (pass or fail).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckReceipt {
    pub schema_version: String,
    #[serde(rename = "type")]
    pub receipt_type: String,
    pub contract_id: String,
    pub contract_hash: String,
    pub timestamp: String,
    pub status: CheckStatus,
    pub scope: ScopeResult,
    pub vcs: VcsInfo,
    pub duration_ms: u64,
    pub punk_version: String,
}

// ---------------------------------------------------------------------------
// Options & errors
// ---------------------------------------------------------------------------

pub struct CheckOptions<'a> {
    pub root: &'a Path,
    pub strict: bool,
    pub json: bool,
}

#[derive(Debug)]
pub enum CheckError {
    NoContract(String),
    NotApproved(String),
    Io(std::io::Error),
    Vcs(vcs::VcsError),
    Parse(String),
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckError::NoContract(msg) => write!(f, "no contract: {msg}"),
            CheckError::NotApproved(msg) => write!(f, "contract not approved: {msg}"),
            CheckError::Io(e) => write!(f, "I/O error: {e}"),
            CheckError::Vcs(e) => write!(f, "VCS error: {e}"),
            CheckError::Parse(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for CheckError {}

impl From<std::io::Error> for CheckError {
    fn from(e: std::io::Error) -> Self {
        CheckError::Io(e)
    }
}

impl From<vcs::VcsError> for CheckError {
    fn from(e: vcs::VcsError) -> Self {
        CheckError::Vcs(e)
    }
}

// ---------------------------------------------------------------------------
// Contract resolution
// ---------------------------------------------------------------------------

/// Verify that the approval_hash in a contract matches the canonical hash.
/// The canonical form is the contract serialized with approval_hash = None.
fn verify_approval_hash(contract: &Contract, raw: &str) -> Result<(), CheckError> {
    let stored_hash = match &contract.approval_hash {
        Some(h) => h.clone(),
        None => {
            return Err(CheckError::NotApproved(
                "contract has no approval_hash. Run `punk plan` and approve.".into(),
            ));
        }
    };

    // Recompute: deserialize, clear approval_hash, serialize, hash
    let mut canonical: Contract = serde_json::from_str(raw)
        .map_err(|e| CheckError::Parse(format!("re-parse for hash verify: {e}")))?;
    canonical.approval_hash = None;
    let canonical_json = serde_json::to_string_pretty(&canonical)
        .map_err(|e| CheckError::Parse(format!("canonical serialize: {e}")))?;
    let expected = sha256_hex(canonical_json.as_bytes());

    if stored_hash != expected {
        return Err(CheckError::NotApproved(format!(
            "approval_hash mismatch — contract was modified after approval. \
             Expected {expected}, found {stored_hash}. Re-run `punk plan` and approve."
        )));
    }

    Ok(())
}

/// Find the approved contract for the current VCS change unit.
/// Returns (contract, contract_dir, contract_json_bytes).
///
/// Contract resolution is strict: only the contract matching the current
/// VCS change_id is accepted. No cross-change fallback — a new commit/change
/// without its own contract returns EXIT_NO_CONTRACT.
pub fn resolve_contract(root: &Path) -> Result<(Contract, PathBuf, String), CheckError> {
    let punk_dir = root.join(".punk");
    if !punk_dir.join("contracts").exists() {
        return Err(CheckError::NoContract(
            "no .punk/contracts/ directory. Run `punk plan` first.".into(),
        ));
    }

    // Get current VCS change id
    let change_id = vcs::detect(root)
        .and_then(|v| v.change_id())
        .unwrap_or_else(|_| String::new());

    if change_id.is_empty() {
        return Err(CheckError::NoContract(
            "could not determine VCS change id. Are you in a git/jj repo?".into(),
        ));
    }

    // Exact match only — no cross-change fallback
    let contract_dir = punk_dir.join("contracts").join(&change_id);

    // Security: reject symlinked contract directories (arbitrary write defense)
    if contract_dir
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(CheckError::Parse(
            "contract directory is a symlink — refusing (possible path traversal)".into(),
        ));
    }

    let contract_path = contract_dir.join("contract.json");

    if !contract_path.exists() {
        return Err(CheckError::NoContract(format!(
            "no contract for change '{change_id}'. Run `punk plan` first."
        )));
    }

    let raw = std::fs::read_to_string(&contract_path)?;
    let contract: Contract = serde_json::from_str(&raw)
        .map_err(|e| CheckError::Parse(format!("contract.json: {e}")))?;

    // Verify approval: hash must be present AND match canonical form
    verify_approval_hash(&contract, &raw)?;

    Ok((contract, contract_dir, raw))
}

// ---------------------------------------------------------------------------
// Never-touch loading from scan.json
// ---------------------------------------------------------------------------

fn load_never_touch(root: &Path) -> Vec<String> {
    let scan_path = root.join(".punk").join("scan.json");
    if !scan_path.exists() {
        return Vec::new();
    }
    let Ok(raw) = std::fs::read_to_string(&scan_path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    v["never_touch"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Scope matching
// ---------------------------------------------------------------------------

/// Check if a file path matches a scope pattern.
/// Patterns:
///   "src/"       → matches any file under src/
///   "src/auth.rs" → exact match
///   "src"        → matches src and src/*
fn matches_pattern(file: &str, pattern: &str) -> bool {
    if pattern.ends_with('/') {
        // Directory prefix
        file.starts_with(pattern)
    } else if file == pattern {
        // Exact match
        true
    } else {
        // Also match as directory prefix: "src" matches "src/foo.rs"
        file.starts_with(&format!("{pattern}/"))
    }
}

/// Check if a file matches any pattern in a list.
fn matches_any(file: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| matches_pattern(file, p))
}

/// Classify a single changed file against the contract scope.
fn classify_file(
    file: &str,
    touch: &[String],
    dont_touch: &[String],
    never_touch: &[String],
) -> Option<ScopeViolation> {
    // never_touch is checked first (highest priority)
    if matches_any(file, never_touch) {
        return Some(ScopeViolation {
            file: file.to_string(),
            violation_type: ViolationType::NeverTouch,
            message: format!(
                "{file} is in project never_touch boundaries. This file must not be modified."
            ),
        });
    }

    // dont_touch from contract
    if matches_any(file, dont_touch) {
        return Some(ScopeViolation {
            file: file.to_string(),
            violation_type: ViolationType::DontTouch,
            message: format!(
                "{file} is in contract dont_touch scope. The approved plan excludes this file."
            ),
        });
    }

    // Check if in-scope
    if matches_any(file, touch) {
        return None; // OK
    }

    // Undeclared
    Some(ScopeViolation {
        file: file.to_string(),
        violation_type: ViolationType::Undeclared,
        message: format!(
            "{file} is not in contract scope. Either expand the plan or unstage this file."
        ),
    })
}

// ---------------------------------------------------------------------------
// Main check logic
// ---------------------------------------------------------------------------

/// Run the scope check. Returns (receipt, exit_code).
pub fn run_check(opts: &CheckOptions) -> Result<(CheckReceipt, i32), CheckError> {
    let start = std::time::Instant::now();

    // 1. Resolve contract
    let (contract, contract_dir, contract_raw) = resolve_contract(opts.root)?;
    let contract_hash = sha256_hex(contract_raw.as_bytes());

    // 2. Get changed files from VCS
    let vcs_box = vcs::detect(opts.root)?;
    let vcs_type = vcs_box.vcs_type();
    let change_id = vcs_box.change_id().unwrap_or_default();
    let mut changed_files = vcs_box.changed_files()?;

    // Also include untracked files — they are part of the working change
    // and could violate scope (e.g., new .env file)
    if let Ok(untracked) = vcs_box.untracked_files() {
        for f in untracked {
            if !changed_files.contains(&f) {
                changed_files.push(f);
            }
        }
    }

    // Filter out .punk/ own files — they are punk infrastructure, not user code
    changed_files.retain(|f| !f.starts_with(".punk/") && !f.starts_with(".punk\\"));

    // 3. Load never_touch from scan.json
    let never_touch = load_never_touch(opts.root);

    // 4. Classify each file
    let mut violations = Vec::new();
    let mut undeclared = Vec::new();

    for file in &changed_files {
        if let Some(v) = classify_file(file, &contract.scope.touch, &contract.scope.dont_touch, &never_touch) {
            if v.violation_type == ViolationType::Undeclared {
                undeclared.push(file.clone());
            }
            violations.push(v);
        }
    }

    // 5. Determine status and exit code
    let has_hard_violations = violations
        .iter()
        .any(|v| matches!(v.violation_type, ViolationType::DontTouch | ViolationType::NeverTouch));

    let has_undeclared = !undeclared.is_empty();

    let (status, exit_code) = if has_hard_violations || (has_undeclared && opts.strict) {
        (CheckStatus::Fail, EXIT_VIOLATION)
    } else {
        (CheckStatus::Pass, EXIT_PASS)
    };

    let duration_ms = start.elapsed().as_millis() as u64;

    // 6. Build receipt
    let receipt = CheckReceipt {
        schema_version: RECEIPT_SCHEMA.to_string(),
        receipt_type: "check".to_string(),
        contract_id: contract.change_id.clone(),
        contract_hash,
        timestamp: Utc::now().to_rfc3339(),
        status,
        scope: ScopeResult {
            declared_files: contract.scope.touch.clone(),
            actual_files: changed_files,
            undeclared_files: undeclared,
            violations,
        },
        vcs: VcsInfo {
            vcs_type: match vcs_type {
                VcsType::Jj => "jj".to_string(),
                VcsType::Git => "git".to_string(),
            },
            change_id,
        },
        duration_ms,
        punk_version: PUNK_VERSION.to_string(),
    };

    // 7. Write receipt atomically (always — pass or fail)
    let receipts_dir = contract_dir.join("receipts");
    std::fs::create_dir_all(&receipts_dir)?;
    let receipt_json = serde_json::to_string_pretty(&receipt)
        .map_err(|e| CheckError::Parse(format!("receipt serialize: {e}")))?;
    let target = receipts_dir.join("check.json");
    let mut tmp = tempfile::NamedTempFile::new_in(&receipts_dir)?;
    std::io::Write::write_all(&mut tmp, receipt_json.as_bytes())?;
    tmp.persist(&target).map_err(|e| CheckError::Io(e.error))?;

    Ok((receipt, exit_code))
}

// ---------------------------------------------------------------------------
// Human-readable output (Rust-style error messages)
// ---------------------------------------------------------------------------

/// Render check result as human-readable output.
pub fn render_check(receipt: &CheckReceipt, strict: bool) -> String {
    let mut out = String::new();

    let scope = &receipt.scope;
    let total_files = scope.actual_files.len();
    let in_scope = total_files - scope.undeclared_files.len()
        - scope.violations.iter()
            .filter(|v| !matches!(v.violation_type, ViolationType::Undeclared))
            .count();

    out.push_str(&format!(
        "punk check: {} ({} files checked, {} in scope)\n",
        match receipt.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Fail => "FAIL",
        },
        total_files,
        in_scope,
    ));

    // Hard violations first
    let hard: Vec<_> = scope.violations.iter()
        .filter(|v| matches!(v.violation_type, ViolationType::DontTouch | ViolationType::NeverTouch))
        .collect();

    for v in &hard {
        let label = match v.violation_type {
            ViolationType::NeverTouch => "NEVER_TOUCH",
            ViolationType::DontTouch => "DONT_TOUCH",
            ViolationType::Undeclared => unreachable!(),
        };
        out.push_str(&format!("\n  error[{label}]: {}\n", v.file));
        out.push_str(&format!("    {}\n", v.message));
        out.push_str("    fix: unstage this file or abandon the contract\n");
    }

    // Undeclared files
    let undeclared: Vec<_> = scope.violations.iter()
        .filter(|v| matches!(v.violation_type, ViolationType::Undeclared))
        .collect();

    if !undeclared.is_empty() {
        let severity = if strict { "error" } else { "warning" };
        for v in &undeclared {
            out.push_str(&format!("\n  {severity}[UNDECLARED]: {}\n", v.file));
            out.push_str(&format!("    {}\n", v.message));
            out.push_str("    fix: `punk plan --expand` or `git restore --staged`\n");
        }
    }

    out.push_str(&format!(
        "\n  contract: {}\n  receipt:  .punk/contracts/{}/receipts/check.json\n  time:     {}ms\n",
        receipt.contract_id, receipt.contract_id, receipt.duration_ms
    ));

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    use crate::plan::ceremony::{CeremonyLevel, ModelTier};
    use crate::plan::contract::{
        AcceptanceCriterion, Contract, Feedback, FeedbackOutcome, RoutingMetadata, Scope,
        CONTRACT_VERSION,
    };
    use crate::plan::save_contract;

    fn make_approved_contract(punk_dir: &Path, change_id: &str, touch: Vec<&str>, dont_touch: Vec<&str>) {
        let mut contract = Contract {
            version: CONTRACT_VERSION.to_string(),
            goal: "test task".to_string(),
            scope: Scope {
                touch: touch.into_iter().map(|s| s.to_string()).collect(),
                dont_touch: dont_touch.into_iter().map(|s| s.to_string()).collect(),
            },
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "AC-01".to_string(),
                description: "tests pass".to_string(),
                verify: Some("cargo test".to_string()),
            }],
            assumptions: vec![],
            warnings: vec![],
            ceremony_level: CeremonyLevel::Skip,
            created_at: "2026-03-24T00:00:00Z".to_string(),
            change_id: change_id.to_string(),
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
            task_id: "test-task-id".to_string(),
            attempt_number: 1,
        };

        let feedback = Feedback {
            outcome: FeedbackOutcome::Approve,
            timestamp: "2026-03-24T00:00:00Z".to_string(),
            note: None,
        };

        save_contract(punk_dir, &mut contract, &feedback).unwrap();
    }

    #[test]
    fn matches_pattern_exact() {
        assert!(matches_pattern("src/auth.rs", "src/auth.rs"));
        assert!(!matches_pattern("src/auth.rs", "src/other.rs"));
    }

    #[test]
    fn matches_pattern_dir_prefix() {
        assert!(matches_pattern("src/auth.rs", "src/"));
        assert!(matches_pattern("src/nested/deep.rs", "src/"));
        assert!(!matches_pattern("tests/foo.rs", "src/"));
    }

    #[test]
    fn matches_pattern_implicit_dir() {
        // "src" without trailing slash matches "src/foo.rs"
        assert!(matches_pattern("src/auth.rs", "src"));
        assert!(!matches_pattern("src2/auth.rs", "src"));
    }

    #[test]
    fn classify_in_scope() {
        let result = classify_file(
            "src/auth.rs",
            &["src/auth.rs".to_string()],
            &[],
            &[],
        );
        assert!(result.is_none(), "in-scope file should have no violation");
    }

    #[test]
    fn classify_dont_touch() {
        let result = classify_file(
            "migrations/001.sql",
            &["src/".to_string()],
            &["migrations/".to_string()],
            &[],
        );
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v.violation_type, ViolationType::DontTouch);
    }

    #[test]
    fn classify_never_touch() {
        let result = classify_file(
            ".env",
            &["src/".to_string()],
            &[],
            &[".env".to_string()],
        );
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v.violation_type, ViolationType::NeverTouch);
    }

    #[test]
    fn classify_undeclared() {
        let result = classify_file(
            "README.md",
            &["src/".to_string()],
            &[],
            &[],
        );
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v.violation_type, ViolationType::Undeclared);
    }

    #[test]
    fn never_touch_takes_priority() {
        // File matches both never_touch and touch — never_touch wins
        let result = classify_file(
            ".env",
            &[".env".to_string()],
            &[],
            &[".env".to_string()],
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().violation_type, ViolationType::NeverTouch);
    }

    #[test]
    fn no_contract_error() {
        let tmp = TempDir::new().unwrap();
        let result = resolve_contract(tmp.path());
        assert!(matches!(result, Err(CheckError::NoContract(_))));
    }

    #[test]
    fn verify_hash_missing() {
        // Contract without approval_hash → NotApproved
        let contract = Contract {
            version: CONTRACT_VERSION.to_string(),
            goal: "test".to_string(),
            scope: Scope { touch: vec![], dont_touch: vec![] },
            acceptance_criteria: vec![],
            assumptions: vec![],
            warnings: vec![],
            ceremony_level: CeremonyLevel::Skip,
            created_at: "2026-03-24T00:00:00Z".to_string(),
            change_id: "test".to_string(),
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
        };
        let raw = serde_json::to_string_pretty(&contract).unwrap();
        let result = verify_approval_hash(&contract, &raw);
        assert!(matches!(result, Err(CheckError::NotApproved(_))));
    }

    #[test]
    fn verify_hash_tampered() {
        // Approved contract, then tampered → hash mismatch
        let mut contract = Contract {
            version: CONTRACT_VERSION.to_string(),
            goal: "original goal".to_string(),
            scope: Scope { touch: vec!["src/".to_string()], dont_touch: vec![] },
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "AC-01".to_string(),
                description: "test".to_string(),
                verify: None,
            }],
            assumptions: vec![],
            warnings: vec![],
            ceremony_level: CeremonyLevel::Skip,
            created_at: "2026-03-24T00:00:00Z".to_string(),
            change_id: "test".to_string(),
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
        };

        // Compute valid hash
        let canonical = serde_json::to_string_pretty(&contract).unwrap();
        contract.approval_hash = Some(sha256_hex(canonical.as_bytes()));

        // Now tamper: change the goal
        contract.goal = "TAMPERED goal".to_string();
        let tampered_raw = serde_json::to_string_pretty(&contract).unwrap();

        let result = verify_approval_hash(&contract, &tampered_raw);
        assert!(matches!(result, Err(CheckError::NotApproved(_))),
            "tampered contract should fail hash verification");
    }

    #[test]
    fn verify_hash_valid() {
        // Properly approved contract → OK
        let mut contract = Contract {
            version: CONTRACT_VERSION.to_string(),
            goal: "good goal".to_string(),
            scope: Scope { touch: vec!["src/".to_string()], dont_touch: vec![] },
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "AC-01".to_string(),
                description: "test".to_string(),
                verify: None,
            }],
            assumptions: vec![],
            warnings: vec![],
            ceremony_level: CeremonyLevel::Skip,
            created_at: "2026-03-24T00:00:00Z".to_string(),
            change_id: "test".to_string(),
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
        };

        let canonical = serde_json::to_string_pretty(&contract).unwrap();
        contract.approval_hash = Some(sha256_hex(canonical.as_bytes()));
        let raw = serde_json::to_string_pretty(&contract).unwrap();

        let result = verify_approval_hash(&contract, &raw);
        assert!(result.is_ok(), "valid hash should pass: {:?}", result.err());
    }

    #[test]
    fn no_vcs_returns_no_contract() {
        // Without VCS, resolve_contract returns NoContract (not a panic)
        let tmp = TempDir::new().unwrap();
        let punk_dir = tmp.path().join(".punk").join("contracts");
        fs::create_dir_all(&punk_dir).unwrap();
        let result = resolve_contract(tmp.path());
        assert!(matches!(result, Err(CheckError::NoContract(_))));
    }

    #[test]
    fn resolve_without_vcs_returns_no_contract() {
        // A tempdir with an approved contract but no VCS → NoContract
        // (strict resolution: VCS change_id required)
        let tmp = TempDir::new().unwrap();
        let punk_dir = tmp.path().join(".punk");
        fs::create_dir_all(&punk_dir).unwrap();

        make_approved_contract(&punk_dir, "test-id", vec!["src/"], vec!["migrations/"]);

        let result = resolve_contract(tmp.path());
        assert!(
            matches!(result, Err(CheckError::NoContract(_))),
            "without VCS, resolve should return NoContract, got: {:?}",
            result
        );
    }

    #[test]
    fn receipt_always_written() {
        let tmp = TempDir::new().unwrap();
        let punk_dir = tmp.path().join(".punk");
        fs::create_dir_all(&punk_dir).unwrap();

        make_approved_contract(&punk_dir, "receipt-test", vec!["src/"], vec![]);

        let receipt_path = punk_dir
            .join("contracts")
            .join("receipt-test")
            .join("receipts")
            .join("check.json");

        // We can't run the full check without VCS, but verify the receipt dir structure
        let receipts_dir = punk_dir.join("contracts").join("receipt-test").join("receipts");
        fs::create_dir_all(&receipts_dir).unwrap();

        let receipt = CheckReceipt {
            schema_version: RECEIPT_SCHEMA.to_string(),
            receipt_type: "check".to_string(),
            contract_id: "receipt-test".to_string(),
            contract_hash: "abc".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            status: CheckStatus::Pass,
            scope: ScopeResult {
                declared_files: vec!["src/".to_string()],
                actual_files: vec!["src/lib.rs".to_string()],
                undeclared_files: vec![],
                violations: vec![],
            },
            vcs: VcsInfo {
                vcs_type: "git".to_string(),
                change_id: "receipt-test".to_string(),
            },
            duration_ms: 5,
            punk_version: PUNK_VERSION.to_string(),
        };

        let json = serde_json::to_string_pretty(&receipt).unwrap();
        fs::write(&receipt_path, &json).unwrap();

        assert!(receipt_path.exists());
        let read_back: CheckReceipt =
            serde_json::from_str(&fs::read_to_string(&receipt_path).unwrap()).unwrap();
        assert_eq!(read_back.status, CheckStatus::Pass);
        assert_eq!(read_back.contract_id, "receipt-test");
    }

    #[test]
    fn render_pass() {
        let receipt = CheckReceipt {
            schema_version: RECEIPT_SCHEMA.to_string(),
            receipt_type: "check".to_string(),
            contract_id: "abc123".to_string(),
            contract_hash: "hash".to_string(),
            timestamp: "2026-03-24T00:00:00Z".to_string(),
            status: CheckStatus::Pass,
            scope: ScopeResult {
                declared_files: vec!["src/".to_string()],
                actual_files: vec!["src/lib.rs".to_string()],
                undeclared_files: vec![],
                violations: vec![],
            },
            vcs: VcsInfo {
                vcs_type: "git".to_string(),
                change_id: "abc123".to_string(),
            },
            duration_ms: 3,
            punk_version: PUNK_VERSION.to_string(),
        };

        let output = render_check(&receipt, false);
        assert!(output.contains("PASS"));
        assert!(output.contains("1 files checked"));
    }

    #[test]
    fn render_hard_violation() {
        let receipt = CheckReceipt {
            schema_version: RECEIPT_SCHEMA.to_string(),
            receipt_type: "check".to_string(),
            contract_id: "abc123".to_string(),
            contract_hash: "hash".to_string(),
            timestamp: "2026-03-24T00:00:00Z".to_string(),
            status: CheckStatus::Fail,
            scope: ScopeResult {
                declared_files: vec!["src/".to_string()],
                actual_files: vec!["src/lib.rs".to_string(), ".env".to_string()],
                undeclared_files: vec![],
                violations: vec![ScopeViolation {
                    file: ".env".to_string(),
                    violation_type: ViolationType::NeverTouch,
                    message: ".env is in project never_touch boundaries.".to_string(),
                }],
            },
            vcs: VcsInfo {
                vcs_type: "git".to_string(),
                change_id: "abc123".to_string(),
            },
            duration_ms: 5,
            punk_version: PUNK_VERSION.to_string(),
        };

        let output = render_check(&receipt, false);
        assert!(output.contains("FAIL"));
        assert!(output.contains("NEVER_TOUCH"));
        assert!(output.contains(".env"));
    }

    #[test]
    fn render_undeclared_warning_vs_strict() {
        let receipt = CheckReceipt {
            schema_version: RECEIPT_SCHEMA.to_string(),
            receipt_type: "check".to_string(),
            contract_id: "abc123".to_string(),
            contract_hash: "hash".to_string(),
            timestamp: "2026-03-24T00:00:00Z".to_string(),
            status: CheckStatus::Pass,
            scope: ScopeResult {
                declared_files: vec!["src/".to_string()],
                actual_files: vec!["src/lib.rs".to_string(), "README.md".to_string()],
                undeclared_files: vec!["README.md".to_string()],
                violations: vec![ScopeViolation {
                    file: "README.md".to_string(),
                    violation_type: ViolationType::Undeclared,
                    message: "not in scope".to_string(),
                }],
            },
            vcs: VcsInfo {
                vcs_type: "git".to_string(),
                change_id: "abc123".to_string(),
            },
            duration_ms: 2,
            punk_version: PUNK_VERSION.to_string(),
        };

        // Default mode: warning
        let output = render_check(&receipt, false);
        assert!(output.contains("warning[UNDECLARED]"));

        // Strict mode: error
        let output_strict = render_check(&receipt, true);
        assert!(output_strict.contains("error[UNDECLARED]"));
    }

    #[test]
    fn check_receipt_roundtrip() {
        let receipt = CheckReceipt {
            schema_version: RECEIPT_SCHEMA.to_string(),
            receipt_type: "check".to_string(),
            contract_id: "test".to_string(),
            contract_hash: "abc".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            status: CheckStatus::Fail,
            scope: ScopeResult {
                declared_files: vec!["src/".to_string()],
                actual_files: vec!["src/lib.rs".to_string(), ".env".to_string()],
                undeclared_files: vec![],
                violations: vec![ScopeViolation {
                    file: ".env".to_string(),
                    violation_type: ViolationType::NeverTouch,
                    message: "forbidden".to_string(),
                }],
            },
            vcs: VcsInfo {
                vcs_type: "git".to_string(),
                change_id: "test".to_string(),
            },
            duration_ms: 10,
            punk_version: PUNK_VERSION.to_string(),
        };

        let json = serde_json::to_string_pretty(&receipt).unwrap();
        let back: CheckReceipt = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, CheckStatus::Fail);
        assert_eq!(back.scope.violations.len(), 1);
        assert_eq!(back.scope.violations[0].violation_type, ViolationType::NeverTouch);
    }
}
