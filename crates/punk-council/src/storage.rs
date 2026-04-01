use std::path::{Path, PathBuf};

use anyhow::Result;
use punk_core::artifacts::{relative_ref, write_json};
use punk_domain::council::{CouncilPacket, CouncilProposal, CouncilRecord, CouncilSynthesis};

use crate::anonymize::{AnonymizedCouncilProposal, ProposalLabelMapArtifact};

#[derive(Debug, Clone)]
pub struct CouncilPaths {
    pub council_dir: PathBuf,
    pub internal_dir: PathBuf,
    pub packet_path: PathBuf,
    pub record_path: PathBuf,
    pub scoreboard_path: PathBuf,
    pub synthesis_path: PathBuf,
    pub proposals_dir: PathBuf,
    pub anonymized_proposals_dir: PathBuf,
    pub proposal_label_map_path: PathBuf,
    pub reviews_dir: PathBuf,
}

impl CouncilPaths {
    pub fn new(repo_root: &Path, council_id: &str) -> Self {
        let council_dir = repo_root.join(".punk").join("council").join(council_id);
        let internal_dir = council_dir.join("internal");
        Self {
            internal_dir: internal_dir.clone(),
            packet_path: council_dir.join("packet.json"),
            record_path: council_dir.join("record.json"),
            scoreboard_path: council_dir.join("scoreboard.json"),
            synthesis_path: council_dir.join("synthesis.json"),
            proposals_dir: council_dir.join("proposals"),
            anonymized_proposals_dir: council_dir.join("anonymized-proposals"),
            proposal_label_map_path: internal_dir.join("proposal-label-map.json"),
            reviews_dir: council_dir.join("reviews"),
            council_dir,
        }
    }

    pub fn proposal_path(&self, slot_id: &str) -> PathBuf {
        self.proposals_dir.join(format!("{slot_id}.json"))
    }

    pub fn anonymized_proposal_path(&self, label: &str) -> PathBuf {
        self.anonymized_proposals_dir.join(format!("{label}.json"))
    }

    pub fn review_path(&self, slot_id: &str) -> PathBuf {
        self.reviews_dir.join(format!("{slot_id}.json"))
    }
}

pub fn persist_packet(
    repo_root: &Path,
    paths: &CouncilPaths,
    packet: &CouncilPacket,
) -> Result<String> {
    write_json(&paths.packet_path, packet)?;
    relative_ref(repo_root, &paths.packet_path)
}

pub fn build_record(
    repo_root: &Path,
    paths: &CouncilPaths,
    packet: &CouncilPacket,
    proposal_refs: &[String],
    review_refs: &[String],
    synthesis_ref: String,
) -> Result<CouncilRecord> {
    Ok(CouncilRecord {
        id: packet.id.clone(),
        packet_ref: relative_ref(repo_root, &paths.packet_path)?,
        proposal_refs: proposal_refs.to_vec(),
        review_refs: review_refs.to_vec(),
        synthesis_ref,
        scoreboard_ref: relative_ref(repo_root, &paths.scoreboard_path)?,
        completed_at: punk_domain::now_rfc3339(),
    })
}

pub fn persist_synthesis(
    repo_root: &Path,
    paths: &CouncilPaths,
    synthesis: &CouncilSynthesis,
) -> Result<String> {
    write_json(&paths.synthesis_path, synthesis)?;
    relative_ref(repo_root, &paths.synthesis_path)
}

pub fn persist_record(
    repo_root: &Path,
    paths: &CouncilPaths,
    packet: &CouncilPacket,
    proposal_refs: &[String],
    review_refs: &[String],
    synthesis_ref: String,
) -> Result<CouncilRecord> {
    let record = build_record(
        repo_root,
        paths,
        packet,
        proposal_refs,
        review_refs,
        synthesis_ref,
    )?;
    write_json(&paths.record_path, &record)?;
    Ok(record)
}

pub fn persist_proposal(
    repo_root: &Path,
    paths: &CouncilPaths,
    slot_id: &str,
    proposal: &CouncilProposal,
) -> Result<String> {
    let path = paths.proposal_path(slot_id);
    write_json(&path, proposal)?;
    relative_ref(repo_root, &path)
}

pub fn persist_anonymized_proposal(
    repo_root: &Path,
    paths: &CouncilPaths,
    label: &str,
    proposal: &AnonymizedCouncilProposal,
) -> Result<String> {
    let path = paths.anonymized_proposal_path(label);
    write_json(&path, proposal)?;
    relative_ref(repo_root, &path)
}

pub fn persist_proposal_label_map(
    repo_root: &Path,
    paths: &CouncilPaths,
    label_map: &ProposalLabelMapArtifact,
) -> Result<String> {
    write_json(&paths.proposal_label_map_path, label_map)?;
    relative_ref(repo_root, &paths.proposal_label_map_path)
}

pub fn persist_review(
    repo_root: &Path,
    paths: &CouncilPaths,
    slot_id: &str,
    review: &punk_domain::council::CouncilReview,
) -> Result<String> {
    let path = paths.review_path(slot_id);
    write_json(&path, review)?;
    relative_ref(repo_root, &path)
}
