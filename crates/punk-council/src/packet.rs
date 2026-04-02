use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use punk_domain::council::{
    CouncilBudget, CouncilCriterion, CouncilKind, CouncilPacket, CouncilRoleAssignment,
    CouncilRubric, CouncilSubjectRef, ProviderKind,
};
use punk_vcs::current_snapshot_ref;

#[derive(Debug, Clone, PartialEq)]
pub struct CouncilPacketInput {
    pub kind: CouncilKind,
    pub project_id: String,
    pub subject: CouncilSubjectRef,
    pub prompt: String,
    pub constraints: Vec<String>,
    pub rubric: CouncilRubric,
    pub role_assignments: Vec<CouncilRoleAssignment>,
    pub budget: CouncilBudget,
    pub contract_ref: Option<String>,
    pub receipt_ref: Option<String>,
    pub research_brief_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArchitecturePacketInput {
    pub project_id: String,
    pub subject: CouncilSubjectRef,
    pub prompt: String,
    pub constraints: Vec<String>,
    pub rubric: Option<CouncilRubric>,
    pub role_assignments: Option<Vec<CouncilRoleAssignment>>,
    pub budget: Option<CouncilBudget>,
    pub contract_ref: Option<String>,
    pub receipt_ref: Option<String>,
    pub research_brief_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContractPacketInput {
    pub project_id: String,
    pub subject: CouncilSubjectRef,
    pub prompt: String,
    pub constraints: Vec<String>,
    pub rubric: Option<CouncilRubric>,
    pub role_assignments: Option<Vec<CouncilRoleAssignment>>,
    pub budget: Option<CouncilBudget>,
    pub contract_ref: Option<String>,
    pub receipt_ref: Option<String>,
    pub research_brief_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewPacketInput {
    pub project_id: String,
    pub subject: CouncilSubjectRef,
    pub prompt: String,
    pub constraints: Vec<String>,
    pub rubric: Option<CouncilRubric>,
    pub role_assignments: Option<Vec<CouncilRoleAssignment>>,
    pub budget: Option<CouncilBudget>,
    pub contract_ref: Option<String>,
    pub receipt_ref: Option<String>,
    pub research_brief_ref: Option<String>,
}

pub fn build_packet(repo_root: &Path, input: CouncilPacketInput) -> Result<CouncilPacket> {
    validate_packet_input(&input)?;
    let repo_snapshot = current_snapshot_ref(repo_root)?;
    Ok(CouncilPacket {
        id: new_council_id(),
        kind: input.kind,
        project_id: input.project_id,
        subject: input.subject,
        repo_snapshot,
        prompt: input.prompt,
        constraints: input.constraints,
        rubric: input.rubric,
        role_assignments: input.role_assignments,
        budget: input.budget,
        contract_ref: input.contract_ref,
        receipt_ref: input.receipt_ref,
        research_brief_ref: input.research_brief_ref,
        created_at: punk_domain::now_rfc3339(),
    })
}

pub fn build_architecture_packet(
    repo_root: &Path,
    input: ArchitecturePacketInput,
) -> Result<CouncilPacket> {
    build_packet(
        repo_root,
        CouncilPacketInput {
            kind: CouncilKind::Architecture,
            project_id: input.project_id,
            subject: input.subject,
            prompt: input.prompt,
            constraints: input.constraints,
            rubric: input.rubric.unwrap_or_else(default_architecture_rubric),
            role_assignments: input
                .role_assignments
                .unwrap_or_else(default_role_assignments),
            budget: input.budget.unwrap_or_else(default_budget),
            contract_ref: input.contract_ref,
            receipt_ref: input.receipt_ref,
            research_brief_ref: input.research_brief_ref,
        },
    )
}

pub fn build_contract_packet(
    repo_root: &Path,
    input: ContractPacketInput,
) -> Result<CouncilPacket> {
    build_packet(
        repo_root,
        CouncilPacketInput {
            kind: CouncilKind::Contract,
            project_id: input.project_id,
            subject: input.subject,
            prompt: input.prompt,
            constraints: input.constraints,
            rubric: input.rubric.unwrap_or_else(default_contract_rubric),
            role_assignments: input
                .role_assignments
                .unwrap_or_else(default_role_assignments),
            budget: input.budget.unwrap_or_else(default_budget),
            contract_ref: input.contract_ref,
            receipt_ref: input.receipt_ref,
            research_brief_ref: input.research_brief_ref,
        },
    )
}

pub fn build_review_packet(repo_root: &Path, input: ReviewPacketInput) -> Result<CouncilPacket> {
    build_packet(
        repo_root,
        CouncilPacketInput {
            kind: CouncilKind::Review,
            project_id: input.project_id,
            subject: input.subject,
            prompt: input.prompt,
            constraints: input.constraints,
            rubric: input.rubric.unwrap_or_else(default_review_rubric),
            role_assignments: input
                .role_assignments
                .unwrap_or_else(default_role_assignments),
            budget: input.budget.unwrap_or_else(default_budget),
            contract_ref: input.contract_ref,
            receipt_ref: input.receipt_ref,
            research_brief_ref: input.research_brief_ref,
        },
    )
}

fn validate_packet_input(input: &CouncilPacketInput) -> Result<()> {
    require_non_empty(&input.project_id, "project id")?;
    require_non_empty(&input.prompt, "prompt")?;
    validate_rubric(&input.rubric)?;
    validate_role_assignments(&input.role_assignments)?;
    validate_budget(&input.budget)?;

    match input.kind {
        CouncilKind::Architecture => {
            require_ref(input.subject.question.as_deref(), "architecture question")?;
        }
        CouncilKind::Contract => {
            require_ref(input.subject.contract_id.as_deref(), "contract subject ref")?;
            require_ref(input.contract_ref.as_deref(), "contract ref")?;
        }
        CouncilKind::Review => {
            require_ref(input.subject.contract_id.as_deref(), "contract subject ref")?;
            require_ref(input.contract_ref.as_deref(), "contract ref")?;
            require_ref(input.subject.run_id.as_deref(), "run subject ref")?;
            require_ref(input.receipt_ref.as_deref(), "run receipt ref")?;
        }
    }

    Ok(())
}

fn validate_rubric(rubric: &CouncilRubric) -> Result<()> {
    if rubric.criteria.is_empty() {
        return Err(anyhow!("rubric criteria must not be empty"));
    }

    for criterion in &rubric.criteria {
        require_non_empty(&criterion.key, "rubric criterion key")?;
        if !criterion.weight.is_finite() || criterion.weight <= 0.0 {
            return Err(anyhow!(
                "rubric criterion `{}` weight must be positive",
                criterion.key
            ));
        }
    }

    Ok(())
}

fn validate_role_assignments(role_assignments: &[CouncilRoleAssignment]) -> Result<()> {
    if role_assignments.is_empty() {
        return Err(anyhow!("role assignments must not be empty"));
    }

    for assignment in role_assignments {
        require_non_empty(&assignment.role, "role assignment role")?;
        require_non_empty(&assignment.model, "role assignment model")?;
    }

    Ok(())
}

fn validate_budget(budget: &CouncilBudget) -> Result<()> {
    if budget.proposal_slots == 0 || budget.review_slots == 0 {
        return Err(anyhow!("budget slots must be greater than zero"));
    }

    if budget.slot_timeout_secs == 0 || budget.max_total_duration_secs == 0 {
        return Err(anyhow!("budget timeouts must be greater than zero"));
    }

    Ok(())
}

fn require_ref(value: Option<&str>, label: &str) -> Result<()> {
    let value = value.ok_or_else(|| anyhow!("{label} is required"))?;
    require_non_empty(value, label)
}

fn require_non_empty(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(anyhow!("{label} must not be empty"));
    }
    Ok(())
}

fn new_council_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("council_{}_{}", millis, std::process::id())
}

fn default_budget() -> CouncilBudget {
    CouncilBudget {
        proposal_slots: 3,
        review_slots: 2,
        slot_timeout_secs: 300,
        max_total_duration_secs: 1800,
    }
}

fn default_role_assignments() -> Vec<CouncilRoleAssignment> {
    vec![
        CouncilRoleAssignment {
            role: "proposer".into(),
            provider: ProviderKind::Codex,
            model: "gpt-5.4".into(),
        },
        CouncilRoleAssignment {
            role: "proposer".into(),
            provider: ProviderKind::ClaudeCode,
            model: "claude-sonnet-4-6".into(),
        },
        CouncilRoleAssignment {
            role: "proposer".into(),
            provider: ProviderKind::Gemini,
            model: "gemini-2.5-pro".into(),
        },
    ]
}

fn criterion(key: &str, weight: f32) -> CouncilCriterion {
    CouncilCriterion {
        key: key.into(),
        weight,
    }
}

fn default_architecture_rubric() -> CouncilRubric {
    CouncilRubric {
        criteria: vec![
            criterion("correctness/completeness", 0.30),
            criterion("scope safety", 0.20),
            criterion("migration realism", 0.15),
            criterion("cleanup coverage", 0.15),
            criterion("operational simplicity", 0.10),
            criterion("reversibility", 0.10),
        ],
    }
}

fn default_contract_rubric() -> CouncilRubric {
    CouncilRubric {
        criteria: vec![
            criterion("explicitness", 0.25),
            criterion("scope boundedness", 0.20),
            criterion("interface clarity", 0.20),
            criterion("check quality", 0.20),
            criterion("cleanup/docs obligations", 0.15),
        ],
    }
}

fn default_review_rubric() -> CouncilRubric {
    CouncilRubric {
        criteria: vec![
            criterion("issue quality", 0.30),
            criterion("correctness of concerns", 0.25),
            criterion("severity calibration", 0.20),
            criterion("coverage", 0.15),
            criterion("actionability", 0.10),
        ],
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use punk_domain::council::{
        CouncilBudget, CouncilKind, CouncilRoleAssignment, CouncilRubric, CouncilSubjectRef,
        ProviderKind,
    };
    use punk_domain::VcsKind;

    static NEXT_TEST_REPO_ID: AtomicU64 = AtomicU64::new(1);

    fn run_ok(dir: &std::path::Path, bin: &str, args: &[&str]) -> anyhow::Result<()> {
        let status = std::process::Command::new(bin)
            .args(args)
            .current_dir(dir)
            .status()?;
        anyhow::ensure!(status.success(), "command failed: {} {:?}", bin, args);
        Ok(())
    }

    fn unique_test_repo_root(prefix: &str) -> PathBuf {
        loop {
            let root = std::env::temp_dir().join(format!(
                "{prefix}-{}-{}",
                std::process::id(),
                NEXT_TEST_REPO_ID.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&root) {
                Ok(()) => return root,
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => panic!("failed to create temp repo root {root:?}: {err}"),
            }
        }
    }

    fn init_git_repo(root: &std::path::Path) {
        run_ok(root, "git", &["init"]).unwrap();
        run_ok(root, "git", &["config", "user.name", "Test User"]).unwrap();
        run_ok(root, "git", &["config", "user.email", "test@example.com"]).unwrap();
        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        run_ok(root, "git", &["add", "tracked.txt"]).unwrap();
        run_ok(root, "git", &["commit", "-m", "init"]).unwrap();
    }

    fn sample_input() -> CouncilPacketInput {
        CouncilPacketInput {
            kind: CouncilKind::Architecture,
            project_id: "specpunk".into(),
            subject: CouncilSubjectRef {
                feature_id: Some("feat_council".into()),
                contract_id: None,
                run_id: None,
                question: Some("how should council packets be initialized?".into()),
            },
            prompt: "compare initialization options".into(),
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
            contract_ref: Some("contracts/feat_council/v1.json".into()),
            receipt_ref: None,
            research_brief_ref: None,
        }
    }

    #[test]
    fn build_packet_populates_id_timestamp_and_repo_snapshot() {
        let root = unique_test_repo_root("punk-council-packet");
        init_git_repo(&root);

        let packet = build_packet(&root, sample_input()).unwrap();
        assert!(packet.id.starts_with("council_"));
        assert_eq!(packet.project_id, "specpunk");
        assert_eq!(packet.repo_snapshot.vcs, Some(VcsKind::Git));
        assert!(packet.repo_snapshot.head_ref.is_some());
        assert!(!packet.repo_snapshot.dirty);
        assert!(!packet.created_at.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    fn make_repo_root() -> std::path::PathBuf {
        let root = unique_test_repo_root("punk-council-packet-family");
        init_git_repo(&root);
        root
    }

    #[test]
    fn architecture_builder_applies_documented_defaults() {
        let root = make_repo_root();
        let packet = build_architecture_packet(
            &root,
            ArchitecturePacketInput {
                project_id: "specpunk".into(),
                subject: CouncilSubjectRef {
                    feature_id: Some("feat_council".into()),
                    contract_id: None,
                    run_id: None,
                    question: Some(
                        "how should architecture council packets be initialized?".into(),
                    ),
                },
                prompt: "compare architecture options".into(),
                constraints: vec!["advisory only".into()],
                rubric: None,
                role_assignments: None,
                budget: None,
                contract_ref: Some("contracts/feat_council/v1.json".into()),
                receipt_ref: None,
                research_brief_ref: None,
            },
        )
        .unwrap();

        assert_eq!(packet.kind, CouncilKind::Architecture);
        assert_eq!(packet.rubric, default_architecture_rubric());
        assert_eq!(packet.role_assignments, default_role_assignments());
        assert_eq!(packet.budget, default_budget());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn contract_builder_preserves_explicit_overrides() {
        let root = make_repo_root();
        let custom_rubric = CouncilRubric {
            criteria: vec![CouncilCriterion {
                key: "custom".into(),
                weight: 1.0,
            }],
        };
        let custom_roles = vec![CouncilRoleAssignment {
            role: "proposer".into(),
            provider: ProviderKind::Codex,
            model: "gpt-5.4-mini".into(),
        }];
        let custom_budget = CouncilBudget {
            proposal_slots: 1,
            review_slots: 1,
            slot_timeout_secs: 60,
            max_total_duration_secs: 120,
        };

        let packet = build_contract_packet(
            &root,
            ContractPacketInput {
                project_id: "specpunk".into(),
                subject: CouncilSubjectRef {
                    feature_id: Some("feat_contract".into()),
                    contract_id: Some("ct_1".into()),
                    run_id: None,
                    question: Some("is the contract bounded?".into()),
                },
                prompt: "review contract quality".into(),
                constraints: vec!["focus on checks".into()],
                rubric: Some(custom_rubric.clone()),
                role_assignments: Some(custom_roles.clone()),
                budget: Some(custom_budget.clone()),
                contract_ref: Some("contracts/feat_contract/v1.json".into()),
                receipt_ref: None,
                research_brief_ref: None,
            },
        )
        .unwrap();

        assert_eq!(packet.kind, CouncilKind::Contract);
        assert_eq!(packet.rubric, custom_rubric);
        assert_eq!(packet.role_assignments, custom_roles);
        assert_eq!(packet.budget, custom_budget);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn review_builder_uses_review_family_defaults() {
        let root = make_repo_root();
        let packet = build_review_packet(
            &root,
            ReviewPacketInput {
                project_id: "specpunk".into(),
                subject: CouncilSubjectRef {
                    feature_id: Some("feat_review".into()),
                    contract_id: Some("ct_2".into()),
                    run_id: Some("run_1".into()),
                    question: Some("what should gate block on?".into()),
                },
                prompt: "compare review findings".into(),
                constraints: vec!["focus on blockers".into()],
                rubric: None,
                role_assignments: None,
                budget: None,
                contract_ref: Some("contracts/feat_review/v1.json".into()),
                receipt_ref: Some("runs/run_1/receipt.json".into()),
                research_brief_ref: None,
            },
        )
        .unwrap();

        assert_eq!(packet.kind, CouncilKind::Review);
        assert_eq!(packet.rubric, default_review_rubric());
        assert_eq!(packet.role_assignments, default_role_assignments());
        assert_eq!(packet.budget, default_budget());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn architecture_builder_rejects_empty_question() {
        let root = make_repo_root();
        let err = build_architecture_packet(
            &root,
            ArchitecturePacketInput {
                project_id: "specpunk".into(),
                subject: CouncilSubjectRef {
                    feature_id: Some("feat_council".into()),
                    contract_id: None,
                    run_id: None,
                    question: Some("   ".into()),
                },
                prompt: "compare architecture options".into(),
                constraints: vec!["advisory only".into()],
                rubric: None,
                role_assignments: None,
                budget: None,
                contract_ref: Some("contracts/feat_council/v1.json".into()),
                receipt_ref: None,
                research_brief_ref: None,
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("architecture question"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn contract_builder_rejects_missing_contract_refs() {
        let root = make_repo_root();
        let err = build_contract_packet(
            &root,
            ContractPacketInput {
                project_id: "specpunk".into(),
                subject: CouncilSubjectRef {
                    feature_id: Some("feat_contract".into()),
                    contract_id: Some("ct_1".into()),
                    run_id: None,
                    question: Some("is the contract bounded?".into()),
                },
                prompt: "review contract quality".into(),
                constraints: vec!["focus on checks".into()],
                rubric: None,
                role_assignments: None,
                budget: None,
                contract_ref: None,
                receipt_ref: None,
                research_brief_ref: None,
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("contract ref"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn review_builder_rejects_missing_run_refs() {
        let root = make_repo_root();
        let err = build_review_packet(
            &root,
            ReviewPacketInput {
                project_id: "specpunk".into(),
                subject: CouncilSubjectRef {
                    feature_id: Some("feat_review".into()),
                    contract_id: Some("ct_2".into()),
                    run_id: Some("run_1".into()),
                    question: Some("what should gate block on?".into()),
                },
                prompt: "compare review findings".into(),
                constraints: vec!["focus on blockers".into()],
                rubric: None,
                role_assignments: None,
                budget: None,
                contract_ref: Some("contracts/feat_review/v1.json".into()),
                receipt_ref: None,
                research_brief_ref: None,
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("run receipt ref"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn contract_builder_rejects_missing_contract_subject_ref() {
        let root = make_repo_root();
        let err = build_contract_packet(
            &root,
            ContractPacketInput {
                project_id: "specpunk".into(),
                subject: CouncilSubjectRef {
                    feature_id: Some("feat_contract".into()),
                    contract_id: None,
                    run_id: None,
                    question: Some("is the contract bounded?".into()),
                },
                prompt: "review contract quality".into(),
                constraints: vec!["focus on checks".into()],
                rubric: None,
                role_assignments: None,
                budget: None,
                contract_ref: Some("contracts/feat_contract/v1.json".into()),
                receipt_ref: None,
                research_brief_ref: None,
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("contract subject ref"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn review_builder_rejects_missing_run_subject_ref() {
        let root = make_repo_root();
        let err = build_review_packet(
            &root,
            ReviewPacketInput {
                project_id: "specpunk".into(),
                subject: CouncilSubjectRef {
                    feature_id: Some("feat_review".into()),
                    contract_id: Some("ct_2".into()),
                    run_id: None,
                    question: Some("what should gate block on?".into()),
                },
                prompt: "compare review findings".into(),
                constraints: vec!["focus on blockers".into()],
                rubric: None,
                role_assignments: None,
                budget: None,
                contract_ref: Some("contracts/feat_review/v1.json".into()),
                receipt_ref: Some("runs/run_1/receipt.json".into()),
                research_brief_ref: None,
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("run subject ref"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn build_packet_rejects_empty_role_assignments() {
        let root = make_repo_root();
        let mut input = sample_input();
        input.role_assignments.clear();

        let err = build_packet(&root, input).unwrap_err();
        assert!(err.to_string().contains("role assignments"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn build_packet_rejects_blank_role_assignment_fields() {
        let root = make_repo_root();
        let mut input = sample_input();
        input.role_assignments[0].role = "   ".into();

        let err = build_packet(&root, input).unwrap_err();
        assert!(err.to_string().contains("role assignment role"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn build_packet_rejects_empty_rubric_criteria() {
        let root = make_repo_root();
        let mut input = sample_input();
        input.rubric.criteria.clear();

        let err = build_packet(&root, input).unwrap_err();
        assert!(err.to_string().contains("rubric criteria"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn build_packet_rejects_blank_rubric_criterion_key() {
        let root = make_repo_root();
        let mut input = sample_input();
        input.rubric.criteria[0].key = "   ".into();

        let err = build_packet(&root, input).unwrap_err();
        assert!(err.to_string().contains("rubric criterion key"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn build_packet_rejects_non_positive_criterion_weight() {
        let root = make_repo_root();
        let mut input = sample_input();
        input.rubric.criteria[0].weight = 0.0;

        let err = build_packet(&root, input).unwrap_err();
        assert!(err.to_string().contains("weight must be positive"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn build_packet_rejects_zero_budget_slots() {
        let root = make_repo_root();
        let mut input = sample_input();
        input.budget.proposal_slots = 0;

        let err = build_packet(&root, input).unwrap_err();
        assert!(err.to_string().contains("budget slots"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn build_packet_rejects_zero_budget_timeouts() {
        let root = make_repo_root();
        let mut input = sample_input();
        input.budget.slot_timeout_secs = 0;

        let err = build_packet(&root, input).unwrap_err();
        assert!(err.to_string().contains("budget timeouts"));

        let _ = fs::remove_dir_all(&root);
    }
}
