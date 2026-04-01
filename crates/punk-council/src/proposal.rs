use std::collections::BTreeMap;

use anyhow::{anyhow, Context, Result};
use punk_adapters::council::{
    NormalizedCouncilContextItem, NormalizedCouncilPayload, ProviderAdapter, SlotRunSpec,
};
use punk_core::artifacts::relative_ref;
use punk_domain::council::{
    CouncilPacket, CouncilPhase, CouncilProposal, CouncilSlotSpec, ProviderKind,
};
use punk_events::EventStore;
use serde_json::Value;

use crate::events;
use crate::storage::{persist_packet, persist_proposal, CouncilPaths};

#[derive(Clone)]
pub struct ProposalAdapterBinding<'a> {
    pub provider: ProviderKind,
    pub adapter: &'a dyn ProviderAdapter,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProposalSlotOutcome {
    pub slot_id: String,
    pub provider: ProviderKind,
    pub proposal_ref: Option<String>,
    pub finish_reason: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProposalRunResult {
    pub council_id: String,
    pub proposal_refs: Vec<String>,
    pub proposals: Vec<CouncilProposal>,
    pub slot_outcomes: Vec<ProposalSlotOutcome>,
}

pub fn run_proposals(
    repo_root: &std::path::Path,
    events: &EventStore,
    packet: &CouncilPacket,
    bindings: &[ProposalAdapterBinding<'_>],
) -> Result<ProposalRunResult> {
    let paths = CouncilPaths::new(repo_root, &packet.id);
    let _ = persist_packet(repo_root, &paths, packet)?;

    let mut proposal_refs = Vec::new();
    let mut proposals = Vec::new();
    let mut slot_outcomes = Vec::new();

    for (index, assignment) in packet.role_assignments.iter().enumerate() {
        let slot = build_proposal_slot_spec(repo_root, packet, assignment, index, &paths)?;
        let Some(binding) = bindings
            .iter()
            .find(|binding| binding.provider == assignment.provider)
        else {
            slot_outcomes.push(ProposalSlotOutcome {
                slot_id: slot.id,
                provider: assignment.provider.clone(),
                proposal_ref: None,
                finish_reason: None,
                error: Some(format!(
                    "missing adapter binding for provider {}",
                    provider_slug(&assignment.provider)
                )),
            });
            continue;
        };

        let run_spec = build_run_spec(packet, &slot);
        match binding.adapter.run_slot(&run_spec) {
            Ok(raw) => match parse_proposal_output(packet, assignment, &slot, &raw.output_text) {
                Ok(mut proposal) => {
                    let proposal_ref = relative_ref(repo_root, &paths.proposal_path(&slot.id))?;
                    proposal.content_ref = proposal_ref.clone();
                    let proposal_ref = persist_proposal(repo_root, &paths, &slot.id, &proposal)?;
                    events::emit_proposal_written(
                        events,
                        repo_root,
                        packet,
                        &slot.id,
                        &paths.proposal_path(&slot.id),
                    )?;
                    proposal_refs.push(proposal_ref.clone());
                    proposals.push(proposal);
                    slot_outcomes.push(ProposalSlotOutcome {
                        slot_id: slot.id,
                        provider: assignment.provider.clone(),
                        proposal_ref: Some(proposal_ref),
                        finish_reason: raw.finish_reason,
                        error: None,
                    });
                }
                Err(err) => {
                    slot_outcomes.push(ProposalSlotOutcome {
                        slot_id: slot.id,
                        provider: assignment.provider.clone(),
                        proposal_ref: None,
                        finish_reason: raw.finish_reason,
                        error: Some(err.to_string()),
                    });
                }
            },
            Err(err) => {
                slot_outcomes.push(ProposalSlotOutcome {
                    slot_id: slot.id,
                    provider: assignment.provider.clone(),
                    proposal_ref: None,
                    finish_reason: None,
                    error: Some(err.to_string()),
                });
            }
        }
    }

    Ok(ProposalRunResult {
        council_id: packet.id.clone(),
        proposal_refs,
        proposals,
        slot_outcomes,
    })
}

fn build_proposal_slot_spec(
    repo_root: &std::path::Path,
    packet: &CouncilPacket,
    assignment: &punk_domain::council::CouncilRoleAssignment,
    index: usize,
    paths: &CouncilPaths,
) -> Result<CouncilSlotSpec> {
    let packet_ref = relative_ref(repo_root, &paths.packet_path)?;
    Ok(CouncilSlotSpec {
        id: format!(
            "proposal-{}-{}-{}",
            index + 1,
            provider_slug(&assignment.provider),
            role_slug(&assignment.role)
        ),
        council_id: packet.id.clone(),
        phase: CouncilPhase::Proposal,
        provider: assignment.provider.clone(),
        model: assignment.model.clone(),
        role: assignment.role.clone(),
        prompt_ref: packet_ref.clone(),
        packet_ref,
        timeout_secs: packet.budget.slot_timeout_secs,
    })
}

fn build_run_spec(packet: &CouncilPacket, slot: &CouncilSlotSpec) -> SlotRunSpec {
    SlotRunSpec {
        contract_id: packet
            .contract_ref
            .clone()
            .unwrap_or_else(|| packet.id.clone()),
        slot_name: slot.id.clone(),
        prompt: format!(
            "Council proposal phase for {}. Role: {}. Objective: {}",
            packet.id, slot.role, packet.prompt
        ),
        payload: NormalizedCouncilPayload {
            contract_id: packet
                .contract_ref
                .clone()
                .unwrap_or_else(|| packet.id.clone()),
            slot_name: slot.id.clone(),
            objective: packet.prompt.clone(),
            instructions: packet.constraints.clone(),
            context: proposal_context(packet, slot),
            expected_outputs: vec!["proposal payload".to_string()],
            metadata: BTreeMap::from([
                ("council_id".to_string(), packet.id.clone()),
                ("phase".to_string(), "proposal".to_string()),
                (
                    "provider".to_string(),
                    provider_slug(&slot.provider).to_string(),
                ),
                ("role".to_string(), slot.role.clone()),
            ]),
        },
        metadata: BTreeMap::from([("slot_id".to_string(), slot.id.clone())]),
    }
}

fn proposal_context(
    packet: &CouncilPacket,
    slot: &CouncilSlotSpec,
) -> Vec<NormalizedCouncilContextItem> {
    let mut context = vec![
        NormalizedCouncilContextItem {
            key: "project_id".to_string(),
            value: packet.project_id.clone(),
        },
        NormalizedCouncilContextItem {
            key: "kind".to_string(),
            value: match packet.kind {
                punk_domain::council::CouncilKind::Architecture => "architecture".to_string(),
                punk_domain::council::CouncilKind::Contract => "contract".to_string(),
                punk_domain::council::CouncilKind::Review => "review".to_string(),
            },
        },
        NormalizedCouncilContextItem {
            key: "role".to_string(),
            value: slot.role.clone(),
        },
    ];
    if let Some(question) = &packet.subject.question {
        context.push(NormalizedCouncilContextItem {
            key: "question".to_string(),
            value: question.clone(),
        });
    }
    if let Some(feature_id) = &packet.subject.feature_id {
        context.push(NormalizedCouncilContextItem {
            key: "feature_id".to_string(),
            value: feature_id.clone(),
        });
    }
    if let Some(contract_id) = &packet.subject.contract_id {
        context.push(NormalizedCouncilContextItem {
            key: "contract_id".to_string(),
            value: contract_id.clone(),
        });
    }
    if let Some(run_id) = &packet.subject.run_id {
        context.push(NormalizedCouncilContextItem {
            key: "run_id".to_string(),
            value: run_id.clone(),
        });
    }
    context
}

fn parse_proposal_output(
    packet: &CouncilPacket,
    assignment: &punk_domain::council::CouncilRoleAssignment,
    slot: &CouncilSlotSpec,
    output_text: &str,
) -> Result<CouncilProposal> {
    let payload: Value = serde_json::from_str(output_text)
        .with_context(|| format!("parse proposal payload JSON for slot {}", slot.id))?;
    if payload.get("kind").and_then(Value::as_str) != Some("proposal") {
        return Err(anyhow!("slot {} returned non-proposal payload", slot.id));
    }
    let proposal = payload
        .get("proposal")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("slot {} missing proposal object", slot.id))?;
    let summary = proposal
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("slot {} missing proposal summary", slot.id))?
        .to_string();

    Ok(CouncilProposal {
        council_id: packet.id.clone(),
        slot_id: slot.id.clone(),
        provider: assignment.provider.clone(),
        model: assignment.model.clone(),
        role: assignment.role.clone(),
        label: None,
        summary,
        findings: string_list(proposal.get("rationale")),
        risks: Vec::new(),
        must_keep: string_list(proposal.get("changes")),
        must_fix: Vec::new(),
        cleanup_obligations: Vec::new(),
        confidence: 1.0,
        content_ref: String::new(),
    })
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn provider_slug(provider: &ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Codex => "codex",
        ProviderKind::ClaudeCode => "claude-code",
        ProviderKind::Gemini => "gemini",
    }
}

