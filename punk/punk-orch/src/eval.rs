use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::receipt::{Receipt, ReceiptStatus};
use crate::skill::{self, SkillState};

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

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectEvalSummary {
    pub project_id: String,
    pub total: usize,
    pub accept_count: usize,
    pub reject_count: usize,
    pub avg_score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WeakTaskEval {
    pub task_id: String,
    pub project_id: String,
    pub overall_score: f64,
    pub gate_outcome: GateOutcome,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvalSummary {
    pub total: usize,
    pub accept_count: usize,
    pub reject_count: usize,
    pub avg_score: f64,
    pub avg_contract_satisfaction: f64,
    pub avg_scope_discipline: f64,
    pub avg_target_pass_rate: f64,
    pub avg_integrity_pass_rate: f64,
    pub avg_cleanup_completion: f64,
    pub avg_docs_parity: f64,
    pub avg_drift_penalty: f64,
    pub projects: Vec<ProjectEvalSummary>,
    pub weakest_tasks: Vec<WeakTaskEval>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromotionDecision {
    Promote,
    Reject,
    Rollback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillEvalPrimaryMetrics {
    pub contract_satisfaction: f64,
    pub target_pass_rate: f64,
    pub blocked_run_rate: f64,
    pub escalation_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillEvalSafetyMetrics {
    pub scope_discipline: f64,
    pub integrity_pass_rate: f64,
    pub cleanup_completion: f64,
    pub docs_parity: f64,
    pub drift_penalty: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillEvalMetricSet {
    pub primary: SkillEvalPrimaryMetrics,
    pub safety: SkillEvalSafetyMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillEvalRecord {
    pub eval_id: String,
    pub skill_name: String,
    pub project_id: String,
    pub suite_id: String,
    pub role: Option<String>,
    pub candidate_path: PathBuf,
    pub baseline: SkillEvalMetricSet,
    pub candidate: SkillEvalMetricSet,
    pub suite_size: usize,
    pub sufficient_suite: bool,
    pub safety_regressions: Vec<String>,
    pub primary_improvements: Vec<String>,
    pub primary_regressions: Vec<String>,
    pub baseline_primary_score: f64,
    pub candidate_primary_score: f64,
    pub decision: PromotionDecision,
    pub decision_reasons: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub notes: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvaluateSkillRequest {
    pub skill_name: String,
    pub project_id: String,
    pub suite_id: String,
    pub role: Option<String>,
    pub baseline: SkillEvalMetricSet,
    pub candidate: SkillEvalMetricSet,
    pub suite_size: usize,
    pub evidence_refs: Vec<String>,
    pub notes: Vec<String>,
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

fn skill_eval_results_dir(project_root: &Path) -> PathBuf {
    project_root.join(".punk/eval/skills")
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

fn skill_eval_id(skill_name: &str, suite_id: &str) -> String {
    format!(
        "{}-{}-{}",
        sanitize_component(skill_name),
        sanitize_component(suite_id),
        Utc::now().format("%Y%m%d%H%M%S")
    )
}

fn sanitize_component(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "item".to_string()
    } else {
        trimmed
    }
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

pub fn summarize_task_evals(
    cwd: &Path,
    limit: Option<usize>,
    project_filter: Option<&str>,
) -> Result<EvalSummary, String> {
    let mut records = list_task_evals(cwd)?;
    if let Some(project) = project_filter {
        records.retain(|record| record.project_id == project);
    }
    if let Some(limit) = limit {
        records.truncate(limit);
    }
    if records.is_empty() {
        return Err("no task evals found for summary".to_string());
    }

    let total = records.len();
    let accept_count = records
        .iter()
        .filter(|record| record.gate_outcome == GateOutcome::Accept)
        .count();
    let reject_count = total - accept_count;

    let avg_score = records
        .iter()
        .map(|record| record.overall_score)
        .sum::<f64>()
        / total as f64;
    let avg_contract_satisfaction = records
        .iter()
        .map(|record| record.metrics.contract_satisfaction)
        .sum::<f64>()
        / total as f64;
    let avg_scope_discipline = records
        .iter()
        .map(|record| record.metrics.scope_discipline)
        .sum::<f64>()
        / total as f64;
    let avg_target_pass_rate = records
        .iter()
        .map(|record| record.metrics.target_pass_rate)
        .sum::<f64>()
        / total as f64;
    let avg_integrity_pass_rate = records
        .iter()
        .map(|record| record.metrics.integrity_pass_rate)
        .sum::<f64>()
        / total as f64;
    let avg_cleanup_completion = records
        .iter()
        .map(|record| record.metrics.cleanup_completion)
        .sum::<f64>()
        / total as f64;
    let avg_docs_parity = records
        .iter()
        .map(|record| record.metrics.docs_parity)
        .sum::<f64>()
        / total as f64;
    let avg_drift_penalty = records
        .iter()
        .map(|record| record.metrics.drift_penalty)
        .sum::<f64>()
        / total as f64;

    let mut project_ids = records
        .iter()
        .map(|record| record.project_id.clone())
        .collect::<Vec<_>>();
    project_ids.sort();
    project_ids.dedup();

    let mut projects = Vec::new();
    for project_id in project_ids {
        let project_records = records
            .iter()
            .filter(|record| record.project_id == project_id)
            .collect::<Vec<_>>();
        let project_total = project_records.len();
        let project_accept_count = project_records
            .iter()
            .filter(|record| record.gate_outcome == GateOutcome::Accept)
            .count();
        let project_reject_count = project_total - project_accept_count;
        let project_avg_score = project_records
            .iter()
            .map(|record| record.overall_score)
            .sum::<f64>()
            / project_total as f64;
        projects.push(ProjectEvalSummary {
            project_id,
            total: project_total,
            accept_count: project_accept_count,
            reject_count: project_reject_count,
            avg_score: project_avg_score,
        });
    }
    projects.sort_by(|a, b| {
        a.avg_score
            .partial_cmp(&b.avg_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.project_id.cmp(&b.project_id))
    });

    let mut weakest_tasks = records
        .iter()
        .map(|record| WeakTaskEval {
            task_id: record.task_id.clone(),
            project_id: record.project_id.clone(),
            overall_score: record.overall_score,
            gate_outcome: record.gate_outcome.clone(),
            created_at: record.created_at,
        })
        .collect::<Vec<_>>();
    weakest_tasks.sort_by(|a, b| {
        a.overall_score
            .partial_cmp(&b.overall_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    weakest_tasks.truncate(3);

    Ok(EvalSummary {
        total,
        accept_count,
        reject_count,
        avg_score,
        avg_contract_satisfaction,
        avg_scope_discipline,
        avg_target_pass_rate,
        avg_integrity_pass_rate,
        avg_cleanup_completion,
        avg_docs_parity,
        avg_drift_penalty,
        projects,
        weakest_tasks,
    })
}

fn validate_unit_metric(name: &str, value: f64) -> Result<(), String> {
    if !(0.0..=1.0).contains(&value) {
        return Err(format!("{name} must be between 0.0 and 1.0"));
    }
    Ok(())
}

fn validate_skill_metric_set(prefix: &str, metrics: &SkillEvalMetricSet) -> Result<(), String> {
    validate_unit_metric(
        &format!("{prefix}.contract_satisfaction"),
        metrics.primary.contract_satisfaction,
    )?;
    validate_unit_metric(
        &format!("{prefix}.target_pass_rate"),
        metrics.primary.target_pass_rate,
    )?;
    validate_unit_metric(
        &format!("{prefix}.blocked_run_rate"),
        metrics.primary.blocked_run_rate,
    )?;
    validate_unit_metric(
        &format!("{prefix}.escalation_rate"),
        metrics.primary.escalation_rate,
    )?;
    validate_unit_metric(
        &format!("{prefix}.scope_discipline"),
        metrics.safety.scope_discipline,
    )?;
    validate_unit_metric(
        &format!("{prefix}.integrity_pass_rate"),
        metrics.safety.integrity_pass_rate,
    )?;
    validate_unit_metric(
        &format!("{prefix}.cleanup_completion"),
        metrics.safety.cleanup_completion,
    )?;
    validate_unit_metric(&format!("{prefix}.docs_parity"), metrics.safety.docs_parity)?;
    validate_unit_metric(
        &format!("{prefix}.drift_penalty"),
        metrics.safety.drift_penalty,
    )?;
    Ok(())
}

fn primary_score(metrics: &SkillEvalMetricSet) -> f64 {
    clamp01(
        (metrics.primary.contract_satisfaction
            + metrics.primary.target_pass_rate
            + (1.0 - metrics.primary.blocked_run_rate)
            + (1.0 - metrics.primary.escalation_rate))
            / 4.0,
    )
}

fn skill_eval_decision(
    baseline: &SkillEvalMetricSet,
    candidate: &SkillEvalMetricSet,
    suite_size: usize,
) -> (
    Vec<String>,
    Vec<String>,
    Vec<String>,
    PromotionDecision,
    Vec<String>,
) {
    const MIN_SUITE_SIZE: usize = 5;
    const MIN_PRIMARY_IMPROVEMENT: f64 = 0.05;
    const MAX_PRIMARY_REGRESSION: f64 = 0.03;

    let mut safety_regressions = Vec::new();
    let mut primary_improvements = Vec::new();
    let mut primary_regressions = Vec::new();
    let mut decision_reasons = Vec::new();

    let safety_pairs = [
        (
            "scope_discipline",
            baseline.safety.scope_discipline,
            candidate.safety.scope_discipline,
            false,
        ),
        (
            "integrity_pass_rate",
            baseline.safety.integrity_pass_rate,
            candidate.safety.integrity_pass_rate,
            false,
        ),
        (
            "cleanup_completion",
            baseline.safety.cleanup_completion,
            candidate.safety.cleanup_completion,
            false,
        ),
        (
            "docs_parity",
            baseline.safety.docs_parity,
            candidate.safety.docs_parity,
            false,
        ),
        (
            "drift_penalty",
            baseline.safety.drift_penalty,
            candidate.safety.drift_penalty,
            true,
        ),
    ];

    for (name, baseline_value, candidate_value, lower_is_better) in safety_pairs {
        let regressed = if lower_is_better {
            candidate_value > baseline_value
        } else {
            candidate_value < baseline_value
        };
        if regressed {
            safety_regressions.push(format!(
                "{name}: baseline={baseline_value:.2} candidate={candidate_value:.2}"
            ));
        }
    }

    let primary_pairs = [
        (
            "contract_satisfaction",
            baseline.primary.contract_satisfaction,
            candidate.primary.contract_satisfaction,
            false,
        ),
        (
            "target_pass_rate",
            baseline.primary.target_pass_rate,
            candidate.primary.target_pass_rate,
            false,
        ),
        (
            "blocked_run_rate",
            baseline.primary.blocked_run_rate,
            candidate.primary.blocked_run_rate,
            true,
        ),
        (
            "escalation_rate",
            baseline.primary.escalation_rate,
            candidate.primary.escalation_rate,
            true,
        ),
    ];

    for (name, baseline_value, candidate_value, lower_is_better) in primary_pairs {
        let delta = if lower_is_better {
            baseline_value - candidate_value
        } else {
            candidate_value - baseline_value
        };
        if delta >= MIN_PRIMARY_IMPROVEMENT {
            primary_improvements.push(format!(
                "{name}: baseline={baseline_value:.2} candidate={candidate_value:.2}"
            ));
        } else if delta <= -MAX_PRIMARY_REGRESSION {
            primary_regressions.push(format!(
                "{name}: baseline={baseline_value:.2} candidate={candidate_value:.2}"
            ));
        }
    }

    let sufficient_suite = suite_size >= MIN_SUITE_SIZE;
    if !sufficient_suite {
        decision_reasons.push(format!(
            "suite coverage below minimum: {} < {}",
            suite_size, MIN_SUITE_SIZE
        ));
    }
    if !safety_regressions.is_empty() {
        decision_reasons.push("safety regression detected".to_string());
    }
    if primary_improvements.is_empty() {
        decision_reasons.push("no primary metric improved by >= 0.05".to_string());
    }
    if !primary_regressions.is_empty() {
        decision_reasons.push("primary regression exceeded 0.03 tolerance".to_string());
    }

    let decision = if sufficient_suite
        && safety_regressions.is_empty()
        && !primary_improvements.is_empty()
        && primary_regressions.is_empty()
    {
        PromotionDecision::Promote
    } else {
        PromotionDecision::Reject
    };

    (
        safety_regressions,
        primary_improvements,
        primary_regressions,
        decision,
        decision_reasons,
    )
}

pub fn evaluate_skill(
    cwd: &Path,
    bus: &Path,
    request: EvaluateSkillRequest,
) -> Result<SkillEvalRecord, String> {
    if request.skill_name.trim().is_empty() {
        return Err("skill name must be non-empty".to_string());
    }
    if request.project_id.trim().is_empty() {
        return Err("project id must be non-empty".to_string());
    }
    if request.suite_id.trim().is_empty() {
        return Err("suite id must be non-empty".to_string());
    }
    if request.evidence_refs.is_empty() {
        return Err("skill eval requires at least one --evidence-ref".to_string());
    }
    validate_skill_metric_set("baseline", &request.baseline)?;
    validate_skill_metric_set("candidate", &request.candidate)?;

    let repo_root = detect_repo_root(cwd)?;
    let candidate = skill::list_skills(bus, Some(&repo_root))
        .into_iter()
        .find(|skill| skill.name == request.skill_name && skill.state == SkillState::Candidate)
        .ok_or_else(|| format!("candidate skill not found: {}", request.skill_name))?;

    let (safety_regressions, primary_improvements, primary_regressions, decision, reasons) =
        skill_eval_decision(&request.baseline, &request.candidate, request.suite_size);
    fs::create_dir_all(skill_eval_results_dir(&repo_root)).map_err(|e| e.to_string())?;

    let record = SkillEvalRecord {
        eval_id: skill_eval_id(&request.skill_name, &request.suite_id),
        skill_name: request.skill_name,
        project_id: request.project_id,
        suite_id: request.suite_id,
        role: request.role,
        candidate_path: candidate.path,
        baseline_primary_score: primary_score(&request.baseline),
        candidate_primary_score: primary_score(&request.candidate),
        baseline: request.baseline,
        candidate: request.candidate,
        suite_size: request.suite_size,
        sufficient_suite: request.suite_size >= 5,
        safety_regressions,
        primary_improvements,
        primary_regressions,
        decision,
        decision_reasons: reasons,
        evidence_refs: request.evidence_refs,
        notes: request.notes,
        created_at: Utc::now(),
    };

    let path = skill_eval_results_dir(&repo_root).join(format!("{}.json", record.eval_id));
    fs::write(
        &path,
        serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    Ok(record)
}

pub fn list_skill_evals(cwd: &Path) -> Result<Vec<SkillEvalRecord>, String> {
    let repo_root = detect_repo_root(cwd)?;
    let dir = skill_eval_results_dir(&repo_root);
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
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let record =
            serde_json::from_str::<SkillEvalRecord>(&content).map_err(|e| e.to_string())?;
        records.push(record);
    }
    records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
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

    fn sample_skill_metrics() -> SkillEvalMetricSet {
        SkillEvalMetricSet {
            primary: SkillEvalPrimaryMetrics {
                contract_satisfaction: 0.70,
                target_pass_rate: 0.72,
                blocked_run_rate: 0.20,
                escalation_rate: 0.10,
            },
            safety: SkillEvalSafetyMetrics {
                scope_discipline: 1.0,
                integrity_pass_rate: 1.0,
                cleanup_completion: 1.0,
                docs_parity: 1.0,
                drift_penalty: 0.05,
            },
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

    #[test]
    fn summarize_task_evals_aggregates_counts_and_averages() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        let mut success = sample_receipt(ReceiptStatus::Success);
        success.task_id = "task-ok".into();
        success.project = "specpunk".into();
        write_receipt_index(&bus, &success);
        let _ = evaluate_task(&repo, &bus, "task-ok").unwrap();

        let mut failed = sample_receipt(ReceiptStatus::Failure);
        failed.task_id = "task-bad".into();
        failed.project = "specpunk".into();
        failed.exit_code = 1;
        failed.punk_check_exit = Some(1);
        failed.summary = "stale docs and leftover v1 path".into();
        write_receipt_index(&bus, &failed);
        let _ = evaluate_task(&repo, &bus, "task-bad").unwrap();

        let summary = summarize_task_evals(&repo, None, None).unwrap();
        assert_eq!(summary.total, 2);
        assert_eq!(summary.accept_count, 1);
        assert_eq!(summary.reject_count, 1);
        assert_eq!(summary.projects.len(), 1);
        assert_eq!(summary.projects[0].project_id, "specpunk");
        assert_eq!(summary.weakest_tasks[0].task_id, "task-bad");
        assert!(summary.avg_score < 1.0);
    }

    #[test]
    fn summarize_task_evals_applies_project_filter_and_limit() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        let mut first = sample_receipt(ReceiptStatus::Success);
        first.task_id = "task-a".into();
        first.project = "alpha".into();
        write_receipt_index(&bus, &first);
        let _ = evaluate_task(&repo, &bus, "task-a").unwrap();

        let mut second = sample_receipt(ReceiptStatus::Failure);
        second.task_id = "task-b".into();
        second.project = "beta".into();
        second.exit_code = 1;
        second.punk_check_exit = Some(1);
        write_receipt_index(&bus, &second);
        let _ = evaluate_task(&repo, &bus, "task-b").unwrap();

        let mut third = sample_receipt(ReceiptStatus::Success);
        third.task_id = "task-c".into();
        third.project = "alpha".into();
        write_receipt_index(&bus, &third);
        let _ = evaluate_task(&repo, &bus, "task-c").unwrap();

        let summary = summarize_task_evals(&repo, Some(1), Some("alpha")).unwrap();
        assert_eq!(summary.total, 1);
        assert_eq!(summary.accept_count, 1);
        assert_eq!(summary.reject_count, 0);
        assert_eq!(summary.projects.len(), 1);
        assert_eq!(summary.projects[0].project_id, "alpha");
        assert_eq!(summary.weakest_tasks.len(), 1);
        assert_eq!(summary.weakest_tasks[0].task_id, "task-c");
    }

    #[test]
    fn evaluate_skill_promotes_clean_candidate_improvement() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        skill::create_candidate_skill(
            &repo,
            "cleanup-overlay",
            "cleanup improvements",
            "Use stricter cleanup checklist.",
            &[String::from("receipt:task-123")],
        )
        .unwrap();

        let baseline = sample_skill_metrics();
        let mut candidate = baseline.clone();
        candidate.primary.contract_satisfaction = 0.80;
        candidate.primary.target_pass_rate = 0.80;
        candidate.primary.blocked_run_rate = 0.10;

        let record = evaluate_skill(
            &repo,
            &bus,
            EvaluateSkillRequest {
                skill_name: "cleanup-overlay".into(),
                project_id: "specpunk".into(),
                suite_id: "cleanup-suite".into(),
                role: Some("implementer".into()),
                baseline,
                candidate,
                suite_size: 5,
                evidence_refs: vec!["receipt:task-123".into()],
                notes: vec!["offline suite replay".into()],
            },
        )
        .unwrap();

        assert_eq!(record.decision, PromotionDecision::Promote);
        assert!(record
            .primary_improvements
            .iter()
            .any(|item| item.contains("contract_satisfaction")));
        assert!(record.safety_regressions.is_empty());
        assert!(record.candidate_primary_score > record.baseline_primary_score);
    }

    #[test]
    fn evaluate_skill_rejects_docs_regression() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        skill::create_candidate_skill(
            &repo,
            "docs-overlay",
            "docs improvements",
            "Keep docs aligned.",
            &[String::from("receipt:task-456")],
        )
        .unwrap();

        let baseline = sample_skill_metrics();
        let mut candidate = baseline.clone();
        candidate.primary.contract_satisfaction = 0.80;
        candidate.safety.docs_parity = 0.60;

        let record = evaluate_skill(
            &repo,
            &bus,
            EvaluateSkillRequest {
                skill_name: "docs-overlay".into(),
                project_id: "specpunk".into(),
                suite_id: "docs-suite".into(),
                role: None,
                baseline,
                candidate,
                suite_size: 6,
                evidence_refs: vec!["receipt:task-456".into()],
                notes: Vec::new(),
            },
        )
        .unwrap();

        assert_eq!(record.decision, PromotionDecision::Reject);
        assert!(record
            .safety_regressions
            .iter()
            .any(|item| item.contains("docs_parity")));
    }

    #[test]
    fn list_skill_evals_returns_newest_first() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        skill::create_candidate_skill(
            &repo,
            "first-skill",
            "first",
            "First candidate.",
            &[String::from("receipt:first")],
        )
        .unwrap();
        skill::create_candidate_skill(
            &repo,
            "second-skill",
            "second",
            "Second candidate.",
            &[String::from("receipt:second")],
        )
        .unwrap();

        let baseline = sample_skill_metrics();
        let mut candidate = baseline.clone();
        candidate.primary.contract_satisfaction = 0.80;

        let _ = evaluate_skill(
            &repo,
            &bus,
            EvaluateSkillRequest {
                skill_name: "first-skill".into(),
                project_id: "specpunk".into(),
                suite_id: "suite-a".into(),
                role: None,
                baseline: baseline.clone(),
                candidate: candidate.clone(),
                suite_size: 5,
                evidence_refs: vec!["receipt:first".into()],
                notes: Vec::new(),
            },
        )
        .unwrap();

        std::thread::sleep(Duration::from_millis(10));

        let _ = evaluate_skill(
            &repo,
            &bus,
            EvaluateSkillRequest {
                skill_name: "second-skill".into(),
                project_id: "specpunk".into(),
                suite_id: "suite-b".into(),
                role: None,
                baseline,
                candidate,
                suite_size: 5,
                evidence_refs: vec!["receipt:second".into()],
                notes: Vec::new(),
            },
        )
        .unwrap();

        let listed = list_skill_evals(&repo).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].skill_name, "second-skill");
        assert_eq!(listed[1].skill_name, "first-skill");
    }
}
