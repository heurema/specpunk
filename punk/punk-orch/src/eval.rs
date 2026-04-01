use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::receipt::{Receipt, ReceiptStatus};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GateOutcome {
    Accept,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskEvalMetrics {
    pub contract_satisfaction: f64,
    pub scope_discipline: f64,
    pub target_pass_rate: f64,
    pub integrity_pass_rate: f64,
    pub cleanup_completion: f64,
    pub docs_parity: f64,
    pub drift_penalty: f64,
    pub gate_outcome: GateOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskEvalRecord {
    pub task_id: String,
    pub project_id: String,
    pub receipt_status: ReceiptStatus,
    pub gate_outcome: GateOutcome,
    pub metrics: TaskEvalMetrics,
    pub notes: Vec<String>,
    pub overall_score: f64,
    pub created_at: DateTime<Utc>,
    pub receipt_created_at: DateTime<Utc>,
}

fn detect_repo_root(cwd: &Path) -> Result<PathBuf, String> {
    for (bin, args) in [
        ("jj", vec!["root"]),
        ("git", vec!["rev-parse", "--show-toplevel"]),
    ] {
        let output = std::process::Command::new(bin)
            .args(&args)
            .current_dir(cwd)
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !root.is_empty() {
                    let root_path = PathBuf::from(root);
                    return Ok(root_path.canonicalize().unwrap_or(root_path));
                }
            }
        }
    }
    Err("eval requires running inside a Git/jj repository".to_string())
}

fn eval_results_dir(project_root: &Path) -> PathBuf {
    project_root.join(".punk/eval/results")
}

