use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::check::{CheckReceipt, CheckStatus};
use crate::plan::sha256_hex;
use crate::vcs;

const PUNK_VERSION: &str = "0.1.0";
const RECEIPT_SCHEMA: &str = "0.1.0";

// ---------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------

pub const EXIT_OK: i32 = 0;
pub const EXIT_NO_CHECK: i32 = 1;
pub const EXIT_CHECK_FAILED: i32 = 2;
pub const EXIT_INTERNAL: i32 = 3;

// ---------------------------------------------------------------------------
// Task receipt
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    Completed,
    Failed,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSummary {
    pub files_created: Vec<String>,
    pub files_modified: Vec<String>,
    pub files_deleted: Vec<String>,
    pub scope_violations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskReceipt {
    pub schema_version: String,
    #[serde(rename = "type")]
    pub receipt_type: String,
    pub contract_id: String,
    pub contract_hash: String,
    pub timestamp: String,
    pub status: TaskStatus,
    /// SHA-256 of check.json — receipt chain link.
    pub check_receipt_hash: String,
    pub summary: FileSummary,
    pub punk_version: String,
}

// ---------------------------------------------------------------------------
// Options & errors
// ---------------------------------------------------------------------------

pub struct ReceiptOptions<'a> {
    pub root: &'a Path,
    pub json: bool,
    pub md: bool,
}

#[derive(Debug)]
pub enum ReceiptError {
    NoContract(String),
    NoCheckReceipt(String),
    CheckFailed(String),
    Io(std::io::Error),
    Parse(String),
    Vcs(vcs::VcsError),
}

impl std::fmt::Display for ReceiptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReceiptError::NoContract(m) => write!(f, "no contract: {m}"),
            ReceiptError::NoCheckReceipt(m) => write!(f, "no check receipt: {m}"),
            ReceiptError::CheckFailed(m) => write!(f, "check did not pass: {m}"),
            ReceiptError::Io(e) => write!(f, "I/O error: {e}"),
            ReceiptError::Parse(m) => write!(f, "parse error: {m}"),
            ReceiptError::Vcs(e) => write!(f, "VCS error: {e}"),
        }
    }
}

impl std::error::Error for ReceiptError {}

impl From<std::io::Error> for ReceiptError {
    fn from(e: std::io::Error) -> Self {
        ReceiptError::Io(e)
    }
}

impl From<vcs::VcsError> for ReceiptError {
    fn from(e: vcs::VcsError) -> Self {
        ReceiptError::Vcs(e)
    }
}

// ---------------------------------------------------------------------------
// File diff summary
// ---------------------------------------------------------------------------

/// Classify changed files into created/modified/deleted.
/// Uses `git cat-file -e HEAD:<file>` to check if file existed in the last commit.
fn build_file_summary(root: &Path, changed_files: &[String], scope_violations: usize) -> FileSummary {
    let mut created = Vec::new();
    let mut modified = Vec::new();
    let mut deleted = Vec::new();

    for file in changed_files {
        let path = root.join(file);
        if !path.exists() {
            deleted.push(file.clone());
        } else {
            // Check if file existed in HEAD (committed state)
            let existed_in_head = std::process::Command::new("git")
                .args(["cat-file", "-e", &format!("HEAD:{file}")])
                .current_dir(root)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            if existed_in_head {
                modified.push(file.clone());
            } else {
                created.push(file.clone());
            }
        }
    }

    FileSummary {
        files_created: created,
        files_modified: modified,
        files_deleted: deleted,
        scope_violations,
    }
}

// ---------------------------------------------------------------------------
// Main receipt logic
// ---------------------------------------------------------------------------

