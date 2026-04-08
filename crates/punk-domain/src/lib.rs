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
    #[serde(default)]
    pub command_evidence: Vec<CommandEvidence>,
    #[serde(default)]
    pub declared_harness_evidence: Vec<DeclaredHarnessEvidence>,
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
    #[serde(default)]
    pub command_evidence: Vec<CommandEvidence>,
    #[serde(default)]
    pub declared_harness_evidence: Vec<DeclaredHarnessEvidence>,
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

        forward_to_deserialize_any! {
            bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf option
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
        assert!(decision.command_evidence.is_empty());
        assert!(decision.declared_harness_evidence.is_empty());
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
        assert!(proofpack.command_evidence.is_empty());
        assert!(proofpack.declared_harness_evidence.is_empty());
    }
}