fn latest_receipt_for_task(bus: &Path, task_id: &str) -> Option<Receipt> {
    let mut indices = vec![bus.join("receipts/index.jsonl")];
    if let Some(parent) = bus.parent() {
        indices.push(parent.join("receipts/index.jsonl"));
    }

    for index in indices {
        let Some(content) = fs::read_to_string(index).ok() else {
            continue;
        };
        if let Some(receipt) = content
            .lines()
            .rev()
            .filter_map(|line| serde_json::from_str::<Receipt>(line).ok())
            .find(|receipt| receipt.task_id == task_id)
        {
            return Some(receipt);
        }
    }

    None
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

fn score_receipt(receipt: &Receipt) -> (TaskEvalMetrics, Vec<String>, f64) {
    let mut notes = Vec::new();
    let mut text = receipt.summary.to_ascii_lowercase();
    if !receipt.errors.is_empty() {
        text.push(' ');
        text.push_str(&receipt.errors.join(" ").to_ascii_lowercase());
    }
    if !receipt.artifacts.is_empty() {
        text.push(' ');
        text.push_str(&receipt.artifacts.join(" ").to_ascii_lowercase());
    }

    let success = matches!(receipt.status, ReceiptStatus::Success) && receipt.exit_code == 0;
    let gate_outcome = if success {
        GateOutcome::Accept
    } else {
        GateOutcome::Reject
    };

    let scope_bad = contains_any(
        &text,
        &[
            "out of scope",
            "outside allowed scope",
            "allowed scope",
            "scope drift",
        ],
    );
    let integrity_bad = contains_any(
        &text,
        &[
            "git checkout",
            "git restore",
            "git reset",
            "git clean",
            "private key",
            "token leak",
            "credential leak",
            "secret leak",
        ],
    );
    let cleanup_bad = contains_any(
        &text,
        &[
            "cleanup incomplete",
            "leftover",
            "temporary file",
            "stale file",
            "follow-up required",
            "todo left",
        ],
    );
    let docs_bad = contains_any(
        &text,
        &[
            "stale docs",
            "docs mismatch",
            "readme outdated",
            "documentation outdated",
            "docs parity",
        ],
    );
    let drift_bad = contains_any(
        &text,
        &[
            "drift",
            "leftover v1",
            "v1 path",
            "old path",
            "stale docs",
            "superseded",
        ],
    );

    if scope_bad {
        notes.push("scope discipline regression detected".to_string());
    }
    if integrity_bad {
        notes.push("integrity regression detected".to_string());
    }
    if cleanup_bad {
        notes.push("cleanup appears incomplete".to_string());
    }
    if docs_bad {
        notes.push("docs parity issue detected".to_string());
    }
    if drift_bad {
        notes.push("drift signal detected".to_string());
    }

    let contract_satisfaction = if success {
        1.0
    } else if matches!(
        receipt.status,
        ReceiptStatus::Cancelled | ReceiptStatus::Timeout
    ) {
        0.0
    } else {
        0.25
    };
    let target_pass_rate = if success {
        1.0
    } else if receipt.punk_check_exit == Some(0) {
        0.5
    } else {
        0.0
    };
    let scope_discipline = if scope_bad { 0.0 } else { 1.0 };
    let integrity_pass_rate = if integrity_bad { 0.0 } else { 1.0 };
    let cleanup_completion = if cleanup_bad {
        0.0
    } else if success {
        1.0
    } else {
        0.5
    };
    let docs_parity = if docs_bad { 0.0 } else { 1.0 };
    let drift_penalty = if drift_bad { 1.0 } else { 0.0 };

    let overall_score = clamp01(
        (contract_satisfaction
            + scope_discipline
            + target_pass_rate
            + integrity_pass_rate
            + cleanup_completion
            + docs_parity)
            / 6.0
            - (drift_penalty * 0.25),
    );

    (
        TaskEvalMetrics {
            contract_satisfaction,
            scope_discipline,
            target_pass_rate,
            integrity_pass_rate,
            cleanup_completion,
            docs_parity,
            drift_penalty,
            gate_outcome: gate_outcome.clone(),
        },
        notes,
        overall_score,
    )
}

pub fn evaluate_task(cwd: &Path, bus: &Path, task_id: &str) -> Result<TaskEvalRecord, String> {
    let receipt = latest_receipt_for_task(bus, task_id)
        .ok_or_else(|| format!("receipt not found for task: {task_id}"))?;
    let (metrics, notes, overall_score) = score_receipt(&receipt);
    let record = TaskEvalRecord {
        task_id: receipt.task_id.clone(),
        project_id: receipt.project.clone(),
        receipt_status: receipt.status.clone(),
        gate_outcome: metrics.gate_outcome.clone(),
        metrics,
        notes,
        overall_score,
        created_at: Utc::now(),
        receipt_created_at: receipt.created_at,
    };

    let repo_root = detect_repo_root(cwd)?;
    let dir = eval_results_dir(&repo_root);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.json", record.task_id));
    fs::write(
        &path,
        serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    Ok(record)
}

pub fn list_task_evals(cwd: &Path) -> Result<Vec<TaskEvalRecord>, String> {
    let repo_root = detect_repo_root(cwd)?;
    let dir = eval_results_dir(&repo_root);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.extension().is_some_and(|ext| ext == "json") {
            continue;
        }
        let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let record = serde_json::from_str::<TaskEvalRecord>(&content).map_err(|e| e.to_string())?;
        records.push(record);
    }
    records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_repo(path: &Path) {
        std::process::Command::new("git")
            .arg("init")
            .arg(path)
            .output()
            .unwrap();
    }

    fn write_receipt_index(bus_root: &Path, receipt: &Receipt) {
        let receipts_dir = bus_root.join("receipts");
        fs::create_dir_all(&receipts_dir).unwrap();
        let line = serde_json::to_string(receipt).unwrap();
        fs::write(receipts_dir.join("index.jsonl"), format!("{line}\n")).unwrap();
    }

    fn sample_receipt(status: ReceiptStatus) -> Receipt {
        Receipt {
            schema_version: 1,
            task_id: "task-123".into(),
            status,
            agent: "claude".into(),
            model: "sonnet".into(),
            project: "specpunk".into(),
            category: "fix".into(),
            call_style: None,
            tokens_used: 0,
            cost_usd: 0.12,
            duration_ms: 2_000,
            exit_code: 0,
            artifacts: vec![],
            errors: vec![],
            summary: "clean success".into(),
            created_at: Utc::now(),
            parent_task_id: None,
            punk_check_exit: Some(0),
        }
    }

    #[test]
    fn evaluate_task_scores_strong_success() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();
        let receipt = sample_receipt(ReceiptStatus::Success);
        write_receipt_index(&bus, &receipt);

        let record = evaluate_task(&repo, &bus, "task-123").unwrap();
        assert_eq!(record.gate_outcome, GateOutcome::Accept);
        assert!(record.overall_score > 0.9);
        assert_eq!(record.metrics.contract_satisfaction, 1.0);
        assert_eq!(record.metrics.target_pass_rate, 1.0);
    }

    #[test]
    fn evaluate_task_penalizes_hidden_drift_signals() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();
        let mut receipt = sample_receipt(ReceiptStatus::Failure);
        receipt.exit_code = 1;
        receipt.punk_check_exit = Some(1);
        receipt.summary = "leftover v1 path and stale docs after failure".into();
        receipt.errors = vec!["out of scope edit detected".into()];
        write_receipt_index(&bus, &receipt);

        let record = evaluate_task(&repo, &bus, "task-123").unwrap();
        assert_eq!(record.gate_outcome, GateOutcome::Reject);
        assert_eq!(record.metrics.scope_discipline, 0.0);
        assert_eq!(record.metrics.docs_parity, 0.0);
        assert_eq!(record.metrics.drift_penalty, 1.0);
        assert!(record.overall_score < 0.5);
    }

    #[test]
    fn list_task_evals_returns_newest_first() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        let mut first = sample_receipt(ReceiptStatus::Success);
        first.task_id = "task-1".into();
        write_receipt_index(&bus, &first);
        let _ = evaluate_task(&repo, &bus, "task-1").unwrap();

        let mut second = sample_receipt(ReceiptStatus::Failure);
        second.task_id = "task-2".into();
        second.exit_code = 1;
        write_receipt_index(&bus, &second);
        let _ = evaluate_task(&repo, &bus, "task-2").unwrap();

        let listed = list_task_evals(&repo).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].task_id, "task-2");
        assert_eq!(listed[1].task_id, "task-1");
    }
}