/// Generate a task receipt. Requires a passing check receipt.
pub fn run_receipt(opts: &ReceiptOptions) -> Result<(TaskReceipt, i32), ReceiptError> {
    // 1. Resolve contract (reuse check's resolution)
    let (contract, contract_dir, contract_raw) =
        crate::check::resolve_contract(opts.root).map_err(|e| match e {
            crate::check::CheckError::NoContract(m) => ReceiptError::NoContract(m),
            crate::check::CheckError::NotApproved(m) => ReceiptError::NoContract(m),
            crate::check::CheckError::Io(e) => ReceiptError::Io(e),
            crate::check::CheckError::Vcs(e) => ReceiptError::Vcs(e),
            crate::check::CheckError::Parse(m) => ReceiptError::Parse(m),
        })?;

    let contract_hash = sha256_hex(contract_raw.as_bytes());

    // 2. Load check receipt — must exist and be PASS
    let check_path = contract_dir.join("receipts").join("check.json");
    if !check_path.exists() {
        return Err(ReceiptError::NoCheckReceipt(
            "no check receipt found. Run `punk check` first.".into(),
        ));
    }

    let check_raw = std::fs::read_to_string(&check_path)?;
    let check_receipt: CheckReceipt = serde_json::from_str(&check_raw)
        .map_err(|e| ReceiptError::Parse(format!("check.json: {e}")))?;

    if check_receipt.status != CheckStatus::Pass {
        return Err(ReceiptError::CheckFailed(format!(
            "check status is {:?}, not PASS. Fix violations and re-run `punk check`.",
            check_receipt.status
        )));
    }

    let check_receipt_hash = sha256_hex(check_raw.as_bytes());

    // 3. Build file summary from check receipt data
    let scope_violations = check_receipt.scope.violations.len();
    let summary = build_file_summary(
        opts.root,
        &check_receipt.scope.actual_files,
        scope_violations,
    );

    // 4. Build task receipt
    let receipt = TaskReceipt {
        schema_version: RECEIPT_SCHEMA.to_string(),
        receipt_type: "task".to_string(),
        contract_id: contract.change_id.clone(),
        contract_hash,
        timestamp: Utc::now().to_rfc3339(),
        status: TaskStatus::Completed,
        check_receipt_hash,
        summary,
        punk_version: PUNK_VERSION.to_string(),
    };

    // 5. Write task receipt atomically
    let receipts_dir = contract_dir.join("receipts");
    std::fs::create_dir_all(&receipts_dir)?;
    let receipt_json = serde_json::to_string_pretty(&receipt)
        .map_err(|e| ReceiptError::Parse(format!("task receipt serialize: {e}")))?;
    let target = receipts_dir.join("task.json");
    let mut tmp = tempfile::NamedTempFile::new_in(&receipts_dir)?;
    std::io::Write::write_all(&mut tmp, receipt_json.as_bytes())?;
    tmp.persist(&target).map_err(|e| ReceiptError::Io(e.error))?;

    Ok((receipt, EXIT_OK))
}

// ---------------------------------------------------------------------------
// Markdown renderer
// ---------------------------------------------------------------------------

pub fn render_receipt_md(receipt: &TaskReceipt) -> String {
    let mut out = String::new();

    out.push_str(&format!("# Task Receipt: {}\n\n", receipt.contract_id));
    out.push_str(&format!("**Status:** {}\n", match receipt.status {
        TaskStatus::Completed => "COMPLETED",
        TaskStatus::Failed => "FAILED",
        TaskStatus::Abandoned => "ABANDONED",
    }));
    out.push_str(&format!("**Timestamp:** {}\n", receipt.timestamp));
    out.push_str(&format!("**Contract hash:** `{}`\n", &receipt.contract_hash[..16]));
    out.push_str(&format!("**Check receipt hash:** `{}`\n\n", &receipt.check_receipt_hash[..16]));

    out.push_str("## Files\n\n");
    let s = &receipt.summary;
    if !s.files_created.is_empty() {
        out.push_str(&format!("**Created ({}):**\n", s.files_created.len()));
        for f in &s.files_created {
            out.push_str(&format!("  + {f}\n"));
        }
    }
    if !s.files_modified.is_empty() {
        out.push_str(&format!("**Modified ({}):**\n", s.files_modified.len()));
        for f in &s.files_modified {
            out.push_str(&format!("  ~ {f}\n"));
        }
    }
    if !s.files_deleted.is_empty() {
        out.push_str(&format!("**Deleted ({}):**\n", s.files_deleted.len()));
        for f in &s.files_deleted {
            out.push_str(&format!("  - {f}\n"));
        }
    }
    if s.files_created.is_empty() && s.files_modified.is_empty() && s.files_deleted.is_empty() {
        out.push_str("  (no file changes)\n");
    }

    out.push_str(&format!("\n**Scope violations:** {}\n", s.scope_violations));
    out.push_str(&format!("**punk version:** {}\n", receipt.punk_version));

    out
}

