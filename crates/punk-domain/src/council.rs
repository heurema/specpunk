use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::VcsKind;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CouncilKind {
    Architecture,
    Contract,
    Review,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CouncilPhase {
    Proposal,
    Review,
    Synthesis,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CouncilOutcome {
    Leader,
    Hybrid,
    Escalate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Codex,
    ClaudeCode,
    Gemini,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilSubjectRef {
    pub feature_id: Option<String>,
    pub contract_id: Option<String>,
    pub run_id: Option<String>,
    pub question: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepoSnapshotRef {
    pub vcs: Option<VcsKind>,
    pub head_ref: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilBudget {
    pub proposal_slots: u32,
    pub review_slots: u32,
    pub slot_timeout_secs: u64,
    pub max_total_duration_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilCriterion {
    pub key: String,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilRubric {
    pub criteria: Vec<CouncilCriterion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilRoleAssignment {
    pub role: String,
    pub provider: ProviderKind,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilPacket {
    pub id: String,
    pub kind: CouncilKind,
    pub project_id: String,
    pub subject: CouncilSubjectRef,
    pub repo_snapshot: RepoSnapshotRef,
    pub prompt: String,
    pub constraints: Vec<String>,
    pub rubric: CouncilRubric,
    pub role_assignments: Vec<CouncilRoleAssignment>,
    pub budget: CouncilBudget,
    pub contract_ref: Option<String>,
    pub receipt_ref: Option<String>,
    pub research_brief_ref: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilSlotSpec {
    pub id: String,
    pub council_id: String,
    pub phase: CouncilPhase,
    pub provider: ProviderKind,
    pub model: String,
    pub role: String,
    pub prompt_ref: String,
    pub packet_ref: String,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilSlotStatus {
    pub slot_id: String,
    pub state: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilProposal {
    pub council_id: String,
    pub slot_id: String,
    pub provider: ProviderKind,
    pub model: String,
    pub role: String,
    pub label: Option<String>,
    pub summary: String,
    pub findings: Vec<String>,
    pub risks: Vec<String>,
    pub must_keep: Vec<String>,
    pub must_fix: Vec<String>,
    pub cleanup_obligations: Vec<String>,
    pub confidence: f32,
    pub content_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilReview {
    pub council_id: String,
    pub reviewer_slot_id: String,
    pub proposal_label: String,
    pub criterion_scores: BTreeMap<String, u8>,
    pub findings: Vec<String>,
    pub blockers: Vec<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilProposalScore {
    pub proposal_label: String,
    pub weighted_score: f32,
    pub blocker_count: u32,
    pub review_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilScoreboard {
    pub proposal_scores: Vec<CouncilProposalScore>,
    pub top_label: Option<String>,
    pub second_label: Option<String>,
    pub top_gap: Option<f32>,
    pub high_disagreement: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilSynthesis {
    pub council_id: String,
    pub outcome: CouncilOutcome,
    pub selected_labels: Vec<String>,
    pub rationale: String,
    pub must_keep: Vec<String>,
    pub must_fix: Vec<String>,
    pub unresolved_risks: Vec<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CouncilRecord {
    pub id: String,
    pub packet_ref: String,
    pub proposal_refs: Vec<String>,
    pub review_refs: Vec<String>,
    pub synthesis_ref: String,
    pub scoreboard_ref: String,
    pub completed_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn council_packet_and_record_shapes_are_constructible() {
        let packet = CouncilPacket {
            id: "council_1".into(),
            kind: CouncilKind::Architecture,
            project_id: "project_1".into(),
            subject: CouncilSubjectRef {
                feature_id: Some("feat_1".into()),
                contract_id: None,
                run_id: None,
                question: Some("how should we model council packets?".into()),
            },
            repo_snapshot: RepoSnapshotRef {
                vcs: Some(VcsKind::Git),
                head_ref: Some("abc123".into()),
                dirty: false,
            },
            prompt: "compare bounded schema options".into(),
            constraints: vec!["schema only".into()],
            rubric: CouncilRubric {
                criteria: vec![CouncilCriterion {
                    key: "correctness".into(),
                    weight: 0.6,
                }],
            },
            role_assignments: vec![CouncilRoleAssignment {
                role: "proposer".into(),
                provider: ProviderKind::Codex,
                model: "gpt-5.4".into(),
            }],
            budget: CouncilBudget {
                proposal_slots: 3,
                review_slots: 3,
                slot_timeout_secs: 300,
                max_total_duration_secs: 1800,
            },
            contract_ref: Some("contracts/feat_1/v1.json".into()),
            receipt_ref: None,
            research_brief_ref: None,
            created_at: "2026-03-30T11:00:00Z".into(),
        };

        let review = CouncilReview {
            council_id: packet.id.clone(),
            reviewer_slot_id: "reviewer_1".into(),
            proposal_label: "A".into(),
            criterion_scores: BTreeMap::from([("correctness".into(), 5)]),
            findings: vec!["bounded schema".into()],
            blockers: vec![],
            confidence: 0.8,
        };

        let record = CouncilRecord {
            id: packet.id.clone(),
            packet_ref: "packet.json".into(),
            proposal_refs: vec!["proposals/A.json".into()],
            review_refs: vec!["reviews/codex.json".into()],
            synthesis_ref: "synthesis.json".into(),
            scoreboard_ref: "scoreboard.json".into(),
            completed_at: "2026-03-30T11:10:00Z".into(),
        };

        assert_eq!(packet.kind, CouncilKind::Architecture);
        assert_eq!(review.criterion_scores.get("correctness"), Some(&5));
        assert_eq!(record.id, packet.id);
    }
}
