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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArchitectureSeverity {
    None,
    Warn,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArchitectureAssessmentOutcome {
    NotApplicable,
    Pass,
    Block,
    Escalate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureThresholds {
    pub warn_file_loc: usize,
    pub critical_file_loc: usize,
    pub critical_scope_roots: usize,
    pub warn_expected_interfaces: usize,
    pub warn_import_paths: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureOversizedFile {
    pub path: String,
    pub loc: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureSignals {
    pub contract_id: String,
    pub feature_id: String,
    #[serde(default)]
    pub scope_roots: Vec<String>,
    #[serde(default)]
    pub oversized_files: Vec<ArchitectureOversizedFile>,
    pub distinct_scope_roots: usize,
    pub entry_point_count: usize,
    pub expected_interface_count: usize,
    pub import_path_count: usize,
    pub has_cleanup_obligations: bool,
    pub has_docs_obligations: bool,
    pub has_migration_sensitive_surfaces: bool,
    pub severity: ArchitectureSeverity,
    #[serde(default)]
    pub trigger_reasons: Vec<String>,
    pub thresholds: ArchitectureThresholds,
    pub computed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureFileLocBudget {
    pub path: String,
    pub max_after_loc: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureForbiddenPathDependency {
    pub from_glob: String,
    pub to_glob: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContractArchitectureIntegrity {
    pub review_required: bool,
    pub brief_ref: String,
    #[serde(default)]
    pub touched_roots_max: Option<usize>,
    #[serde(default)]
    pub file_loc_budgets: Vec<ArchitectureFileLocBudget>,
    #[serde(default)]
    pub forbidden_path_dependencies: Vec<ArchitectureForbiddenPathDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureFileLocAssessment {
    pub path: String,
    pub max_after_loc: usize,
    pub actual_loc: usize,
    pub status: CheckStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureForbiddenPathDependencyAssessment {
    pub from_glob: String,
    pub to_glob: String,
    pub status: CheckStatus,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureAssessment {
    pub run_id: String,
    pub contract_id: String,
    #[serde(default)]
    pub signals_ref: Option<String>,
    #[serde(default)]
    pub brief_ref: Option<String>,
    pub severity: ArchitectureSeverity,
    pub outcome: ArchitectureAssessmentOutcome,
    pub review_required: bool,
    pub contract_integrity_present: bool,
    pub touched_root_count: usize,
    #[serde(default)]
    pub touched_roots: Vec<String>,
    #[serde(default)]
    pub file_loc_results: Vec<ArchitectureFileLocAssessment>,
    #[serde(default)]
    pub forbidden_path_dependency_results: Vec<ArchitectureForbiddenPathDependencyAssessment>,
    #[serde(default)]
    pub reason_codes: Vec<String>,
    #[serde(default)]
    pub reasons: Vec<String>,
    pub assessed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandEvidence {
    pub evidence_type: String,
    pub lane: String,
    pub command: String,
    pub status: CheckStatus,
    pub summary: String,
    pub stdout_ref: Option<String>,
    pub stderr_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeclaredHarnessEvidence {
    pub evidence_type: String,
    pub profile: String,
    pub source_ref: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HarnessEvidence {
    pub evidence_type: String,
    pub profile: String,
    pub status: CheckStatus,
    pub summary: String,
    pub source_ref: Option<String>,
    pub artifact_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationContextFileState {
    pub path: String,
    pub exists: bool,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationContextIdentity {
    pub backend: VcsKind,
    pub workspace_ref: String,
    pub change_ref: String,
    pub base_ref: Option<String>,
    pub changed_files: Vec<String>,
    pub fingerprint_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationContext {
    pub identity: VerificationContextIdentity,
    pub file_states: Vec<VerificationContextFileState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_resolution_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_resolution_sha256: Option<String>,
    pub captured_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrozenArchitectureInputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signals_source_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signals_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signals_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_source_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_sha256: Option<String>,
    pub captured_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CapabilityScopeSeeds {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_points: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directory_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityCandidateView {
    pub id: String,
    pub version: String,
    pub source_kind: String,
    pub semantic_hash: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_markers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoCapabilityResolution {
    pub resolution_mode: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detected: Vec<CapabilityCandidateView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active: Vec<CapabilityCandidateView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suppressed: Vec<CapabilityCandidateView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicted: Vec<CapabilityCandidateView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advisory: Vec<CapabilityCandidateView>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectCapabilityIndex {
    pub schema: String,
    pub version: u32,
    pub project_id: String,
    pub source_kind: String,
    pub resolution_mode: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detected: Vec<CapabilityCandidateView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active: Vec<CapabilityCandidateView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suppressed: Vec<CapabilityCandidateView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicted: Vec<CapabilityCandidateView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advisory: Vec<CapabilityCandidateView>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrozenCapabilitySpec {
    pub id: String,
    pub version: String,
    pub source_kind: String,
    pub semantic_hash: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_markers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_rules: Vec<String>,
    #[serde(default, skip_serializing_if = "CapabilityScopeSeeds::is_empty")]
    pub scope_seeds: CapabilityScopeSeeds,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_checks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub integrity_checks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller_scaffold_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrozenCapabilityResolution {
    pub schema: String,
    pub version: u32,
    pub contract_id: String,
    pub project_capability_index_ref: String,
    pub project_capability_index_sha256: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_capabilities: Vec<FrozenCapabilitySpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_rules: Vec<String>,
    #[serde(default, skip_serializing_if = "CapabilityScopeSeeds::is_empty")]
    pub scope_seeds: CapabilityScopeSeeds,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_checks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub integrity_checks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller_scaffold_kind: Option<String>,
    pub generated_at: String,
}

impl CapabilityScopeSeeds {
    fn is_empty(&self) -> bool {
        self.entry_points.is_empty()
            && self.file_paths.is_empty()
            && self.directory_paths.is_empty()
    }
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
pub struct PersistedContract {
    #[serde(flatten)]
    pub contract: Contract,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture_signals_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture_integrity: Option<ContractArchitectureIntegrity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_resolution_ref: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_resolution: Option<RepoCapabilityResolution>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    #[serde(default)]
    pub verification_context_ref: Option<String>,
    #[serde(default)]
    pub architecture_inputs_ref: Option<String>,
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
    #[serde(default)]
    pub verification_context_ref: Option<String>,
    #[serde(default)]
    pub verification_context_identity: Option<VerificationContextIdentity>,
    #[serde(default)]
    pub command_evidence: Vec<CommandEvidence>,
    #[serde(default)]
    pub declared_harness_evidence: Vec<DeclaredHarnessEvidence>,
    #[serde(default)]
    pub harness_evidence: Vec<HarnessEvidence>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proofpack {
    pub id: String,
    pub decision_id: String,
    pub run_id: String,
    #[serde(default)]
    pub run_ref: Option<String>,
    pub contract_ref: String,
    pub receipt_ref: String,
    pub decision_ref: String,
    pub check_refs: Vec<String>,
    #[serde(default)]
    pub workspace_lineage: Option<RunVcs>,
    #[serde(default)]
    pub verification_context_ref: Option<String>,
    #[serde(default)]
    pub verification_context_identity: Option<VerificationContextIdentity>,
    #[serde(default)]
    pub executor_identity: Option<ProofExecutorIdentity>,
    #[serde(default)]
    pub reproducibility_claim: Option<ProofReproducibilityClaim>,
    #[serde(default)]
    pub command_evidence: Vec<CommandEvidence>,
    #[serde(default)]
    pub declared_harness_evidence: Vec<DeclaredHarnessEvidence>,
    #[serde(default)]
    pub harness_evidence: Vec<HarnessEvidence>,
    pub hashes: std::collections::BTreeMap<String, String>,
    pub summary: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofExecutorIdentity {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofReproducibilityClaim {
    pub level: String,
    pub summary: String,
    #[serde(default)]
    pub environment_digest_sha256: Option<String>,
    #[serde(default)]
    pub limits: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IncidentRecord {
    pub id: String,
    pub project_id: String,
    pub repo_root: String,
    pub work_id: String,
    pub goal: String,
    pub contract_ref: String,
    pub run_ref: String,
    pub decision_ref: String,
    pub proof_ref: String,
    #[serde(default)]
    pub autonomy_ref: Option<String>,
    pub incident_kind: String,
    pub decision_outcome: String,
    pub summary: String,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    pub failure_signature: String,
    #[serde(default)]
    pub capture_basis: Vec<String>,
    pub issue_draft_ref: String,
    pub repro_ref: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IncidentPromotionExecution {
    pub run_id: String,
    pub receipt_ref: String,
    pub decision_id: String,
    pub proof_id: String,
    pub decision_outcome: String,
    pub receipt_summary: String,
    pub completed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IncidentPromotionFailure {
    pub phase: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_id: Option<String>,
    pub failed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IncidentPromotionRecord {
    pub id: String,
    pub incident_id: String,
    pub source_project_id: String,
    pub source_repo_root: String,
    pub source_incident_ref: String,
    pub source_issue_draft_ref: String,
    pub source_repro_ref: String,
    pub target_project_id: String,
    pub target_repo_root: String,
    pub imported_incident_ref: String,
    pub imported_issue_draft_ref: String,
    pub imported_repro_ref: String,
    pub prepared_goal: String,
    pub draft_feature_id: String,
    pub draft_contract_id: String,
    #[serde(default)]
    pub auto_run_attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure: Option<IncidentPromotionFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<IncidentPromotionExecution>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IncidentSubmissionRecord {
    pub id: String,
    pub incident_id: String,
    pub submission_kind: String,
    pub github_repo: String,
    pub issue_title: String,
    pub body_ref: String,
    pub preview_command: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_issue_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_issue_number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResearchBudget {
    pub max_rounds: u32,
    pub max_worker_slots: u32,
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
    pub max_duration_minutes: u32,
    pub max_artifacts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchQuestion {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    #[serde(default)]
    pub subject_ref: Option<String>,
    pub question: String,
    pub goal: String,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResearchPacket {
    pub id: String,
    pub research_id: String,
    pub question_ref: String,
    pub repo_snapshot_ref: council::RepoSnapshotRef,
    #[serde(default)]
    pub contract_ref: Option<String>,
    #[serde(default)]
    pub receipt_ref: Option<String>,
    #[serde(default)]
    pub skill_ref: Option<String>,
    #[serde(default)]
    pub eval_ref: Option<String>,
    #[serde(default)]
    pub context_refs: Vec<String>,
    pub budget: ResearchBudget,
    #[serde(default)]
    pub stop_rules: Vec<String>,
    pub output_schema_ref: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResearchRecord {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub state: String,
    pub question_ref: String,
    pub packet_ref: String,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub synthesis_ref: Option<String>,
    #[serde(default)]
    pub synthesis_history_refs: Vec<String>,
    #[serde(default)]
    pub invalidated_synthesis_ref: Option<String>,
    #[serde(default)]
    pub invalidation_artifact_ref: Option<String>,
    #[serde(default)]
    pub invalidation_history: Vec<ResearchInvalidationEntry>,
    #[serde(default)]
    pub outcome: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchInvalidationEntry {
    pub invalidated_synthesis_ref: String,
    pub invalidating_artifact_ref: String,
    pub invalidated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchArtifact {
    pub id: String,
    pub research_id: String,
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub source_ref: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResearchStartInput {
    pub kind: String,
    pub question: String,
    pub goal: String,
    #[serde(default)]
    pub subject_ref: Option<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub context_refs: Vec<String>,
    #[serde(default)]
    pub contract_ref: Option<String>,
    #[serde(default)]
    pub receipt_ref: Option<String>,
    #[serde(default)]
    pub skill_ref: Option<String>,
    #[serde(default)]
    pub eval_ref: Option<String>,
    pub budget: ResearchBudget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchArtifactInput {
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchSynthesis {
    pub id: String,
    pub research_id: String,
    pub outcome: String,
    pub summary: String,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub supersedes_ref: Option<String>,
    #[serde(default)]
    pub follow_up_refs: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchSynthesisInput {
    pub outcome: String,
    pub summary: String,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub replace_existing: bool,
    #[serde(default)]
    pub follow_up_refs: Vec<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::{self, IntoDeserializer, Visitor};
    use serde::forward_to_deserialize_any;

    #[derive(Clone)]
    enum LegacyValue {
        String(&'static str),
        F64(f64),
        Seq(Vec<LegacyValue>),
        Map(Vec<(&'static str, LegacyValue)>),
    }

    impl<'de> IntoDeserializer<'de, de::value::Error> for LegacyValue {
        type Deserializer = Self;

        fn into_deserializer(self) -> Self::Deserializer {
            self
        }
    }

    impl<'de> de::Deserializer<'de> for LegacyValue {
        type Error = de::value::Error;

        fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            match self {
                LegacyValue::String(value) => value.into_deserializer().deserialize_any(visitor),
                LegacyValue::F64(value) => value.into_deserializer().deserialize_any(visitor),
                LegacyValue::Seq(values) => {
                    visitor.visit_seq(de::value::SeqDeserializer::new(values.into_iter()))
                }
                LegacyValue::Map(values) => visitor.visit_map(de::value::MapDeserializer::new(
                    values
                        .into_iter()
                        .map(|(key, value)| (key.into_deserializer(), value)),
                )),
            }
        }

        fn deserialize_enum<V>(
            self,
            _name: &'static str,
            _variants: &'static [&'static str],
            visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            match self {
                LegacyValue::String(value) => visitor.visit_enum(value.into_deserializer()),
                other => other.deserialize_any(visitor),
            }
        }

        fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            visitor.visit_some(self)
        }

        forward_to_deserialize_any! {
            bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf
            unit unit_struct newtype_struct seq tuple tuple_struct map struct identifier
            ignored_any
        }
    }

    #[test]
    fn legacy_decision_object_without_command_evidence_deserializes() {
        let legacy = LegacyValue::Map(vec![
            ("id", LegacyValue::String("dec_1")),
            ("run_id", LegacyValue::String("run_1")),
            ("contract_id", LegacyValue::String("ct_1")),
            ("decision", LegacyValue::String("accept")),
            ("deterministic_status", LegacyValue::String("pass")),
            ("target_status", LegacyValue::String("pass")),
            ("integrity_status", LegacyValue::String("pass")),
            ("confidence_estimate", LegacyValue::F64(1.0)),
            (
                "decision_basis",
                LegacyValue::Seq(vec![LegacyValue::String("checks passed")]),
            ),
            (
                "contract_ref",
                LegacyValue::String(".punk/contracts/feat_1/v1.json"),
            ),
            (
                "receipt_ref",
                LegacyValue::String(".punk/runs/run_1/receipt.json"),
            ),
            (
                "check_refs",
                LegacyValue::Seq(vec![LegacyValue::String(
                    ".punk/runs/run_1/checks/target-01.stdout.log",
                )]),
            ),
            ("created_at", LegacyValue::String("2026-04-08T00:00:00Z")),
        ]);

        let decision = DecisionObject::deserialize(legacy).unwrap();
        assert!(decision.verification_context_ref.is_none());
        assert!(decision.verification_context_identity.is_none());
        assert!(decision.command_evidence.is_empty());
        assert!(decision.declared_harness_evidence.is_empty());
        assert!(decision.harness_evidence.is_empty());
    }

    #[test]
    fn legacy_proofpack_without_command_evidence_deserializes() {
        let legacy = LegacyValue::Map(vec![
            ("id", LegacyValue::String("proof_1")),
            ("decision_id", LegacyValue::String("dec_1")),
            ("run_id", LegacyValue::String("run_1")),
            (
                "contract_ref",
                LegacyValue::String(".punk/contracts/feat_1/v1.json"),
            ),
            (
                "receipt_ref",
                LegacyValue::String(".punk/runs/run_1/receipt.json"),
            ),
            (
                "decision_ref",
                LegacyValue::String(".punk/decisions/dec_1.json"),
            ),
            (
                "check_refs",
                LegacyValue::Seq(vec![LegacyValue::String(
                    ".punk/runs/run_1/checks/target-01.stdout.log",
                )]),
            ),
            ("hashes", LegacyValue::Map(vec![])),
            ("summary", LegacyValue::String("proof for dec_1")),
            ("created_at", LegacyValue::String("2026-04-08T00:00:00Z")),
        ]);

        let proofpack = Proofpack::deserialize(legacy).unwrap();
        assert!(proofpack.run_ref.is_none());
        assert!(proofpack.workspace_lineage.is_none());
        assert!(proofpack.verification_context_ref.is_none());
        assert!(proofpack.verification_context_identity.is_none());
        assert!(proofpack.executor_identity.is_none());
        assert!(proofpack.reproducibility_claim.is_none());
        assert!(proofpack.command_evidence.is_empty());
        assert!(proofpack.declared_harness_evidence.is_empty());
        assert!(proofpack.harness_evidence.is_empty());
    }

    #[test]
    fn legacy_research_record_without_synthesis_history_deserializes() {
        let legacy = LegacyValue::Map(vec![
            ("id", LegacyValue::String("research_1")),
            ("project_id", LegacyValue::String("specpunk")),
            ("kind", LegacyValue::String("architecture")),
            ("state", LegacyValue::String("synthesized")),
            (
                "question_ref",
                LegacyValue::String(".punk/research/research_1/question.json"),
            ),
            (
                "packet_ref",
                LegacyValue::String(".punk/research/research_1/packet.json"),
            ),
            (
                "artifact_refs",
                LegacyValue::Seq(vec![LegacyValue::String(
                    ".punk/research/research_1/artifacts/artifact_1.json",
                )]),
            ),
            (
                "synthesis_ref",
                LegacyValue::String(".punk/research/research_1/synthesis.json"),
            ),
            ("outcome", LegacyValue::String("adr_draft")),
            ("created_at", LegacyValue::String("2026-04-11T00:00:00Z")),
            ("updated_at", LegacyValue::String("2026-04-11T00:00:00Z")),
        ]);

        let record = ResearchRecord::deserialize(legacy).unwrap();
        assert!(record.synthesis_history_refs.is_empty());
        assert!(record.invalidated_synthesis_ref.is_none());
        assert!(record.invalidation_artifact_ref.is_none());
        assert!(record.invalidation_history.is_empty());
    }

    #[test]
    fn legacy_research_synthesis_without_supersedes_ref_deserializes() {
        let legacy = LegacyValue::Map(vec![
            ("id", LegacyValue::String("synthesis_1")),
            ("research_id", LegacyValue::String("research_1")),
            ("outcome", LegacyValue::String("adr_draft")),
            ("summary", LegacyValue::String("bounded recommendation")),
            (
                "artifact_refs",
                LegacyValue::Seq(vec![LegacyValue::String(
                    ".punk/research/research_1/artifacts/artifact_1.json",
                )]),
            ),
            (
                "follow_up_refs",
                LegacyValue::Seq(vec![LegacyValue::String("docs/product/ARCHITECTURE.md")]),
            ),
            ("created_at", LegacyValue::String("2026-04-11T00:00:00Z")),
        ]);

        let synthesis = ResearchSynthesis::deserialize(legacy).unwrap();
        assert!(synthesis.supersedes_ref.is_none());
    }

    #[test]
    fn persisted_contract_architecture_integrity_round_trips() {
        let persisted = PersistedContract {
            contract: Contract {
                id: "ct_1".into(),
                feature_id: "feat_1".into(),
                version: 1,
                status: ContractStatus::Approved,
                prompt_source: "add architecture steering".into(),
                entry_points: vec!["crates/punk-orch/src/lib.rs".into()],
                import_paths: vec!["crates/punk-core".into()],
                expected_interfaces: vec!["architecture signals".into()],
                behavior_requirements: vec!["keep gate deterministic".into()],
                allowed_scope: vec!["crates/punk-orch/src/lib.rs".into()],
                target_checks: vec!["cargo test -p punk-orch".into()],
                integrity_checks: vec!["cargo test --workspace".into()],
                risk_level: "medium".into(),
                created_at: "2026-04-12T00:00:00Z".into(),
                approved_at: Some("2026-04-12T00:05:00Z".into()),
            },
            architecture_signals_ref: Some(
                ".punk/contracts/feat_1/architecture-signals.json".into(),
            ),
            architecture_integrity: Some(ContractArchitectureIntegrity {
                review_required: true,
                brief_ref: ".punk/contracts/feat_1/architecture-brief.md".into(),
                touched_roots_max: Some(1),
                file_loc_budgets: vec![ArchitectureFileLocBudget {
                    path: "crates/punk-orch/src/lib.rs".into(),
                    max_after_loc: 900,
                }],
                forbidden_path_dependencies: vec![ArchitectureForbiddenPathDependency {
                    from_glob: "crates/punk-gate/**".into(),
                    to_glob: "crates/specpunk/**".into(),
                }],
            }),
            capability_resolution_ref: Some(
                ".punk/contracts/feat_1/capability-resolution.json".into(),
            ),
        };

        let json = serde_json::to_value(&persisted).unwrap();
        assert_eq!(
            json["architecture_integrity"]["forbidden_path_dependencies"][0]["from_glob"],
            "crates/punk-gate/**"
        );

        let decoded: PersistedContract = serde_json::from_value(json).unwrap();
        let integrity = decoded.architecture_integrity.unwrap();
        assert_eq!(integrity.touched_roots_max, Some(1));
        assert_eq!(integrity.file_loc_budgets[0].max_after_loc, 900);
        assert_eq!(integrity.forbidden_path_dependencies.len(), 1);
    }

    #[test]
    fn incident_record_round_trips_through_json() {
        let incident = IncidentRecord {
            id: "inc_123".into(),
            project_id: "specpunk".into(),
            repo_root: "/tmp/specpunk".into(),
            work_id: "feat_123".into(),
            goal: "capture runtime bug".into(),
            contract_ref: ".punk/contracts/feat_123/v1.json".into(),
            run_ref: ".punk/runs/feat_123/run.json".into(),
            decision_ref: ".punk/decisions/dec_123.json".into(),
            proof_ref: ".punk/proofs/dec_123/proofpack.json".into(),
            autonomy_ref: Some(".punk/autonomy/feat_123/auto_123.json".into()),
            incident_kind: "suspected_runtime_bug".into(),
            decision_outcome: "blocked".into(),
            summary: "bounded run stalled without product changes".into(),
            blocked_reason: Some("no-progress failure".into()),
            failure_signature: "blocked:no-progress".into(),
            capture_basis: vec![
                "decision outcome: blocked".into(),
                "matched runtime marker: no-progress".into(),
            ],
            issue_draft_ref: ".punk/incidents/feat_123/inc_123/issue.md".into(),
            repro_ref: ".punk/incidents/feat_123/inc_123/repro.md".into(),
            created_at: "2026-04-16T00:00:00Z".into(),
        };

        let json = serde_json::to_value(&incident).unwrap();
        let decoded: IncidentRecord = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, incident);
    }

    #[test]
    fn incident_promotion_record_round_trips_through_json() {
        let promotion = IncidentPromotionRecord {
            id: "prom_123".into(),
            incident_id: "inc_123".into(),
            source_project_id: "foreign-demo".into(),
            source_repo_root: "/tmp/source".into(),
            source_incident_ref: ".punk/incidents/feat_123/inc_123/incident.json".into(),
            source_issue_draft_ref: ".punk/incidents/feat_123/inc_123/issue.md".into(),
            source_repro_ref: ".punk/incidents/feat_123/inc_123/repro.md".into(),
            target_project_id: "specpunk".into(),
            target_repo_root: "/tmp/target".into(),
            imported_incident_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/incident.json".into(),
            imported_issue_draft_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/issue.md".into(),
            imported_repro_ref: ".punk/imported-incidents/foreign-demo/inc_123/prom_123/repro.md"
                .into(),
            prepared_goal: "Investigate and fix promoted incident inc_123".into(),
            draft_feature_id: "feat_456".into(),
            draft_contract_id: "ct_456_v1".into(),
            auto_run_attempts: 2,
            last_attempt_at: Some("2026-04-16T00:45:00Z".into()),
            last_failure: Some(IncidentPromotionFailure {
                phase: "proof_write".into(),
                summary: "proofpack write failed after gate output".into(),
                contract_status: Some("approved".into()),
                run_id: Some("run_456".into()),
                receipt_ref: Some(".punk/runs/run_456/receipt.json".into()),
                decision_id: Some("dec_456".into()),
                failed_at: "2026-04-16T00:40:00Z".into(),
            }),
            execution: Some(IncidentPromotionExecution {
                run_id: "run_456".into(),
                receipt_ref: ".punk/runs/run_456/receipt.json".into(),
                decision_id: "dec_456".into(),
                proof_id: "proof_456".into(),
                decision_outcome: "accept".into(),
                receipt_summary: "bounded fix applied and checks passed".into(),
                completed_at: "2026-04-16T00:45:00Z".into(),
            }),
            created_at: "2026-04-16T00:30:00Z".into(),
        };

        let json = serde_json::to_value(&promotion).unwrap();
        let decoded: IncidentPromotionRecord = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, promotion);
    }

    #[test]
    fn incident_submission_record_round_trips_through_json() {
        let submission = IncidentSubmissionRecord {
            id: "sub_123".into(),
            incident_id: "inc_123".into(),
            submission_kind: "github_issue".into(),
            github_repo: "heurema/specpunk".into(),
            issue_title: "punk runtime bug [inc_123]: blocked:no-progress".into(),
            body_ref: ".punk/submissions/inc_123/sub_123/body.md".into(),
            preview_command: "gh issue create --repo heurema/specpunk".into(),
            state: "submitted".into(),
            published_issue_url: Some("https://github.com/heurema/specpunk/issues/123".into()),
            published_issue_number: Some(123),
            publish_error: None,
            created_at: "2026-04-16T01:00:00Z".into(),
            updated_at: "2026-04-16T01:01:00Z".into(),
        };

        let json = serde_json::to_value(&submission).unwrap();
        let decoded: IncidentSubmissionRecord = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, submission);
    }

    #[test]
    fn frozen_architecture_inputs_round_trip() {
        let frozen = FrozenArchitectureInputs {
            signals_source_ref: Some(".punk/contracts/feat_1/architecture-signals.json".into()),
            signals_ref: Some(".punk/runs/run_1/architecture-signals.json".into()),
            signals_sha256: Some("signals-sha".into()),
            brief_source_ref: Some(".punk/contracts/feat_1/architecture-brief.md".into()),
            brief_ref: Some(".punk/runs/run_1/architecture-brief.md".into()),
            brief_sha256: Some("brief-sha".into()),
            captured_at: "2026-04-16T00:00:00Z".into(),
        };

        let json = serde_json::to_value(&frozen).unwrap();
        assert_eq!(
            json["signals_ref"],
            ".punk/runs/run_1/architecture-signals.json"
        );
        assert_eq!(json["brief_sha256"], "brief-sha");

        let decoded: FrozenArchitectureInputs = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, frozen);
    }
}
