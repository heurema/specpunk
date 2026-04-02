use std::collections::BTreeMap;

use anyhow::{anyhow, Context, Result};
use punk_adapters::council::{
    NormalizedCouncilContextItem, NormalizedCouncilPayload, ProviderAdapter, SlotRunSpec,
};
use punk_domain::council::{
    CouncilPacket, CouncilPhase, CouncilReview, CouncilSlotSpec, ProviderKind,
};
use punk_events::EventStore;
use serde_json::Value;

use crate::anonymize::AnonymizedCouncilProposal;
use crate::events;
use crate::storage::{persist_packet, persist_review, CouncilPaths};

#[derive(Clone)]
pub struct ReviewAdapterBinding<'a> {
    pub provider: ProviderKind,
    pub adapter: &'a dyn ProviderAdapter,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewSlotOutcome {
    pub reviewer_slot_id: String,
    pub provider: ProviderKind,
    pub proposal_label: String,
    pub review_ref: Option<String>,
    pub finish_reason: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewRunResult {
    pub council_id: String,
    pub review_refs: Vec<String>,
    pub reviews: Vec<CouncilReview>,
    pub slot_outcomes: Vec<ReviewSlotOutcome>,
}

pub fn run_reviews(
    repo_root: &std::path::Path,
    events: &EventStore,
    packet: &CouncilPacket,
    proposals: &[AnonymizedCouncilProposal],
    bindings: &[ReviewAdapterBinding<'_>],
) -> Result<ReviewRunResult> {
    let paths = CouncilPaths::new(repo_root, &packet.id);
    let _ = persist_packet(repo_root, &paths, packet)?;

    let mut review_refs = Vec::new();
    let mut reviews = Vec::new();
    let mut slot_outcomes = Vec::new();

    for assignment in &packet.role_assignments {
        let Some(binding) = bindings
            .iter()
            .find(|binding| binding.provider == assignment.provider)
        else {
            for proposal in proposals {
                slot_outcomes.push(ReviewSlotOutcome {
                    reviewer_slot_id: build_review_slot_id(
                        &assignment.provider,
                        &assignment.role,
                        &proposal.label,
                    ),
                    provider: assignment.provider.clone(),
                    proposal_label: proposal.label.clone(),
                    review_ref: None,
                    finish_reason: None,
                    error: Some(format!(
                        "missing adapter binding for provider {}",
                        provider_slug(&assignment.provider)
                    )),
                });
            }
            continue;
        };

        for proposal in proposals {
            let slot = build_review_slot_spec(repo_root, packet, assignment, proposal, &paths)?;
            let run_spec = build_run_spec(packet, &slot, proposal);
            match binding.adapter.run_slot(&run_spec) {
                Ok(raw) => match parse_review_output(packet, &slot, proposal, &raw.output_text) {
                    Ok(review) => {
                        let review_ref = persist_review(repo_root, &paths, &slot.id, &review)?;
                        events::emit_review_written(
                            events,
                            repo_root,
                            packet,
                            &slot.id,
                            &paths.review_path(&slot.id),
                        )?;
                        review_refs.push(review_ref.clone());
                        reviews.push(review);
                        slot_outcomes.push(ReviewSlotOutcome {
                            reviewer_slot_id: slot.id,
                            provider: assignment.provider.clone(),
                            proposal_label: proposal.label.clone(),
                            review_ref: Some(review_ref),
                            finish_reason: raw.finish_reason,
                            error: None,
                        });
                    }
                    Err(err) => {
                        slot_outcomes.push(ReviewSlotOutcome {
                            reviewer_slot_id: slot.id,
                            provider: assignment.provider.clone(),
                            proposal_label: proposal.label.clone(),
                            review_ref: None,
                            finish_reason: raw.finish_reason,
                            error: Some(err.to_string()),
                        });
                    }
                },
                Err(err) => {
                    slot_outcomes.push(ReviewSlotOutcome {
                        reviewer_slot_id: slot.id,
                        provider: assignment.provider.clone(),
                        proposal_label: proposal.label.clone(),
                        review_ref: None,
                        finish_reason: None,
                        error: Some(err.to_string()),
                    });
                }
            }
        }
    }

    Ok(ReviewRunResult {
        council_id: packet.id.clone(),
        review_refs,
        reviews,
        slot_outcomes,
    })
}

fn build_review_slot_spec(
    repo_root: &std::path::Path,
    packet: &CouncilPacket,
    assignment: &punk_domain::council::CouncilRoleAssignment,
    proposal: &AnonymizedCouncilProposal,
    paths: &CouncilPaths,
) -> Result<CouncilSlotSpec> {
    let packet_ref = punk_core::artifacts::relative_ref(repo_root, &paths.packet_path)?;
    Ok(CouncilSlotSpec {
        id: build_review_slot_id(&assignment.provider, &assignment.role, &proposal.label),
        council_id: packet.id.clone(),
        phase: CouncilPhase::Review,
        provider: assignment.provider.clone(),
        model: assignment.model.clone(),
        role: assignment.role.clone(),
        prompt_ref: packet_ref.clone(),
        packet_ref,
        timeout_secs: packet.budget.slot_timeout_secs,
    })
}

fn build_review_slot_id(provider: &ProviderKind, role: &str, proposal_label: &str) -> String {
    format!(
        "review-{}-{}-{}",
        provider_slug(provider),
        role_slug(role),
        proposal_label.to_ascii_lowercase()
    )
}

fn build_run_spec(
    packet: &CouncilPacket,
    slot: &CouncilSlotSpec,
    proposal: &AnonymizedCouncilProposal,
) -> SlotRunSpec {
    SlotRunSpec {
        contract_id: packet
            .contract_ref
            .clone()
            .unwrap_or_else(|| packet.id.clone()),
        slot_name: slot.id.clone(),
        prompt: format!(
            "Council review phase for {}. Review proposal {} against the rubric.",
            packet.id, proposal.label
        ),
        payload: NormalizedCouncilPayload {
            contract_id: packet
                .contract_ref
                .clone()
                .unwrap_or_else(|| packet.id.clone()),
            slot_name: slot.id.clone(),
            objective: packet.prompt.clone(),
            instructions: packet.constraints.clone(),
            context: review_context(packet, slot, proposal),
            expected_outputs: vec!["review payload".to_string()],
            metadata: BTreeMap::from([
                ("council_id".to_string(), packet.id.clone()),
                ("phase".to_string(), "review".to_string()),
                (
                    "provider".to_string(),
                    provider_slug(&slot.provider).to_string(),
                ),
                ("role".to_string(), slot.role.clone()),
                ("proposal_label".to_string(), proposal.label.clone()),
            ]),
        },
        metadata: BTreeMap::from([
            ("slot_id".to_string(), slot.id.clone()),
            ("proposal_label".to_string(), proposal.label.clone()),
        ]),
    }
}

fn review_context(
    packet: &CouncilPacket,
    slot: &CouncilSlotSpec,
    proposal: &AnonymizedCouncilProposal,
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
        NormalizedCouncilContextItem {
            key: "proposal_label".to_string(),
            value: proposal.label.clone(),
        },
        NormalizedCouncilContextItem {
            key: "proposal_summary".to_string(),
            value: proposal.summary.clone(),
        },
    ];
    for finding in &proposal.findings {
        context.push(NormalizedCouncilContextItem {
            key: "proposal_finding".to_string(),
            value: finding.clone(),
        });
    }
    for risk in &proposal.risks {
        context.push(NormalizedCouncilContextItem {
            key: "proposal_risk".to_string(),
            value: risk.clone(),
        });
    }
    for keep in &proposal.must_keep {
        context.push(NormalizedCouncilContextItem {
            key: "proposal_must_keep".to_string(),
            value: keep.clone(),
        });
    }
    for criterion in &packet.rubric.criteria {
        context.push(NormalizedCouncilContextItem {
            key: "rubric_criterion".to_string(),
            value: format!("{}:{}", criterion.key, criterion.weight),
        });
    }
    context
}

