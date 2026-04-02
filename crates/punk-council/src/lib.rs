pub mod anonymize;
mod events;
pub mod packet;
pub mod proposal;
pub mod review;
mod scoring;
pub mod storage;
pub mod synthesis;

use std::path::{Path, PathBuf};

use anonymize::AnonymizationBatchResult;
use anyhow::Result;
use packet::{ArchitecturePacketInput, ContractPacketInput, CouncilPacketInput, ReviewPacketInput};
use proposal::{ProposalAdapterBinding, ProposalRunResult};
use punk_domain::council::{
    CouncilKind, CouncilPacket, CouncilProposal, CouncilRecord, CouncilScoreboard,
};
use punk_events::EventStore;
use review::{ReviewAdapterBinding, ReviewRunResult};
use storage::{persist_packet, persist_record, persist_synthesis, CouncilPaths};

pub use anonymize::{
    anonymize_proposals, AnonymizedCouncilProposal, PersistedAnonymizedProposal,
    ProposalLabelMapArtifact, ProposalLabelMapEntry,
};
pub use scoring::score_reviews;
pub use storage::CouncilPaths as CouncilRunPaths;
pub use synthesis::synthesize_from_scoreboard;

pub struct CouncilService {
    repo_root: PathBuf,
    events: EventStore,
}

impl CouncilService {
    pub fn new(repo_root: impl AsRef<Path>) -> Self {
        let repo_root = repo_root.as_ref().to_path_buf();
        let events = EventStore::new(repo_root.join(".punk"));
        Self { repo_root, events }
    }

    pub fn build_packet(&self, input: CouncilPacketInput) -> Result<CouncilPacket> {
        let CouncilPacketInput {
            kind,
            project_id,
            subject,
            prompt,
            constraints,
            rubric,
            role_assignments,
            budget,
            contract_ref,
            receipt_ref,
            research_brief_ref,
        } = input;

        match kind {
            CouncilKind::Architecture => packet::build_architecture_packet(
                &self.repo_root,
                ArchitecturePacketInput {
                    project_id,
                    subject,
                    prompt,
                    constraints,
                    rubric: Some(rubric),
                    role_assignments: Some(role_assignments),
                    budget: Some(budget),
                    contract_ref,
                    receipt_ref,
                    research_brief_ref,
                },
            ),
            CouncilKind::Contract => packet::build_contract_packet(
                &self.repo_root,
                ContractPacketInput {
                    project_id,
                    subject,
                    prompt,
                    constraints,
                    rubric: Some(rubric),
                    role_assignments: Some(role_assignments),
                    budget: Some(budget),
                    contract_ref,
                    receipt_ref,
                    research_brief_ref,
                },
            ),
            CouncilKind::Review => packet::build_review_packet(
                &self.repo_root,
                ReviewPacketInput {
                    project_id,
                    subject,
                    prompt,
                    constraints,
                    rubric: Some(rubric),
                    role_assignments: Some(role_assignments),
                    budget: Some(budget),
                    contract_ref,
                    receipt_ref,
                    research_brief_ref,
                },
            ),
        }
    }

    pub fn start(&self, packet: &CouncilPacket) -> Result<CouncilRunPaths> {
        let paths = CouncilPaths::new(&self.repo_root, &packet.id);
        persist_packet(&self.repo_root, &paths, packet)?;
        events::emit_started(&self.events, &self.repo_root, packet, &paths.packet_path)?;
        Ok(paths)
    }

    pub fn complete(&self, packet: &CouncilPacket) -> Result<CouncilRecord> {
        let paths = CouncilPaths::new(&self.repo_root, &packet.id);
        let synthesis_ref = paths
            .synthesis_path
            .strip_prefix(&self.repo_root)
            .unwrap_or(&paths.synthesis_path)
            .to_string_lossy()
            .replace('\\', "/");
        let record = persist_record(&self.repo_root, &paths, packet, &[], &[], synthesis_ref)?;
        events::emit_completed(
            &self.events,
            &self.repo_root,
            packet,
            &record,
            &paths.record_path,
        )?;
        Ok(record)
    }

