use std::path::Path;

use anyhow::{anyhow, bail, Result};
use punk_core::artifacts::relative_ref;
use punk_domain::council::{CouncilProposal, ProviderKind};
use serde::{Deserialize, Serialize};

use crate::storage::{persist_anonymized_proposal, persist_proposal_label_map, CouncilPaths};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnonymizedCouncilProposal {
    pub council_id: String,
    pub label: String,
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
pub struct ProposalLabelMapEntry {
    pub label: String,
    pub slot_id: String,
    pub provider: ProviderKind,
    pub model: String,
    pub role: String,
    pub proposal_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProposalLabelMapArtifact {
    pub council_id: String,
    pub entries: Vec<ProposalLabelMapEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PersistedAnonymizedProposal {
    pub proposal_ref: String,
    pub proposal: AnonymizedCouncilProposal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnonymizationBatchResult {
    pub council_id: String,
    pub proposals: Vec<PersistedAnonymizedProposal>,
    pub label_map_ref: String,
    pub label_map: ProposalLabelMapArtifact,
}

pub fn anonymize_proposals(
    repo_root: &Path,
    council_id: &str,
    proposals: &[CouncilProposal],
) -> Result<AnonymizationBatchResult> {
    let paths = CouncilPaths::new(repo_root, council_id);
    let mut anonymized = Vec::with_capacity(proposals.len());
    let mut label_map_entries = Vec::with_capacity(proposals.len());

    for (index, proposal) in proposals.iter().enumerate() {
        validate_proposal_identity(council_id, proposal)?;

        let label = review_label(index);
        let proposal_ref = relative_ref(repo_root, &paths.anonymized_proposal_path(&label))?;
        let proposal_artifact = AnonymizedCouncilProposal {
            council_id: council_id.to_string(),
            label: label.clone(),
            summary: proposal.summary.clone(),
            findings: proposal.findings.clone(),
            risks: proposal.risks.clone(),
            must_keep: proposal.must_keep.clone(),
            must_fix: proposal.must_fix.clone(),
            cleanup_obligations: proposal.cleanup_obligations.clone(),
            confidence: proposal.confidence,
            content_ref: proposal_ref.clone(),
        };
        let persisted_ref =
            persist_anonymized_proposal(repo_root, &paths, &label, &proposal_artifact)?;
        label_map_entries.push(ProposalLabelMapEntry {
            label: label.clone(),
            slot_id: proposal.slot_id.clone(),
            provider: proposal.provider.clone(),
            model: proposal.model.clone(),
            role: proposal.role.clone(),
            proposal_ref: proposal.content_ref.clone(),
        });
        anonymized.push(PersistedAnonymizedProposal {
            proposal_ref: persisted_ref,
            proposal: proposal_artifact,
        });
    }

    let label_map = ProposalLabelMapArtifact {
        council_id: council_id.to_string(),
        entries: label_map_entries,
    };
    let label_map_ref = persist_proposal_label_map(repo_root, &paths, &label_map)?;

    Ok(AnonymizationBatchResult {
        council_id: council_id.to_string(),
        proposals: anonymized,
        label_map_ref,
        label_map,
    })
}

fn validate_proposal_identity(council_id: &str, proposal: &CouncilProposal) -> Result<()> {
    if proposal.council_id != council_id {
        bail!(
            "proposal {} belongs to council {}, expected {}",
            proposal.slot_id,
            proposal.council_id,
            council_id
        );
    }
    if proposal.content_ref.trim().is_empty() {
        return Err(anyhow!(
            "proposal {} is missing persisted content_ref",
            proposal.slot_id
        ));
    }
    Ok(())
}

fn review_label(index: usize) -> String {
    let mut value = index;
    let mut label = String::new();
    loop {
        let remainder = value % 26;
        label.insert(0, char::from(b'A' + remainder as u8));
        if value < 26 {
            return label;
        }
        value = (value / 26) - 1;
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_ANONYMIZE_TEST_ID: AtomicU64 = AtomicU64::new(1);

    fn unique_test_root(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            NEXT_ANONYMIZE_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn sample_proposal(
        slot_id: &str,
        proposal_ref: &str,
        provider: ProviderKind,
    ) -> CouncilProposal {
        CouncilProposal {
            council_id: "council_anonymize_test".into(),
            slot_id: slot_id.into(),
            provider,
            model: "model-x".into(),
            role: "proposer".into(),
            label: None,
            summary: "summary".into(),
            findings: vec!["finding".into()],
            risks: vec!["risk".into()],
            must_keep: vec!["keep".into()],
            must_fix: vec!["fix".into()],
            cleanup_obligations: vec!["cleanup".into()],
            confidence: 0.8,
            content_ref: proposal_ref.into(),
        }
    }

    #[test]
    fn anonymize_proposals_persists_anonymized_artifacts_and_internal_label_map() {
        let root = unique_test_root("punk-council-anonymize");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let proposals = vec![
            sample_proposal(
                "proposal-1",
                ".punk/council/council_anonymize_test/proposals/proposal-1.json",
                ProviderKind::Codex,
            ),
            sample_proposal(
                "proposal-2",
                ".punk/council/council_anonymize_test/proposals/proposal-2.json",
                ProviderKind::ClaudeCode,
            ),
            sample_proposal(
                "proposal-3",
                ".punk/council/council_anonymize_test/proposals/proposal-3.json",
                ProviderKind::Gemini,
            ),
        ];

        let result = anonymize_proposals(&root, "council_anonymize_test", &proposals).unwrap();

        assert_eq!(result.proposals.len(), 3);
        assert_eq!(
            result
                .proposals
                .iter()
                .map(|proposal| proposal.proposal.label.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B", "C"]
        );
        assert_eq!(
            result
                .proposals
                .iter()
                .map(|proposal| proposal.proposal.content_ref.as_str())
                .collect::<Vec<_>>(),
            vec![
                ".punk/council/council_anonymize_test/anonymized-proposals/A.json",
                ".punk/council/council_anonymize_test/anonymized-proposals/B.json",
                ".punk/council/council_anonymize_test/anonymized-proposals/C.json",
            ]
        );
        assert_eq!(
            result.label_map_ref,
            ".punk/council/council_anonymize_test/internal/proposal-label-map.json"
        );
        assert_eq!(
            result
                .label_map
                .entries
                .iter()
                .map(|entry| entry.proposal_ref.as_str())
                .collect::<Vec<_>>(),
            vec![
                ".punk/council/council_anonymize_test/proposals/proposal-1.json",
                ".punk/council/council_anonymize_test/proposals/proposal-2.json",
                ".punk/council/council_anonymize_test/proposals/proposal-3.json",
            ]
        );

        let anonymized_a = fs::read_to_string(
            root.join(".punk/council/council_anonymize_test/anonymized-proposals/A.json"),
        )
        .unwrap();
        assert!(anonymized_a.contains("\"label\": \"A\""));
        assert!(!anonymized_a.contains("proposal-1"));
        assert!(!anonymized_a.contains("provider"));
        assert!(!anonymized_a.contains("role"));

        let label_map = fs::read_to_string(
            root.join(".punk/council/council_anonymize_test/internal/proposal-label-map.json"),
        )
        .unwrap();
        assert!(label_map.contains("\"slot_id\": \"proposal-1\""));
        assert!(label_map.contains(
            "\"proposal_ref\": \".punk/council/council_anonymize_test/proposals/proposal-1.json\""
        ));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn review_labels_stay_stable_past_single_letter_range() {
        assert_eq!(review_label(0), "A");
        assert_eq!(review_label(25), "Z");
        assert_eq!(review_label(26), "AA");
        assert_eq!(review_label(27), "AB");
        assert_eq!(review_label(51), "AZ");
        assert_eq!(review_label(52), "BA");
        assert_eq!(review_label(701), "ZZ");
        assert_eq!(review_label(702), "AAA");
    }

    #[test]
    fn anonymize_proposals_rejects_unpersisted_inputs() {
        let root = unique_test_root("punk-council-anonymize-missing-ref");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let proposals = vec![sample_proposal("proposal-1", "", ProviderKind::Codex)];
        let error = anonymize_proposals(&root, "council_anonymize_test", &proposals).unwrap_err();
        assert!(error.to_string().contains("missing persisted content_ref"));

        let _ = fs::remove_dir_all(&root);
    }
}