fn parse_review_output(
    packet: &CouncilPacket,
    slot: &CouncilSlotSpec,
    proposal: &AnonymizedCouncilProposal,
    output_text: &str,
) -> Result<CouncilReview> {
    let payload: Value = serde_json::from_str(output_text)
        .with_context(|| format!("parse review payload JSON for slot {}", slot.id))?;
    if payload.get("kind").and_then(Value::as_str) != Some("review") {
        return Err(anyhow!("slot {} returned non-review payload", slot.id));
    }
    let review = payload
        .get("review")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("slot {} missing review object", slot.id))?;
    let summary = review
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("slot {} missing review summary", slot.id))?
        .to_string();
    let verdict = review
        .get("verdict")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("slot {} missing review verdict", slot.id))?;

    let findings = std::iter::once(summary)
        .chain(string_list(review.get("notes")))
        .collect::<Vec<_>>();
    let blockers = string_list(review.get("issues"));
    let criterion_scores = packet
        .rubric
        .criteria
        .iter()
        .map(|criterion| {
            (
                criterion.key.clone(),
                score_for_verdict(verdict, blockers.is_empty()),
            )
        })
        .collect::<BTreeMap<_, _>>();

    Ok(CouncilReview {
        council_id: packet.id.clone(),
        reviewer_slot_id: slot.id.clone(),
        proposal_label: proposal.label.clone(),
        criterion_scores,
        findings,
        blockers,
        confidence: 1.0,
    })
}