fn role_slug(role: &str) -> String {
    role.chars()
        .map(|ch| match ch {
            'a'..='z' | '0'..='9' => ch,
            'A'..='Z' => ch.to_ascii_lowercase(),
            _ => '-',
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}
#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use punk_adapters::council::{FakeCouncilAdapter, FakeCouncilMode};
    use punk_domain::council::{
        CouncilBudget, CouncilCriterion, CouncilKind, CouncilRoleAssignment, CouncilRubric,
        CouncilSubjectRef,
    };

    use super::*;

    static NEXT_PROPOSAL_TEST_ID: AtomicU64 = AtomicU64::new(1);

    fn unique_test_root(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            NEXT_PROPOSAL_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn sample_packet() -> CouncilPacket {
        CouncilPacket {
            id: "council_proposal_test".into(),
            kind: CouncilKind::Architecture,
            project_id: "specpunk".into(),
            subject: CouncilSubjectRef {
                feature_id: Some("feat_proposal".into()),
                contract_id: None,
                run_id: None,
                question: Some("how should proposal orchestration work?".into()),
            },
            repo_snapshot: punk_domain::council::RepoSnapshotRef {
                vcs: None,
                head_ref: None,
                dirty: false,
            },
            prompt: "compare proposal strategies".into(),
            constraints: vec!["proposal-only".into()],
            rubric: CouncilRubric {
                criteria: vec![CouncilCriterion {
                    key: "correctness".into(),
                    weight: 1.0,
                }],
            },
            role_assignments: vec![
                CouncilRoleAssignment {
                    role: "proposer".into(),
                    provider: ProviderKind::Codex,
                    model: "gpt-5.4".into(),
                },
                CouncilRoleAssignment {
                    role: "proposer".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: "claude-sonnet".into(),
                },
                CouncilRoleAssignment {
                    role: "proposer".into(),
                    provider: ProviderKind::Gemini,
                    model: "gemini-pro".into(),
                },
            ],
            budget: CouncilBudget {
                proposal_slots: 3,
                review_slots: 0,
                slot_timeout_secs: 60,
                max_total_duration_secs: 180,
            },
            contract_ref: Some("contracts/feat_proposal/v1.json".into()),
            receipt_ref: None,
            research_brief_ref: None,
            created_at: punk_domain::now_rfc3339(),
        }
    }

    #[test]
    fn run_proposals_persists_successful_artifacts_and_slot_failures() {
        let root = unique_test_root("punk-council-proposals");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let packet = sample_packet();
        let codex = FakeCouncilAdapter::named("fake-codex", FakeCouncilMode::ValidProposal);
        let claude = FakeCouncilAdapter::named("fake-claude", FakeCouncilMode::MalformedProposal);
        let gemini = FakeCouncilAdapter::named("fake-gemini", FakeCouncilMode::Timeout);
        let bindings = vec![
            ProposalAdapterBinding {
                provider: ProviderKind::Codex,
                adapter: &codex,
            },
            ProposalAdapterBinding {
                provider: ProviderKind::ClaudeCode,
                adapter: &claude,
            },
            ProposalAdapterBinding {
                provider: ProviderKind::Gemini,
                adapter: &gemini,
            },
        ];
        let events = EventStore::new(root.join(".punk"));

        let result = run_proposals(&root, &events, &packet, &bindings).unwrap();

        assert_eq!(result.proposals.len(), 1);
        assert_eq!(result.proposal_refs.len(), 1);
        assert_eq!(result.slot_outcomes.len(), 3);
        assert!(result
            .slot_outcomes
            .iter()
            .any(|outcome| outcome.proposal_ref.is_some()));
        assert!(result.slot_outcomes.iter().any(|outcome| outcome
            .error
            .as_deref()
            .is_some_and(|message| message.contains("non-proposal")
                || message.contains("parse proposal payload"))));
        assert!(result.slot_outcomes.iter().any(|outcome| outcome
            .error
            .as_deref()
            .is_some_and(|message| message.contains("timeout"))));

        let persisted = root
            .join(".punk/council/council_proposal_test/proposals")
            .read_dir()
            .unwrap()
            .count();
        assert_eq!(persisted, 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_proposals_rejects_review_payload_for_proposal_slot() {
        let root = unique_test_root("punk-council-proposals-review");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let mut packet = sample_packet();
        packet.role_assignments.truncate(1);
        let review_adapter = FakeCouncilAdapter::named("fake-review", FakeCouncilMode::ValidReview);
        let bindings = vec![ProposalAdapterBinding {
            provider: ProviderKind::Codex,
            adapter: &review_adapter,
        }];
        let events = EventStore::new(root.join(".punk"));

        let result = run_proposals(&root, &events, &packet, &bindings).unwrap();
        assert!(result.proposals.is_empty());
        assert_eq!(result.slot_outcomes.len(), 1);
        assert!(result.slot_outcomes[0]
            .error
            .as_deref()
            .is_some_and(|message| message.contains("non-proposal payload")));

        let _ = fs::remove_dir_all(&root);
    }
}
