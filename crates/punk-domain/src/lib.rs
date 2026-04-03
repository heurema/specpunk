use std::collections::BTreeMap;

use chrono::Utc;
use serde::{Deserialize, Serialize};

pub mod council;

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModeId {
    Plot,
    Cut,
    Gate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VcsKind {
    Jj,
    Git,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeatureStatus {
    Draft,
    Active,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContractStatus {
    Draft,
    Approved,
    Superseded,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Implement,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Claimed,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Finished,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Accept,
    Block,
    Escalate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyOutcome {
    Succeeded,
    Blocked,
    Escalated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeterministicStatus {
    Pass,
    Fail,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    Partial,
    Unverified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub path: String,
    pub vcs_backend: Option<VcsKind>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub summary: String,
    pub status: FeatureStatus,
    pub target_surface: Vec<String>,
    pub integrity_scope: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    pub id: String,
    pub feature_id: String,
    pub version: u32,
    pub status: ContractStatus,
    pub prompt_source: String,
    pub entry_points: Vec<String>,
    pub import_paths: Vec<String>,
    pub expected_interfaces: Vec<String>,
    pub behavior_requirements: Vec<String>,
    pub allowed_scope: Vec<String>,
    pub target_checks: Vec<String>,
    pub integrity_checks: Vec<String>,
    pub risk_level: String,
    pub created_at: String,
    pub approved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftProposal {
    pub title: String,
    pub summary: String,
    pub entry_points: Vec<String>,
    pub import_paths: Vec<String>,
    pub expected_interfaces: Vec<String>,
    pub behavior_requirements: Vec<String>,
    pub allowed_scope: Vec<String>,
    pub target_checks: Vec<String>,
    pub integrity_checks: Vec<String>,
    pub risk_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoScanSummary {
    pub project_kind: String,
    pub manifests: Vec<String>,
    pub package_manager: Option<String>,
    pub available_scripts: BTreeMap<String, String>,
    pub candidate_entry_points: Vec<String>,
    pub candidate_scope_paths: Vec<String>,
    pub candidate_file_scope_paths: Vec<String>,
    pub candidate_directory_scope_paths: Vec<String>,
    pub candidate_target_checks: Vec<String>,
    pub candidate_integrity_checks: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftValidationError {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftInput {
    pub repo_root: String,
    pub prompt: String,
    pub scan: RepoScanSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefineInput {
    pub repo_root: String,
    pub prompt: String,
    pub guidance: String,
    pub current: DraftProposal,
    pub scan: RepoScanSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub feature_id: String,
    pub contract_id: String,
    pub kind: TaskKind,
    pub status: TaskStatus,
    pub requested_by: String,
    pub created_at: String,
    pub claimed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunVcs {
    pub backend: VcsKind,
    pub workspace_ref: String,
    pub change_ref: String,
    pub base_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub task_id: String,
    pub feature_id: String,
    pub contract_id: String,
    pub attempt: u32,
    pub status: RunStatus,
    pub mode_origin: ModeId,
    pub vcs: RunVcs,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptArtifacts {
    pub stdout_ref: String,
    pub stderr_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub id: String,
    pub run_id: String,
    pub task_id: String,
    pub status: String,
    pub executor_name: String,
    pub changed_files: Vec<String>,
    pub artifacts: ReceiptArtifacts,
    pub checks_run: Vec<String>,
    pub duration_ms: u64,
    pub cost_usd: Option<f64>,
    pub summary: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionObject {
    pub id: String,
    pub run_id: String,
    pub contract_id: String,
    pub decision: Decision,
    pub deterministic_status: DeterministicStatus,
    pub target_status: CheckStatus,
    pub integrity_status: CheckStatus,
    pub confidence_estimate: f64,
    pub decision_basis: Vec<String>,
    pub contract_ref: String,
    pub receipt_ref: String,
    pub check_refs: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proofpack {
    pub id: String,
    pub decision_id: String,
    pub run_id: String,
    pub contract_ref: String,
    pub receipt_ref: String,
    pub decision_ref: String,
    pub check_refs: Vec<String>,
    pub hashes: std::collections::BTreeMap<String, String>,
    pub summary: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyRecord {
    pub id: String,
    pub work_id: String,
    pub goal_ref: Option<String>,
    pub contract_ref: String,
    pub run_ref: String,
    pub decision_ref: String,
    pub proof_ref: String,
    pub autonomy_outcome: AutonomyOutcome,
    pub basis_summary: String,
    pub recovery_contract_ref: Option<String>,
    pub next_action: String,
    pub next_action_ref: String,
    pub recorded_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_id: String,
    pub ts: String,
    pub project_id: String,
    pub feature_id: Option<String>,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    #[serde(default = "default_event_actor")]
    pub actor: String,
    pub mode: ModeId,
    pub kind: String,
    pub payload_ref: Option<String>,
    pub payload_sha256: Option<String>,
}

fn default_event_actor() -> String {
    "unknown".to_string()
}