/// Render check-style one-liner for terminal.
pub fn render_receipt_short(receipt: &TaskReceipt) -> String {
    let s = &receipt.summary;
    let total = s.files_created.len() + s.files_modified.len() + s.files_deleted.len();
    format!(
        "punk receipt: {} ({} files: +{} ~{} -{}, {} violations)\n  contract: {}\n  receipt:  .punk/contracts/{}/receipts/task.json\n",
        match receipt.status {
            TaskStatus::Completed => "COMPLETED",
            TaskStatus::Failed => "FAILED",
            TaskStatus::Abandoned => "ABANDONED",
        },
        total,
        s.files_created.len(),
        s.files_modified.len(),
        s.files_deleted.len(),
        s.scope_violations,
        receipt.contract_id,
        receipt.contract_id,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn task_receipt_roundtrip() {
        let receipt = TaskReceipt {
            schema_version: RECEIPT_SCHEMA.to_string(),
            receipt_type: "task".to_string(),
            contract_id: "abc123".to_string(),
            contract_hash: "deadbeef".repeat(4),
            timestamp: Utc::now().to_rfc3339(),
            status: TaskStatus::Completed,
            check_receipt_hash: "cafebabe".repeat(4),
            summary: FileSummary {
                files_created: vec!["src/new.rs".to_string()],
                files_modified: vec!["src/lib.rs".to_string()],
                files_deleted: vec![],
                scope_violations: 0,
            },
            punk_version: PUNK_VERSION.to_string(),
        };

        let json = serde_json::to_string_pretty(&receipt).unwrap();
        let back: TaskReceipt = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, TaskStatus::Completed);
        assert_eq!(back.summary.files_created.len(), 1);
        assert_eq!(back.summary.files_modified.len(), 1);
        assert_eq!(back.check_receipt_hash, "cafebabe".repeat(4));
    }

    #[test]
    fn render_md_output() {
        let receipt = TaskReceipt {
            schema_version: RECEIPT_SCHEMA.to_string(),
            receipt_type: "task".to_string(),
            contract_id: "test123".to_string(),
            contract_hash: "a".repeat(64),
            timestamp: "2026-03-25T00:00:00Z".to_string(),
            status: TaskStatus::Completed,
            check_receipt_hash: "b".repeat(64),
            summary: FileSummary {
                files_created: vec!["src/new.rs".to_string()],
                files_modified: vec!["src/lib.rs".to_string()],
                files_deleted: vec!["old.rs".to_string()],
                scope_violations: 0,
            },
            punk_version: PUNK_VERSION.to_string(),
        };

        let md = render_receipt_md(&receipt);
        assert!(md.contains("# Task Receipt: test123"));
        assert!(md.contains("COMPLETED"));
        assert!(md.contains("+ src/new.rs"));
        assert!(md.contains("~ src/lib.rs"));
        assert!(md.contains("- old.rs"));
    }

    #[test]
    fn render_short_output() {
        let receipt = TaskReceipt {
            schema_version: RECEIPT_SCHEMA.to_string(),
            receipt_type: "task".to_string(),
            contract_id: "abc".to_string(),
            contract_hash: "x".repeat(64),
            timestamp: "2026-03-25T00:00:00Z".to_string(),
            status: TaskStatus::Completed,
            check_receipt_hash: "y".repeat(64),
            summary: FileSummary {
                files_created: vec!["a.rs".to_string()],
                files_modified: vec!["b.rs".to_string(), "c.rs".to_string()],
                files_deleted: vec![],
                scope_violations: 1,
            },
            punk_version: PUNK_VERSION.to_string(),
        };

        let short = render_receipt_short(&receipt);
        assert!(short.contains("COMPLETED"));
        assert!(short.contains("3 files"));
        assert!(short.contains("+1 ~2 -0"));
        assert!(short.contains("1 violations"));
    }

    #[test]
    fn no_check_receipt_error() {
        let tmp = TempDir::new().unwrap();
        let opts = ReceiptOptions {
            root: tmp.path(),
            json: false,
            md: false,
        };
        let result = run_receipt(&opts);
        assert!(result.is_err());
    }

    #[test]
    fn task_status_variants() {
        for (status, expected) in [
            (TaskStatus::Completed, "\"COMPLETED\""),
            (TaskStatus::Failed, "\"FAILED\""),
            (TaskStatus::Abandoned, "\"ABANDONED\""),
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected);
        }
    }
}