fn score_for_verdict(verdict: &str, no_blockers: bool) -> u8 {
    match verdict.to_ascii_lowercase().as_str() {
        "approve" if no_blockers => 5,
        "approve" => 4,
        "needs_changes" => 3,
        "reject" => 1,
        _ => 2,
    }
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

    static NEXT_REVIEW_TEST_ID: AtomicU64 = AtomicU64::new(1);

    fn unique_test_root(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            NEXT_REVIEW_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn sample_packet() -> CouncilPacket {
        CouncilPacket {
            id: "council_review_test".into(),
            kind: CouncilKind::Architecture,
            project_id: "specpunk".into(),
            subject: CouncilSubjectRef {
                feature_id: Some("feat_review".into()),
                contract_id: None,
                run_id: None,
                question: Some("how should review orchestration work?".into()),
            },
            repo_snapshot: punk_domain::council::RepoSnapshotRef {
                vcs: None,
                head_ref: None,
                dirty: false,
            },
            prompt: "compare anonymized proposals".into(),
            constraints: vec!["review-only".into()],
            rubric: CouncilRubric {
                criteria: vec![
                    CouncilCriterion {
                        key: "correctness".into(),
                        weight: 1.0,
                    },
                    CouncilCriterion {
                        key: "scope_safety".into(),
                        weight: 0.5,
                    },
                ],
            },
            role_assignments: vec![
                CouncilRoleAssignment {
                    role: "reviewer".into(),
                    provider: ProviderKind::Codex,
                    model: "gpt-5.4".into(),
                },
                CouncilRoleAssignment {
                    role: "reviewer".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: "claude-sonnet".into(),
                },
            ],
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

    fn sample_anonymized(label: &str) -> AnonymizedCouncilProposal {
        AnonymizedCouncilProposal {
            council_id: "council_review_test".into(),
            label: label.into(),
            summary: format!("proposal {label}"),
            findings: vec![format!("finding {label}")],
            risks: vec![format!("risk {label}")],
            must_keep: vec![format!("keep {label}")],
            must_fix: vec![],
            cleanup_obligations: vec![],
            confidence: 0.9,
            content_ref: format!(
                ".punk/council/council_review_test/anonymized-proposals/{label}.json"
            ),
        }
    }

    #[test]
    fn run_reviews_persists_review_artifacts_for_each_reviewer_and_proposal() {
        let root = unique_test_root("punk-council-review");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let packet = sample_packet();
        let proposals = vec![sample_anonymized("A"), sample_anonymized("B")];
        let codex = FakeCouncilAdapter::named("fake-codex", FakeCouncilMode::ValidReview);
        let claude = FakeCouncilAdapter::named("fake-claude", FakeCouncilMode::ValidReview);
        let bindings = vec![
            ReviewAdapterBinding {
                provider: ProviderKind::Codex,
                adapter: &codex,
            },
            ReviewAdapterBinding {
                provider: ProviderKind::ClaudeCode,
                adapter: &claude,
            },
        ];
        let events = EventStore::new(root.join(".punk"));

        let result = run_reviews(&root, &events, &packet, &proposals, &bindings).unwrap();

        assert_eq!(result.reviews.len(), 4);
        assert_eq!(result.review_refs.len(), 4);
        assert!(result
            .slot_outcomes
            .iter()
            .all(|outcome| outcome.error.is_none()));
        assert!(result
            .reviews
            .iter()
            .all(|review| review.criterion_scores.get("correctness") == Some(&5)));
        assert!(root
            .join(".punk/council/council_review_test/reviews/review-codex-reviewer-a.json")
            .exists());
        assert!(root
            .join(".punk/council/council_review_test/reviews/review-claude-code-reviewer-b.json")
            .exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_reviews_reports_missing_adapter_bindings() {
        let root = unique_test_root("punk-council-review-missing");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let packet = sample_packet();
        let proposals = vec![sample_anonymized("A")];
        let codex = FakeCouncilAdapter::named("fake-codex", FakeCouncilMode::ValidReview);
        let bindings = vec![ReviewAdapterBinding {
            provider: ProviderKind::Codex,
            adapter: &codex,
        }];
        let events = EventStore::new(root.join(".punk"));

        let result = run_reviews(&root, &events, &packet, &proposals, &bindings).unwrap();

        assert_eq!(result.reviews.len(), 1);
        assert_eq!(result.slot_outcomes.len(), 2);
        assert!(result.slot_outcomes.iter().any(|outcome| outcome
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("missing adapter binding")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_reviews_captures_malformed_review_payloads_as_slot_errors() {
        let root = unique_test_root("punk-council-review-malformed");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let packet = sample_packet();
        let proposals = vec![sample_anonymized("A")];
        let adapter = FakeCouncilAdapter::named("fake-codex", FakeCouncilMode::MalformedReview);
        let bindings = vec![ReviewAdapterBinding {
            provider: ProviderKind::Codex,
            adapter: &adapter,
        }];
        let events = EventStore::new(root.join(".punk"));

        let result = run_reviews(&root, &events, &packet, &proposals, &bindings).unwrap();

        assert!(result.reviews.is_empty());
        assert!(result.review_refs.is_empty());
        assert!(result
            .slot_outcomes
            .iter()
            .any(|outcome| outcome.error.is_some()));

        let _ = fs::remove_dir_all(&root);
    }
}