    pub fn run_proposals(
        &self,
        packet: &CouncilPacket,
        bindings: &[ProposalAdapterBinding<'_>],
    ) -> Result<ProposalRunResult> {
        proposal::run_proposals(&self.repo_root, &self.events, packet, bindings)
    }

    pub fn anonymize_proposals(
        &self,
        council_id: &str,
        proposals: &[CouncilProposal],
    ) -> Result<AnonymizationBatchResult> {
        anonymize::anonymize_proposals(&self.repo_root, council_id, proposals)
    }

    pub fn run_reviews(
        &self,
        packet: &CouncilPacket,
        proposals: &[anonymize::AnonymizedCouncilProposal],
        bindings: &[ReviewAdapterBinding<'_>],
    ) -> Result<ReviewRunResult> {
        review::run_reviews(&self.repo_root, &self.events, packet, proposals, bindings)
    }

    pub fn complete_synthesis(
        &self,
        packet: &CouncilPacket,
        proposal_refs: &[String],
        review_refs: &[String],
        scoreboard: &CouncilScoreboard,
    ) -> Result<CouncilRecord> {
        let paths = CouncilPaths::new(&self.repo_root, &packet.id);
        let synthesis = synthesize_from_scoreboard(&packet.id, scoreboard)?;
        let synthesis_ref = persist_synthesis(&self.repo_root, &paths, &synthesis)?;
        events::emit_synthesis_written(
            &self.events,
            &self.repo_root,
            packet,
            &paths.synthesis_path,
        )?;
        let record = persist_record(
            &self.repo_root,
            &paths,
            packet,
            proposal_refs,
            review_refs,
            synthesis_ref,
        )?;
        events::emit_completed(
            &self.events,
            &self.repo_root,
            packet,
            &record,
            &paths.record_path,
        )?;
        Ok(record)
    }
}
#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use punk_domain::council::{
        CouncilBudget, CouncilCriterion, CouncilKind, CouncilPacket, CouncilRoleAssignment,
        CouncilRubric, CouncilSubjectRef, ProviderKind, RepoSnapshotRef,
    };
    use punk_domain::VcsKind;

    fn sample_packet() -> CouncilPacket {
        CouncilPacket {
            id: "council_test".into(),
            kind: CouncilKind::Architecture,
            project_id: "specpunk".into(),
            subject: CouncilSubjectRef {
                feature_id: Some("feat_test".into()),
                contract_id: None,
                run_id: None,
                question: Some("how should council storage start?".into()),
            },
            repo_snapshot: RepoSnapshotRef {
                vcs: Some(VcsKind::Git),
                head_ref: Some("abc123".into()),
                dirty: false,
            },
            prompt: "scaffold council".into(),
            constraints: vec!["advisory only".into()],
            rubric: CouncilRubric {
                criteria: vec![CouncilCriterion {
                    key: "correctness".into(),
                    weight: 1.0,
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
            contract_ref: None,
            receipt_ref: None,
            research_brief_ref: None,
            created_at: punk_domain::now_rfc3339(),
        }
    }

    #[test]
    fn start_and_complete_persist_council_artifacts_and_events() {
        let root = std::env::temp_dir().join(format!("punk-council-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let service = CouncilService::new(&root);
        let packet = sample_packet();

        let paths = service.start(&packet).unwrap();
        assert!(paths.packet_path.exists());
        assert_eq!(paths.council_dir.file_name().unwrap(), "council_test");

        let record = service.complete(&packet).unwrap();
        assert_eq!(record.id, packet.id);
        assert!(paths.record_path.exists());

        let events_store = EventStore::new(root.join(".punk"));
        let events = events_store.load_all().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, "council.started");
        assert_eq!(events[1].kind, "council.completed");

        let _ = fs::remove_dir_all(&root);
    }
}
