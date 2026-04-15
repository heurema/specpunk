use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use punk_core::{
    find_object_path, line_count_for_path, read_json, relative_ref,
    repo_relative_path_is_product_change, scan_forbidden_path_dependency, scope_roots,
    validate_check_command, write_json,
};
use punk_domain::{
    now_rfc3339, ArchitectureAssessment, ArchitectureAssessmentOutcome,
    ArchitectureFileLocAssessment, ArchitectureForbiddenPathDependencyAssessment,
    ArchitectureSeverity, ArchitectureSignals, CheckStatus, CommandEvidence, Contract,
    ContractStatus, Decision, DecisionObject, DeclaredHarnessEvidence, DeterministicStatus,
    EventEnvelope, HarnessEvidence, ModeId, PersistedContract, Receipt, VerificationContext,
    VerificationContextFileState, VerificationContextIdentity,
};
use punk_events::EventStore;
use punk_orch::project_id;
use punk_vcs::detect_backend;

pub struct GateService {
    repo_root: PathBuf,
    events: EventStore,
}

struct CheckRunSummary {
    status: CheckStatus,
    reasons: Vec<String>,
    refs: Vec<String>,
    command_evidence: Vec<CommandEvidence>,
}

struct HarnessRunSummary {
    status: CheckStatus,
    reasons: Vec<String>,
    harness_evidence: Vec<HarnessEvidence>,
}

struct VerificationContextOutcome {
    check_root: Option<PathBuf>,
    context_ref: Option<String>,
    identity: Option<VerificationContextIdentity>,
    capability_resolution_ref: Option<String>,
    capability_resolution_sha256: Option<String>,
    reasons: Vec<String>,
    valid: bool,
}

struct ArchitectureAssessmentArtifact {
    assessment: ArchitectureAssessment,
    assessment_ref: String,
}

impl GateService {
    pub fn new(repo_root: impl AsRef<Path>, global_root: impl AsRef<Path>) -> Self {
        let repo_root = repo_root.as_ref().to_path_buf();
        let events = EventStore::new(global_root.as_ref());
        Self { repo_root, events }
    }

    pub fn gate_run(&self, run_id: &str) -> Result<DecisionObject> {
        let project = project_id(&self.repo_root)?;
        let run_path = find_object_path(&self.repo_root.join(".punk/runs"), run_id)?;
        let receipt_path = run_path
            .parent()
            .ok_or_else(|| anyhow!("invalid run path"))?
            .join("receipt.json");
        let run: punk_domain::Run = read_json(&run_path)?;
        let receipt: Receipt = read_json(&receipt_path)?;
        let contract_path =
            find_object_path(&self.repo_root.join(".punk/contracts"), &run.contract_id)?;
        let persisted_contract: PersistedContract = read_json(&contract_path)?;
        let contract: Contract = persisted_contract.contract.clone();
        if contract.status != ContractStatus::Approved {
            return Err(anyhow!("gate requires an approved contract"));
        }
        let verification_context = resolve_verification_context(&self.repo_root, &run);
        let declared_harness_evidence = load_declared_harness_evidence(&self.repo_root);
        let harness = run_harness_recipes(&self.repo_root)?;
        let architecture = assess_architecture(
            &self.repo_root,
            &run,
            &receipt,
            &contract_path,
            &persisted_contract,
            verification_context.check_root.as_deref(),
        )?;

        let mut decision_basis = Vec::new();
        let mut check_refs = Vec::new();
        let mut command_evidence = Vec::new();
        let receipt_ok = receipt.status == "success"
            && !is_empty_successful_bounded_receipt(&contract, &receipt);
        if !receipt_ok {
            if receipt.status == "success"
                && is_empty_successful_bounded_receipt(&contract, &receipt)
            {
                decision_basis.push(format!(
                    "run receipt reported success without observable repo changes: {}",
                    receipt.summary
                ));
            } else {
                decision_basis.push(format!(
                    "run receipt status is {}: {}",
                    receipt.status, receipt.summary
                ));
            }
        }
        let scope_ok = validate_scope(&contract.allowed_scope, &receipt.changed_files);
        if !scope_ok {
            decision_basis.push("scope violation: changed files outside allowed_scope".to_string());
        }
        decision_basis.extend(verification_context.reasons.clone());
        if let Some(capability_ref) = verification_context.capability_resolution_ref.as_ref() {
            let capability_hash = verification_context
                .capability_resolution_sha256
                .as_deref()
                .unwrap_or("unknown");
            decision_basis.push(format!(
                "capability resolution frozen at {capability_ref} ({capability_hash})"
            ));
        } else if let Some(capability_ref) = persisted_contract.capability_resolution_ref.as_ref() {
            decision_basis.push(format!(
                "capability resolution expected from approved contract at {capability_ref}"
            ));
        }
        let (target, integrity) = if let Some(check_root) = verification_context.check_root.as_ref()
        {
            (
                run_checks(
                    check_root,
                    &self.repo_root,
                    &run.id,
                    "target",
                    &contract.target_checks,
                    &contract.allowed_scope,
                )?,
                run_checks(
                    check_root,
                    &self.repo_root,
                    &run.id,
                    "integrity",
                    &contract.integrity_checks,
                    &contract.allowed_scope,
                )?,
            )
        } else {
            (
                invalid_verification_check_summary("target"),
                invalid_verification_check_summary("integrity"),
            )
        };
        check_refs.extend(target.refs.iter().cloned());
        check_refs.extend(integrity.refs.iter().cloned());
        check_refs.push(architecture.assessment_ref.clone());
        decision_basis.extend(target.reasons.clone());
        decision_basis.extend(integrity.reasons.clone());
        decision_basis.extend(harness.reasons.clone());
        decision_basis.extend(architecture.assessment.reasons.clone());
        command_evidence.extend(target.command_evidence);
        command_evidence.extend(integrity.command_evidence);

        let architecture_blocks = matches!(
            architecture.assessment.outcome,
            ArchitectureAssessmentOutcome::Block
        );
        let architecture_escalates = matches!(
            architecture.assessment.outcome,
            ArchitectureAssessmentOutcome::Escalate
        );

        let (decision, deterministic_status, confidence_estimate) = if architecture_blocks
            || !receipt_ok
            || !scope_ok
            || !verification_context.valid
            || target.status == CheckStatus::Fail
            || integrity.status == CheckStatus::Fail
            || harness.status == CheckStatus::Fail
        {
            (Decision::Block, DeterministicStatus::Fail, 0.9)
        } else if architecture_escalates {
            (Decision::Escalate, DeterministicStatus::Mixed, 0.5)
        } else if target.status == CheckStatus::Pass && integrity.status == CheckStatus::Pass {
            (Decision::Accept, DeterministicStatus::Pass, 1.0)
        } else {
            (Decision::Escalate, DeterministicStatus::Mixed, 0.5)
        };

        let decision_object = DecisionObject {
            id: format!("dec_{}", run.id.trim_start_matches("run_")),
            run_id: run.id.clone(),
            contract_id: contract.id.clone(),
            decision,
            deterministic_status,
            target_status: target.status,
            integrity_status: integrity.status,
            confidence_estimate,
            decision_basis,
            contract_ref: relative_ref(&self.repo_root, &contract_path)?,
            receipt_ref: relative_ref(&self.repo_root, &receipt_path)?,
            check_refs,
            verification_context_ref: verification_context.context_ref,
            verification_context_identity: verification_context.identity,
            command_evidence,
            declared_harness_evidence,
            harness_evidence: harness.harness_evidence,
            created_at: now_rfc3339(),
        };
        let decisions_dir = self.repo_root.join(".punk/decisions");
        fs::create_dir_all(&decisions_dir)?;
        let decision_path = decisions_dir.join(format!("{}.json", decision_object.id));
        write_json(&decision_path, &decision_object)?;
        let event = EventEnvelope {
            event_id: format!("evt_decision_{}", run.id.trim_start_matches("run_")),
            ts: now_rfc3339(),
            project_id: project,
            feature_id: Some(run.feature_id.clone()),
            task_id: Some(run.task_id.clone()),
            run_id: Some(run.id.clone()),
            actor: "gate".to_string(),
            mode: ModeId::Gate,
            kind: "decision.written".to_string(),
            payload_ref: Some(relative_ref(&self.repo_root, &decision_path)?),
            payload_sha256: Some(self.events.file_sha256(&decision_path)?),
        };
        self.events.append(&event)?;
        Ok(decision_object)
    }
}

fn assess_architecture(
    repo_root: &Path,
    run: &punk_domain::Run,
    receipt: &Receipt,
    contract_path: &Path,
    persisted_contract: &PersistedContract,
    check_root: Option<&Path>,
) -> Result<ArchitectureAssessmentArtifact> {
    let signals_ref = persisted_contract
        .architecture_signals_ref
        .clone()
        .or_else(|| default_architecture_signals_ref(repo_root, contract_path));
    let signals = signals_ref
        .as_ref()
        .map(|reference| read_json::<ArchitectureSignals>(&repo_root.join(reference)))
        .transpose()?;
    let severity = signals
        .as_ref()
        .map(|signals| signals.severity.clone())
        .unwrap_or(ArchitectureSeverity::None);
    let integrity = persisted_contract.architecture_integrity.as_ref();

    let touched_files = architecture_changed_files(&receipt.changed_files);
    let touched_roots = scope_roots(&touched_files);
    let touched_root_count = touched_roots.len();

    let mut reasons = Vec::new();
    let mut reason_codes = Vec::new();
    let mut file_loc_results = Vec::new();
    let mut forbidden_path_dependency_results = Vec::new();
    let mut blocked = false;
    let mut escalated = false;

    if matches!(severity, ArchitectureSeverity::Critical) && integrity.is_none() {
        escalated = true;
        reason_codes.push("critical_signals_missing_contract_architecture_integrity".to_string());
        reasons.push(
            "critical architecture signals require contract architecture integrity review, but the approved contract has none".to_string(),
        );
    }

    if let Some(integrity) = integrity {
        if let Some(max) = integrity.touched_roots_max {
            if touched_root_count > max {
                blocked = true;
                reason_codes.push("touched_roots_max_exceeded".to_string());
                reasons.push(format!(
                    "architecture constraint failed: touched roots {} exceed touched_roots_max {}",
                    touched_root_count, max
                ));
            }
        }

        for budget in &integrity.file_loc_budgets {
            let (actual_loc, status, note) = match check_root {
                Some(root) => match line_count_for_path(root, &budget.path) {
                    Ok(actual_loc) if actual_loc > budget.max_after_loc => {
                        blocked = true;
                        reason_codes.push("file_loc_budget_exceeded".to_string());
                        (
                            actual_loc,
                            CheckStatus::Fail,
                            Some(format!(
                                "architecture constraint failed: {} has {} LOC which exceeds the max_after_loc budget {}",
                                budget.path, actual_loc, budget.max_after_loc
                            )),
                        )
                    }
                    Ok(actual_loc) => (actual_loc, CheckStatus::Pass, None),
                    Err(error) => (
                        0,
                        CheckStatus::Unverified,
                        Some(format!(
                            "architecture file LOC budget for {} could not be verified: {}",
                            budget.path, error
                        )),
                    ),
                },
                None => (
                    0,
                    CheckStatus::Unverified,
                    Some(format!(
                        "architecture file LOC budget for {} could not be verified because the frozen verification context is unavailable",
                        budget.path
                    )),
                ),
            };
            if let Some(note) = note {
                reasons.push(note);
            }
            file_loc_results.push(ArchitectureFileLocAssessment {
                path: budget.path.clone(),
                max_after_loc: budget.max_after_loc,
                actual_loc,
                status,
            });
        }

        for rule in &integrity.forbidden_path_dependencies {
            let (status, summary) = match check_root {
                Some(root) => {
                    let scan = scan_forbidden_path_dependency(
                        root,
                        &touched_files,
                        &rule.from_glob,
                        &rule.to_glob,
                    )?;
                    if !scan.violating_edges.is_empty() {
                        blocked = true;
                        reason_codes.push("forbidden_path_dependency_violated".to_string());
                        let edges = scan
                            .violating_edges
                            .iter()
                            .map(|edge| format!("{} -> {}", edge.from_path, edge.to_path))
                            .collect::<Vec<_>>()
                            .join(", ");
                        (
                            CheckStatus::Fail,
                            format!(
                                "architecture constraint failed: forbidden dependency {} -> {} was observed in {}",
                                rule.from_glob, rule.to_glob, edges
                            ),
                        )
                    } else if !scan.unparsed_files.is_empty() {
                        reason_codes.push("forbidden_path_dependency_unverified".to_string());
                        (
                            CheckStatus::Unverified,
                            format!(
                                "forbidden dependency {} -> {} matched files that are not yet deterministically parsed in v0: {}",
                                rule.from_glob,
                                rule.to_glob,
                                scan.unparsed_files.join(", ")
                            ),
                        )
                    } else if scan.matched_files.is_empty() {
                        (
                            CheckStatus::Pass,
                            format!(
                                "no touched files matched forbidden dependency rule {} -> {}",
                                rule.from_glob, rule.to_glob
                            ),
                        )
                    } else {
                        (
                            CheckStatus::Pass,
                            format!(
                                "checked {} touched file(s) for forbidden dependency rule {} -> {}; no violating edges found",
                                scan.matched_files.len(),
                                rule.from_glob,
                                rule.to_glob
                            ),
                        )
                    }
                }
                None => (
                    CheckStatus::Unverified,
                    format!(
                        "forbidden dependency {} -> {} could not be verified because the frozen verification context is unavailable",
                        rule.from_glob, rule.to_glob
                    ),
                ),
            };

            if !matches!(status, CheckStatus::Pass) {
                reasons.push(summary.clone());
            }
            forbidden_path_dependency_results.push(ArchitectureForbiddenPathDependencyAssessment {
                from_glob: rule.from_glob.clone(),
                to_glob: rule.to_glob.clone(),
                status,
                summary,
            });
        }
    }

    let outcome = if blocked {
        ArchitectureAssessmentOutcome::Block
    } else if escalated {
        ArchitectureAssessmentOutcome::Escalate
    } else if signals.is_some() || integrity.is_some() {
        if reasons.is_empty() {
            reasons.push("architecture constraints satisfied".to_string());
        }
        ArchitectureAssessmentOutcome::Pass
    } else {
        ArchitectureAssessmentOutcome::NotApplicable
    };

    let assessment = ArchitectureAssessment {
        run_id: run.id.clone(),
        contract_id: run.contract_id.clone(),
        signals_ref: signals_ref.clone(),
        brief_ref: integrity.map(|integrity| integrity.brief_ref.clone()),
        severity,
        outcome,
        review_required: integrity
            .map(|integrity| integrity.review_required)
            .unwrap_or(matches!(
                signals.as_ref().map(|signals| &signals.severity),
                Some(ArchitectureSeverity::Critical)
            )),
        contract_integrity_present: integrity.is_some(),
        touched_root_count,
        touched_roots,
        file_loc_results,
        forbidden_path_dependency_results,
        reason_codes,
        reasons,
        assessed_at: now_rfc3339(),
    };

    let assessment_path = repo_root
        .join(".punk/runs")
        .join(&run.id)
        .join("architecture-assessment.json");
    write_json(&assessment_path, &assessment)?;

    Ok(ArchitectureAssessmentArtifact {
        assessment,
        assessment_ref: relative_ref(repo_root, &assessment_path)?,
    })
}

fn default_architecture_signals_ref(repo_root: &Path, contract_path: &Path) -> Option<String> {
    let signals_path = contract_path.with_file_name("architecture-signals.json");
    signals_path
        .exists()
        .then(|| relative_ref(repo_root, &signals_path).ok())
        .flatten()
}

fn architecture_changed_files(changed_files: &[String]) -> Vec<String> {
    changed_files
        .iter()
        .filter(|path| repo_relative_path_is_product_change(path))
        .cloned()
        .collect()
}

fn validate_scope(allowed_scope: &[String], changed_files: &[String]) -> bool {
    if allowed_scope.is_empty() {
        return false;
    }
    changed_files.iter().all(|file| {
        if !repo_relative_path_is_product_change(file) {
            return true;
        }
        allowed_scope
            .iter()
            .any(|prefix| file == prefix || file.starts_with(&format!("{prefix}/")))
    })
}

fn is_empty_successful_bounded_receipt(contract: &Contract, receipt: &Receipt) -> bool {
    receipt.status == "success"
        && receipt.changed_files.is_empty()
        && contract_has_non_manifest_entry_points(contract)
        && !receipt
            .summary
            .trim()
            .starts_with("already satisfied in allowed scope before bounded dispatch")
}

fn contract_has_non_manifest_entry_points(contract: &Contract) -> bool {
    contract
        .entry_points
        .iter()
        .any(|entry_point| is_non_manifest_entry_point(entry_point))
}

fn is_non_manifest_entry_point(path: &str) -> bool {
    Path::new(path).extension().is_some()
        && !matches!(
            path,
            "Cargo.toml" | "Cargo.lock" | "README.md" | "rust-toolchain.toml"
        )
}

fn invalid_verification_check_summary(kind: &str) -> CheckRunSummary {
    CheckRunSummary {
        status: CheckStatus::Fail,
        reasons: vec![format!(
            "{kind} checks skipped: frozen verification context unavailable"
        )],
        refs: Vec::new(),
        command_evidence: Vec::new(),
    }
}

fn prune_generated_cargo_lock_if_out_of_scope(
    check_root: &Path,
    allowed_scope: &[String],
    command: &str,
    cargo_lock_existed_before_check: bool,
) -> Result<()> {
    if cargo_lock_existed_before_check
        || allowed_scope.iter().any(|path| path == "Cargo.lock")
        || !command.trim_start().starts_with("cargo ")
    {
        return Ok(());
    }
    let cargo_lock = check_root.join("Cargo.lock");
    if cargo_lock.exists() {
        fs::remove_file(cargo_lock)?;
    }
    Ok(())
}

fn snapshot_verification_file_state(
    workspace_root: &Path,
    path: &str,
    event_store: &EventStore,
) -> Result<VerificationContextFileState> {
    let file_path = workspace_root.join(path);
    if !file_path.exists() {
        return Ok(VerificationContextFileState {
            path: path.to_string(),
            exists: false,
            sha256: None,
        });
    }

    Ok(VerificationContextFileState {
        path: path.to_string(),
        exists: true,
        sha256: Some(event_store.file_sha256(&file_path)?),
    })
}

fn verification_context_fingerprint(
    identity: &VerificationContextIdentity,
    file_states: &[VerificationContextFileState],
) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(format!("backend:{:?}\n", identity.backend));
    hasher.update(format!("workspace_ref:{}\n", identity.workspace_ref));
    hasher.update(format!("change_ref:{}\n", identity.change_ref));
    hasher.update(format!(
        "base_ref:{}\n",
        identity.base_ref.as_deref().unwrap_or("")
    ));
    for path in &identity.changed_files {
        hasher.update(format!("changed:{path}\n"));
    }
    for state in file_states {
        hasher.update(format!(
            "file:{}:{}:{}\n",
            state.path,
            if state.exists { "present" } else { "missing" },
            state.sha256.as_deref().unwrap_or(""),
        ));
    }
    hex::encode(hasher.finalize())
}

fn is_verification_context_runtime_artifact(path: &str) -> bool {
    !repo_relative_path_is_product_change(path)
}

fn normalize_verification_changed_files(
    check_root: &Path,
    current_files: Vec<String>,
    expected_files: &[String],
) -> Vec<String> {
    let mut normalized = BTreeSet::new();
    for path in current_files {
        let trimmed = path.trim_end_matches('/').to_string();
        if check_root.join(&trimmed).is_dir() {
            let mut matched = false;
            let prefix = format!("{trimmed}/");
            for expected in expected_files {
                if expected == &trimmed || expected.starts_with(&prefix) {
                    normalized.insert(expected.clone());
                    matched = true;
                }
            }
            if !matched {
                normalized.insert(trimmed);
            }
        } else {
            normalized.insert(trimmed);
        }
    }
    normalized.into_iter().collect()
}

fn resolve_verification_context(
    repo_root: &Path,
    run: &punk_domain::Run,
) -> VerificationContextOutcome {
    let Some(context_ref) = run.verification_context_ref.clone() else {
        return VerificationContextOutcome {
            check_root: None,
            context_ref: None,
            identity: None,
            capability_resolution_ref: None,
            capability_resolution_sha256: None,
            reasons: vec!["frozen verification context missing from run".to_string()],
            valid: false,
        };
    };

    let context_path = repo_root.join(&context_ref);
    if !context_path.exists() {
        return VerificationContextOutcome {
            check_root: None,
            context_ref: Some(context_ref),
            identity: None,
            capability_resolution_ref: None,
            capability_resolution_sha256: None,
            reasons: vec![format!(
                "frozen verification context missing at {}",
                context_path.display()
            )],
            valid: false,
        };
    }

    let context: VerificationContext = match read_json(&context_path) {
        Ok(context) => context,
        Err(err) => {
            return VerificationContextOutcome {
                check_root: None,
                context_ref: Some(context_ref),
                identity: None,
                capability_resolution_ref: None,
                capability_resolution_sha256: None,
                reasons: vec![format!("unable to read frozen verification context: {err}")],
                valid: false,
            };
        }
    };

    let mut reasons = Vec::new();
    if context.identity.workspace_ref != run.vcs.workspace_ref {
        reasons.push("frozen verification context workspace_ref does not match run".to_string());
    }
    if context.identity.change_ref != run.vcs.change_ref {
        reasons.push("frozen verification context change_ref does not match run".to_string());
    }
    if context.identity.base_ref != run.vcs.base_ref {
        reasons.push("frozen verification context base_ref does not match run".to_string());
    }
    if context.identity.backend != run.vcs.backend {
        reasons.push("frozen verification context backend does not match run".to_string());
    }

    let check_root = PathBuf::from(&context.identity.workspace_ref);
    if !check_root.exists() {
        reasons.push(format!(
            "frozen verification workspace missing: {}",
            check_root.display()
        ));
        return VerificationContextOutcome {
            check_root: None,
            context_ref: Some(context_ref),
            identity: Some(context.identity),
            capability_resolution_ref: context.capability_resolution_ref,
            capability_resolution_sha256: context.capability_resolution_sha256,
            reasons,
            valid: false,
        };
    }

    let backend = match detect_backend(&check_root) {
        Ok(backend) => backend,
        Err(err) => {
            reasons.push(format!(
                "unable to open frozen verification workspace {}: {err}",
                check_root.display()
            ));
            return VerificationContextOutcome {
                check_root: None,
                context_ref: Some(context_ref),
                identity: Some(context.identity),
                capability_resolution_ref: context.capability_resolution_ref,
                capability_resolution_sha256: context.capability_resolution_sha256,
                reasons,
                valid: false,
            };
        }
    };

    if backend.kind() != context.identity.backend {
        reasons.push("frozen verification workspace backend drifted".to_string());
    }
    match backend.current_change_ref() {
        Ok(current_change_ref) if current_change_ref != context.identity.change_ref => {
            reasons.push(format!(
                "frozen verification context drifted: change_ref {} != {}",
                current_change_ref, context.identity.change_ref
            ));
        }
        Err(err) => reasons.push(format!("unable to read frozen change_ref: {err}")),
        _ => {}
    }

    let current_changed_files = match backend.changed_files() {
        Ok(files) => files
            .into_iter()
            .filter(|path| !is_verification_context_runtime_artifact(path))
            .collect::<Vec<_>>(),
        Err(err) => {
            reasons.push(format!("unable to read frozen changed files: {err}"));
            Vec::new()
        }
    };
    let current_changed_files = normalize_verification_changed_files(
        &check_root,
        current_changed_files,
        &context.identity.changed_files,
    );
    if current_changed_files != context.identity.changed_files {
        reasons.push(format!(
            "frozen verification context drifted: changed_files {:?} != {:?}",
            current_changed_files, context.identity.changed_files
        ));
    }

    let event_store = EventStore::new(repo_root);
    let mut current_file_states = Vec::new();
    for state in &context.file_states {
        match snapshot_verification_file_state(&check_root, &state.path, &event_store) {
            Ok(current_state) => {
                if current_state != *state {
                    reasons.push(format!(
                        "frozen verification context drifted at {}",
                        state.path
                    ));
                }
                current_file_states.push(current_state);
            }
            Err(err) => reasons.push(format!(
                "unable to snapshot frozen verification file {}: {err}",
                state.path
            )),
        }
    }
    if verification_context_fingerprint(&context.identity, &context.file_states)
        != context.identity.fingerprint_sha256
    {
        reasons.push("stored frozen verification fingerprint is invalid".to_string());
    }
    if reasons.is_empty()
        && verification_context_fingerprint(&context.identity, &current_file_states)
            != context.identity.fingerprint_sha256
    {
        reasons.push("frozen verification fingerprint drifted".to_string());
    }

    let valid = reasons.is_empty();
    VerificationContextOutcome {
        check_root: valid.then_some(check_root),
        context_ref: Some(context_ref),
        identity: Some(context.identity),
        capability_resolution_ref: context.capability_resolution_ref,
        capability_resolution_sha256: context.capability_resolution_sha256,
        reasons,
        valid,
    }
}

fn run_checks(
    check_root: &Path,
    repo_root: &Path,
    run_id: &str,
    kind: &str,
    commands: &[String],
    allowed_scope: &[String],
) -> Result<CheckRunSummary> {
    if commands.is_empty() {
        return Ok(CheckRunSummary {
            status: CheckStatus::Unverified,
            reasons: vec![format!("{kind} checks missing")],
            refs: Vec::new(),
            command_evidence: Vec::new(),
        });
    }
    let checks_dir = repo_root.join(".punk/runs").join(run_id).join("checks");
    fs::create_dir_all(&checks_dir)?;
    let mut failed = false;
    let mut reasons = Vec::new();
    let mut refs = Vec::new();
    let mut command_evidence = Vec::new();
    for (index, command_str) in commands.iter().enumerate() {
        let stdout_path = checks_dir.join(format!("{}-{:02}.stdout.log", kind, index + 1));
        let stderr_path = checks_dir.join(format!("{}-{:02}.stderr.log", kind, index + 1));

        let args = split_command_args(command_str);
        if args.is_empty() {
            failed = true;
            let summary = format!("{kind} check failed: empty command");
            reasons.push(summary.clone());
            command_evidence.push(CommandEvidence {
                evidence_type: "command".to_string(),
                lane: kind.to_string(),
                command: command_str.clone(),
                status: CheckStatus::Fail,
                summary,
                stdout_ref: None,
                stderr_ref: None,
            });
            continue;
        }

        if let Err(msg) = validate_check_command(check_root, command_str) {
            failed = true;
            let summary = format!("{kind} check failed: invalid command: {msg}");
            reasons.push(summary.clone());
            command_evidence.push(CommandEvidence {
                evidence_type: "command".to_string(),
                lane: kind.to_string(),
                command: command_str.clone(),
                status: CheckStatus::Fail,
                summary,
                stdout_ref: None,
                stderr_ref: None,
            });
            continue;
        }

        let cargo_lock_existed_before_check = check_root.join("Cargo.lock").exists();
        let output = std::process::Command::new(&args[0])
            .args(&args[1..])
            .current_dir(check_root)
            .output()?;
        prune_generated_cargo_lock_if_out_of_scope(
            check_root,
            allowed_scope,
            command_str,
            cargo_lock_existed_before_check,
        )?;

        fs::write(&stdout_path, &output.stdout)?;
        fs::write(&stderr_path, &output.stderr)?;
        let stdout_ref = relative_ref(repo_root, &stdout_path)?;
        let stderr_ref = relative_ref(repo_root, &stderr_path)?;
        refs.push(stdout_ref.clone());
        refs.push(stderr_ref.clone());
        let (status, summary) = if output.status.success() {
            (
                CheckStatus::Pass,
                format!("{kind} check passed: {command_str}"),
            )
        } else {
            failed = true;
            (
                CheckStatus::Fail,
                format!("{kind} check failed: {command_str}"),
            )
        };
        reasons.push(summary.clone());
        command_evidence.push(CommandEvidence {
            evidence_type: "command".to_string(),
            lane: kind.to_string(),
            command: command_str.clone(),
            status,
            summary,
            stdout_ref: Some(stdout_ref),
            stderr_ref: Some(stderr_ref),
        });
    }
    Ok(CheckRunSummary {
        status: if failed {
            CheckStatus::Fail
        } else {
            CheckStatus::Pass
        },
        reasons,
        refs,
        command_evidence,
    })
}

fn load_declared_harness_evidence(repo_root: &Path) -> Vec<DeclaredHarnessEvidence> {
    let harness_spec_path = repo_root.join(".punk/project/harness.json");
    if !harness_spec_path.exists() {
        return Vec::new();
    }
    let spec: serde_json::Value = match read_json(&harness_spec_path) {
        Ok(spec) => spec,
        Err(_) => return Vec::new(),
    };
    let harness_ref = relative_ref(repo_root, &harness_spec_path)
        .unwrap_or_else(|_| ".punk/project/harness.json".to_string());
    let mut pairs = BTreeSet::new();
    let Some(profiles) = spec.get("profiles").and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    for profile in profiles {
        let Some(profile_name) = profile.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(validation_surfaces) = profile
            .get("validation_surfaces")
            .and_then(|value| value.as_array())
        else {
            continue;
        };
        for surface in validation_surfaces {
            let Some(surface_name) = surface.as_str() else {
                continue;
            };
            if surface_name == "command" {
                continue;
            }
            pairs.insert((profile_name.to_string(), surface_name.to_string()));
        }
    }
    pairs
        .into_iter()
        .map(|(profile, evidence_type)| DeclaredHarnessEvidence {
            summary: format!("declared harness surface {evidence_type} from profile {profile}"),
            evidence_type,
            profile,
            source_ref: Some(harness_ref.clone()),
        })
        .collect()
}

fn run_harness_recipes(repo_root: &Path) -> Result<HarnessRunSummary> {
    let harness_spec_path = repo_root.join(".punk/project/harness.json");
    if !harness_spec_path.exists() {
        return Ok(HarnessRunSummary {
            status: CheckStatus::Unverified,
            reasons: Vec::new(),
            harness_evidence: Vec::new(),
        });
    }
    let spec: serde_json::Value = read_json(&harness_spec_path)?;
    let harness_ref = relative_ref(repo_root, &harness_spec_path)
        .unwrap_or_else(|_| ".punk/project/harness.json".to_string());
    let Some(profiles) = spec.get("profiles").and_then(|value| value.as_array()) else {
        return Ok(HarnessRunSummary {
            status: CheckStatus::Unverified,
            reasons: Vec::new(),
            harness_evidence: Vec::new(),
        });
    };

    let mut failed = false;
    let mut executed = false;
    let mut reasons = Vec::new();
    let mut harness_evidence = Vec::new();

    for profile in profiles {
        let Some(profile_name) = profile.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(validation_recipes) = profile
            .get("validation_recipes")
            .and_then(|value| value.as_array())
        else {
            continue;
        };
        for recipe in validation_recipes {
            let Some(kind) = recipe.get("kind").and_then(|value| value.as_str()) else {
                continue;
            };
            if kind != "artifact_assertion" {
                continue;
            }
            let Some(path) = recipe.get("path").and_then(|value| value.as_str()) else {
                continue;
            };
            executed = true;
            let (status, summary, artifact_ref) =
                run_artifact_assertion(repo_root, profile_name, path)?;
            if status == CheckStatus::Fail {
                failed = true;
            }
            reasons.push(summary.clone());
            harness_evidence.push(HarnessEvidence {
                evidence_type: "artifact_assertion".to_string(),
                profile: profile_name.to_string(),
                status,
                summary,
                source_ref: Some(harness_ref.clone()),
                artifact_ref,
            });
        }
    }

    Ok(HarnessRunSummary {
        status: if failed {
            CheckStatus::Fail
        } else if executed {
            CheckStatus::Pass
        } else {
            CheckStatus::Unverified
        },
        reasons,
        harness_evidence,
    })
}

fn run_artifact_assertion(
    repo_root: &Path,
    profile_name: &str,
    path: &str,
) -> Result<(CheckStatus, String, Option<String>)> {
    if !is_safe_repo_relative_path(path) {
        return Ok((
            CheckStatus::Fail,
            format!(
                "artifact_assertion failed for profile {profile_name}: invalid repo-relative path {path}"
            ),
            None,
        ));
    }
    let artifact_path = repo_root.join(path);
    if artifact_path.exists() {
        let artifact_ref = relative_ref(repo_root, &artifact_path).ok();
        Ok((
            CheckStatus::Pass,
            format!("artifact_assertion passed for profile {profile_name}: {path} exists"),
            artifact_ref,
        ))
    } else {
        Ok((
            CheckStatus::Fail,
            format!("artifact_assertion failed for profile {profile_name}: {path} is missing"),
            None,
        ))
    }
}

fn is_safe_repo_relative_path(path: &str) -> bool {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return false;
    }
    candidate.components().all(|component| {
        !matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    })
}

fn split_command_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in s.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use punk_core::write_json;
    use punk_domain::{
        ArchitectureFileLocBudget, ArchitectureSignals, ArchitectureThresholds,
        ContractArchitectureIntegrity, PersistedContract, ReceiptArtifacts, RunStatus, VcsKind,
    };

    fn normalized_decision_value(decision: &punk_domain::DecisionObject) -> serde_json::Value {
        let mut value = serde_json::to_value(decision).unwrap();
        value["created_at"] = serde_json::Value::String("<normalized>".into());
        value
    }

    fn attach_verification_context(
        root: &Path,
        run: &mut punk_domain::Run,
        changed_files: &[&str],
    ) {
        let workspace_root = PathBuf::from(&run.vcs.workspace_ref);
        if let Ok(backend) = detect_backend(&workspace_root) {
            run.vcs.backend = backend.kind();
            if let Ok(change_ref) = backend.current_change_ref() {
                run.vcs.change_ref = change_ref;
            }
        }
        let mut changed_files = changed_files
            .iter()
            .filter(|path| !is_verification_context_runtime_artifact(path))
            .map(|path| path.to_string())
            .collect::<Vec<_>>();
        changed_files.sort();
        changed_files.dedup();
        let event_store = EventStore::new(root);
        let mut file_states = changed_files
            .iter()
            .map(|path| {
                snapshot_verification_file_state(&workspace_root, path, &event_store).unwrap()
            })
            .collect::<Vec<_>>();
        file_states.sort_by(|a, b| a.path.cmp(&b.path));
        let identity = VerificationContextIdentity {
            backend: run.vcs.backend.clone(),
            workspace_ref: run.vcs.workspace_ref.clone(),
            change_ref: run.vcs.change_ref.clone(),
            base_ref: run.vcs.base_ref.clone(),
            changed_files,
            fingerprint_sha256: String::new(),
        };
        let fingerprint_sha256 = verification_context_fingerprint(&identity, &file_states);
        let context = VerificationContext {
            identity: VerificationContextIdentity {
                fingerprint_sha256,
                ..identity
            },
            file_states,
            capability_resolution_ref: None,
            capability_resolution_sha256: None,
            captured_at: now_rfc3339(),
        };
        let context_path = root
            .join(".punk/runs")
            .join(&run.id)
            .join("verification-context.json");
        write_json(&context_path, &context).unwrap();
        run.verification_context_ref = Some(relative_ref(root, &context_path).unwrap());
    }

    fn architecture_test_root(label: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "punk-gate-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        (root, global)
    }

    fn commit_all(root: &Path, message: &str) {
        let add = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(add.status.success(), "git add failed: {:?}", add);
        let commit = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Test User",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                message,
            ])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(commit.status.success(), "git commit failed: {:?}", commit);
    }

    fn write_persisted_contract(
        root: &Path,
        contract: Contract,
        signals: Option<ArchitectureSignals>,
        integrity: Option<ContractArchitectureIntegrity>,
    ) {
        let signals_ref = signals.as_ref().map(|_| {
            let path = root.join(".punk/contracts/feat_1/architecture-signals.json");
            let signals = signals.as_ref().unwrap();
            write_json(&path, signals).unwrap();
            relative_ref(root, &path).unwrap()
        });
        write_json(
            &root.join(".punk/contracts/feat_1/v1.json"),
            &PersistedContract {
                contract,
                architecture_signals_ref: signals_ref,
                architecture_integrity: integrity,
                capability_resolution_ref: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn gate_blocks_scope_violation() {
        let root = std::env::temp_dir().join(format!("punk-gate-{}", std::process::id()));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["allowed.txt".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();
        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        fs::write(root.join("not-allowed.txt"), "scope drift\n").unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec!["not-allowed.txt".into()],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        let changed_files = receipt
            .changed_files
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        attach_verification_context(&root, &mut run, &changed_files);
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();
        assert_eq!(decision.decision, Decision::Block);
        assert_eq!(decision.command_evidence.len(), 2);
        assert!(decision
            .command_evidence
            .iter()
            .all(|item| item.evidence_type == "command"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gate_escalates_when_critical_architecture_review_is_missing() {
        let (root, global) = architecture_test_root("missing-architecture-review");
        fs::write(root.join("tracked.txt"), "tracked\n").unwrap();

        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["tracked.txt".into()],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["tracked.txt".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "medium".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_persisted_contract(
            &root,
            contract,
            Some(ArchitectureSignals {
                contract_id: "ct_1".into(),
                feature_id: "feat_1".into(),
                scope_roots: vec!["tracked.txt".into()],
                oversized_files: vec![punk_domain::ArchitectureOversizedFile {
                    path: "tracked.txt".into(),
                    loc: 1300,
                }],
                distinct_scope_roots: 1,
                entry_point_count: 1,
                expected_interface_count: 1,
                import_path_count: 0,
                has_cleanup_obligations: false,
                has_docs_obligations: false,
                has_migration_sensitive_surfaces: false,
                severity: ArchitectureSeverity::Critical,
                trigger_reasons: vec!["oversized file tracked.txt has 1300 LOC".into()],
                thresholds: ArchitectureThresholds {
                    warn_file_loc: 600,
                    critical_file_loc: 1200,
                    critical_scope_roots: 1,
                    warn_expected_interfaces: 2,
                    warn_import_paths: 5,
                },
                computed_at: now_rfc3339(),
            }),
            None,
        );

        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec!["tracked.txt".into()],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        attach_verification_context(&root, &mut run, &["tracked.txt"]);
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();

        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();
        assert_eq!(decision.decision, Decision::Escalate);
        assert!(decision
            .check_refs
            .iter()
            .any(|reference| reference.ends_with("/architecture-assessment.json")));

        let assessment: ArchitectureAssessment =
            read_json(&root.join(".punk/runs/run_1/architecture-assessment.json")).unwrap();
        assert_eq!(assessment.outcome, ArchitectureAssessmentOutcome::Escalate);
        assert!(assessment
            .reason_codes
            .contains(&"critical_signals_missing_contract_architecture_integrity".to_string()));
    }

    #[test]
    fn gate_blocks_when_architecture_file_loc_budget_is_exceeded() {
        let (root, global) = architecture_test_root("file-loc-budget");
        let oversized = std::iter::repeat("line\n").take(20).collect::<String>();
        fs::write(root.join("tracked.txt"), oversized).unwrap();
        fs::write(
            root.join(".punk/contracts/feat_1/architecture-brief.md"),
            "# brief\n",
        )
        .unwrap();

        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["tracked.txt".into()],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["tracked.txt".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "medium".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_persisted_contract(
            &root,
            contract,
            Some(ArchitectureSignals {
                contract_id: "ct_1".into(),
                feature_id: "feat_1".into(),
                scope_roots: vec!["tracked.txt".into()],
                oversized_files: vec![punk_domain::ArchitectureOversizedFile {
                    path: "tracked.txt".into(),
                    loc: 20,
                }],
                distinct_scope_roots: 1,
                entry_point_count: 1,
                expected_interface_count: 1,
                import_path_count: 0,
                has_cleanup_obligations: false,
                has_docs_obligations: false,
                has_migration_sensitive_surfaces: false,
                severity: ArchitectureSeverity::Warn,
                trigger_reasons: vec!["oversized file tracked.txt has 20 LOC".into()],
                thresholds: ArchitectureThresholds {
                    warn_file_loc: 10,
                    critical_file_loc: 100,
                    critical_scope_roots: 1,
                    warn_expected_interfaces: 2,
                    warn_import_paths: 5,
                },
                computed_at: now_rfc3339(),
            }),
            Some(ContractArchitectureIntegrity {
                review_required: true,
                brief_ref: ".punk/contracts/feat_1/architecture-brief.md".into(),
                touched_roots_max: Some(1),
                file_loc_budgets: vec![ArchitectureFileLocBudget {
                    path: "tracked.txt".into(),
                    max_after_loc: 5,
                }],
                forbidden_path_dependencies: Vec::new(),
            }),
        );

        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec!["tracked.txt".into()],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        attach_verification_context(&root, &mut run, &["tracked.txt"]);
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();

        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();
        assert_eq!(decision.decision, Decision::Block);

        let assessment: ArchitectureAssessment =
            read_json(&root.join(".punk/runs/run_1/architecture-assessment.json")).unwrap();
        assert_eq!(assessment.outcome, ArchitectureAssessmentOutcome::Block);
        assert_eq!(assessment.file_loc_results[0].status, CheckStatus::Fail);
        assert!(assessment
            .reason_codes
            .contains(&"file_loc_budget_exceeded".to_string()));
    }

    #[test]
    fn gate_blocks_when_forbidden_path_dependency_is_violated() {
        let (root, global) = architecture_test_root("forbidden-path-dependency");
        fs::create_dir_all(root.join("crates/app-core/src")).unwrap();
        fs::create_dir_all(root.join("crates/forbidden/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/app-core\", \"crates/forbidden\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/app-core/Cargo.toml"),
            "[package]\nname = \"app-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/forbidden/Cargo.toml"),
            "[package]\nname = \"forbidden\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/app-core/src/lib.rs"),
            "pub fn ready() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/forbidden/src/api.rs"),
            "pub struct Client;\n",
        )
        .unwrap();
        commit_all(&root, "baseline");
        fs::write(
            root.join("crates/app-core/src/lib.rs"),
            "use forbidden::api::Client;\npub fn build() -> Client { todo!() }\n",
        )
        .unwrap();
        fs::write(
            root.join(".punk/contracts/feat_1/architecture-brief.md"),
            "# brief\n",
        )
        .unwrap();

        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["crates/app-core/src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["crates/app-core/src/lib.rs".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "medium".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_persisted_contract(
            &root,
            contract,
            Some(ArchitectureSignals {
                contract_id: "ct_1".into(),
                feature_id: "feat_1".into(),
                scope_roots: vec!["crates".into()],
                oversized_files: Vec::new(),
                distinct_scope_roots: 1,
                entry_point_count: 1,
                expected_interface_count: 1,
                import_path_count: 0,
                has_cleanup_obligations: false,
                has_docs_obligations: false,
                has_migration_sensitive_surfaces: false,
                severity: ArchitectureSeverity::Warn,
                trigger_reasons: vec!["dependency-direction review requested".into()],
                thresholds: ArchitectureThresholds {
                    warn_file_loc: 600,
                    critical_file_loc: 1200,
                    critical_scope_roots: 1,
                    warn_expected_interfaces: 2,
                    warn_import_paths: 5,
                },
                computed_at: now_rfc3339(),
            }),
            Some(ContractArchitectureIntegrity {
                review_required: true,
                brief_ref: ".punk/contracts/feat_1/architecture-brief.md".into(),
                touched_roots_max: Some(1),
                file_loc_budgets: Vec::new(),
                forbidden_path_dependencies: vec![
                    punk_domain::ArchitectureForbiddenPathDependency {
                        from_glob: "crates/app-core/**".into(),
                        to_glob: "crates/forbidden/**".into(),
                    },
                ],
            }),
        );

        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec!["crates/app-core/src/lib.rs".into()],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        attach_verification_context(&root, &mut run, &["crates/app-core/src/lib.rs"]);
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();

        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();
        assert_eq!(decision.decision, Decision::Block);

        let assessment: ArchitectureAssessment =
            read_json(&root.join(".punk/runs/run_1/architecture-assessment.json")).unwrap();
        assert_eq!(assessment.outcome, ArchitectureAssessmentOutcome::Block);
        assert_eq!(
            assessment.forbidden_path_dependency_results[0].status,
            CheckStatus::Fail
        );
        assert!(assessment
            .reason_codes
            .contains(&"forbidden_path_dependency_violated".to_string()));
    }

    #[test]
    fn gate_blocks_successful_noop_bounded_receipt() {
        let root =
            std::env::temp_dir().join(format!("punk-gate-successful-noop-{}", std::process::id()));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "implement demo source change".into(),
            entry_points: vec!["src/lib.rs".into()],
            import_paths: vec!["src/lib.rs".into()],
            expected_interfaces: vec!["demo source edit".into()],
            behavior_requirements: vec!["change source".into()],
            allowed_scope: vec!["src/lib.rs".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();
        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec![],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "PUNK_EXECUTION_COMPLETE: claimed success without edits".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        let changed_files = receipt
            .changed_files
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        attach_verification_context(&root, &mut run, &changed_files);
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();
        assert_eq!(decision.decision, Decision::Block);
        assert!(decision
            .decision_basis
            .iter()
            .any(|reason| reason.contains("reported success without observable repo changes")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gate_persists_declared_non_command_harness_evidence_from_packet() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-gate-harness-declared-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        fs::create_dir_all(root.join(".punk/project")).unwrap();
        fs::create_dir_all(root.join("artifacts")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join("artifacts/result.txt"), "ok\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "artifacts/result.txt"])
            .current_dir(&root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Punk Test",
                "-c",
                "user.email=punk@example.com",
                "commit",
                "-m",
                "baseline",
            ])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(
            root.join(".punk/project/harness.json"),
            r#"{
  "project_id": "demo",
  "inspect_ready": true,
  "bootable_per_workspace": true,
  "capabilities": {
    "ui_legible": true,
    "logs_legible": true,
    "metrics_legible": false,
    "traces_legible": false
  },
  "profiles": [
    {
      "name": "default",
      "validation_surfaces": ["command", "ui_snapshot", "log_query"],
      "validation_recipes": [
        {
          "kind": "artifact_assertion",
          "path": "artifacts/result.txt"
        }
      ]
    }
  ],
  "derivation_source": "repo_markers_v1",
  "updated_at": "2026-04-08T00:00:00Z"
}"#,
        )
        .unwrap();
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["src".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();
        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec![],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        let changed_files = receipt
            .changed_files
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        attach_verification_context(&root, &mut run, &changed_files);
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();
        assert_eq!(decision.decision, Decision::Accept);
        assert_eq!(
            decision.declared_harness_evidence,
            vec![
                DeclaredHarnessEvidence {
                    evidence_type: "log_query".into(),
                    profile: "default".into(),
                    source_ref: Some(".punk/project/harness.json".into()),
                    summary: "declared harness surface log_query from profile default".into(),
                },
                DeclaredHarnessEvidence {
                    evidence_type: "ui_snapshot".into(),
                    profile: "default".into(),
                    source_ref: Some(".punk/project/harness.json".into()),
                    summary: "declared harness surface ui_snapshot from profile default".into(),
                },
            ]
        );
        assert_eq!(
            decision.harness_evidence,
            vec![HarnessEvidence {
                evidence_type: "artifact_assertion".into(),
                profile: "default".into(),
                status: CheckStatus::Pass,
                summary:
                    "artifact_assertion passed for profile default: artifacts/result.txt exists"
                        .into(),
                source_ref: Some(".punk/project/harness.json".into()),
                artifact_ref: Some("artifacts/result.txt".into()),
            }]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn failing_artifact_assertion_blocks_gate() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-gate-harness-artifact-block-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        fs::create_dir_all(root.join(".punk/project")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(
            root.join(".punk/project/harness.json"),
            r#"{
  "project_id": "demo",
  "profiles": [
    {
      "name": "default",
      "validation_surfaces": ["command"],
      "validation_recipes": [
        {
          "kind": "artifact_assertion",
          "path": "artifacts/missing.txt"
        }
      ]
    }
  ]
}"#,
        )
        .unwrap();
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["src".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();
        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec![],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        let changed_files = receipt
            .changed_files
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        attach_verification_context(&root, &mut run, &changed_files);
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();
        assert_eq!(decision.decision, Decision::Block);
        assert_eq!(decision.harness_evidence.len(), 1);
        assert_eq!(decision.harness_evidence[0].status, CheckStatus::Fail);
        assert!(decision.decision_basis.iter().any(|reason| {
            reason.contains(
                "artifact_assertion failed for profile default: artifacts/missing.txt is missing",
            )
        }));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn validate_scope_rejects_string_prefix_escape() {
        assert!(validate_scope(
            &["src/lib.rs".into(), "foo".into()],
            &["src/lib.rs".into(), "foo/bar.rs".into()]
        ));
        assert!(!validate_scope(
            &["src/lib.rs".into()],
            &["src/lib.rs.bak".into()]
        ));
        assert!(!validate_scope(&["foo".into()], &["foobar".into()]));
    }

    #[test]
    fn architecture_changed_files_filters_generated_noise_and_runtime_artifacts() {
        let filtered = architecture_changed_files(&[
            "src/lib.rs".into(),
            ".punk/runs/run_1/run.json".into(),
            ".playwright-mcp/state/session.json".into(),
            "dist/app.js".into(),
            "node_modules/react/index.js".into(),
            ".venv/bin/python".into(),
            ".pytest_cache/v/cache".into(),
            "Packages/App/.build/debug/App".into(),
        ]);
        assert_eq!(filtered, vec!["src/lib.rs".to_string()]);
    }

    #[test]
    fn validate_scope_ignores_controller_runtime_artifacts() {
        assert!(validate_scope(
            &["Cargo.toml".into()],
            &[
                ".punk/runs/run_1/run.json".into(),
                ".punk/runs/run_1/stdout.log".into(),
                ".punk/runs/run_1/stderr.log".into(),
            ]
        ));
        assert!(!validate_scope(
            &["Cargo.toml".into()],
            &[
                ".punk/runs/run_1/stdout.log".into(),
                "not-allowed.txt".into(),
            ]
        ));
    }

    #[test]
    fn validate_scope_ignores_generated_noise_and_runtime_artifacts() {
        assert!(validate_scope(
            &["src/lib.rs".into()],
            &[
                "src/lib.rs".into(),
                "dist/app.js".into(),
                "node_modules/react/index.js".into(),
                ".venv/bin/python".into(),
                ".pytest_cache/v/cache".into(),
                "Packages/App/.build/debug/App".into(),
                ".playwright-mcp/state/session.json".into(),
            ]
        ));
        assert!(!validate_scope(
            &["src/lib.rs".into()],
            &["src/lib.rs".into(), "not-allowed.txt".into()]
        ));
    }

    #[test]
    fn gate_accepts_when_only_runtime_artifacts_changed_outside_scope() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-gate-runtime-artifacts-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["Cargo.toml".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();
        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec![
                ".punk/runs/run_1/run.json".into(),
                ".punk/runs/run_1/stdout.log".into(),
                ".punk/runs/run_1/stderr.log".into(),
                ".playwright-mcp/state/session.json".into(),
                "dist/app.js".into(),
                "node_modules/react/index.js".into(),
                ".venv/bin/python".into(),
                ".pytest_cache/v/cache".into(),
                "Packages/App/.build/debug/App".into(),
            ],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec!["true".into()],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        let changed_files = receipt
            .changed_files
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        attach_verification_context(&root, &mut run, &changed_files);
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();

        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();

        assert_eq!(decision.decision, Decision::Accept);
        assert_eq!(decision.deterministic_status, DeterministicStatus::Pass);
        assert!(!decision
            .decision_basis
            .iter()
            .any(|reason| reason.contains("scope violation")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gate_runs_trusted_checks_in_run_workspace_ref_when_present() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-gate-workspace-ref-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let workspace = root.join("isolated-workspace");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        fs::create_dir_all(workspace.join("crates/demo-cli/src")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&workspace)
            .output()
            .unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/demo-cli\"]\n",
        )
        .unwrap();
        fs::write(
            workspace.join("crates/demo-cli/Cargo.toml"),
            "[package]\nname = \"demo-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            workspace.join("crates/demo-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();

        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "bootstrap".into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec![],
            expected_interfaces: vec!["workspace scaffold".into()],
            behavior_requirements: vec!["bootstrap project".into()],
            allowed_scope: vec!["Cargo.toml".into(), "crates/demo-cli".into()],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();

        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: workspace.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec![
                "Cargo.toml".into(),
                "crates/demo-cli/Cargo.toml".into(),
                "crates/demo-cli/src/main.rs".into(),
            ],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "bootstrap succeeded".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        let changed_files = receipt
            .changed_files
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        attach_verification_context(&root, &mut run, &changed_files);
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();

        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();

        assert_eq!(decision.decision, Decision::Accept);
        assert!(decision
            .decision_basis
            .iter()
            .all(|reason| !reason.contains("failed")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gate_uses_frozen_capability_resolution_even_if_project_index_changes() {
        let (root, global) = architecture_test_root("frozen-capability");
        fs::create_dir_all(root.join(".punk/project")).unwrap();
        fs::write(
            root.join(".punk/project/capabilities.json"),
            "{ \"active\": [] }\n",
        )
        .unwrap();
        fs::write(root.join("tracked.txt"), "ok\n").unwrap();
        commit_all(&root, "initial");
        fs::write(root.join("tracked.txt"), "changed\n").unwrap();

        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "stabilize tracked file".into(),
            entry_points: vec!["tracked.txt".into()],
            import_paths: vec![],
            expected_interfaces: vec!["tracked file stays stable".into()],
            behavior_requirements: vec!["keep tracked file intact".into()],
            allowed_scope: vec!["tracked.txt".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        let capability_ref = ".punk/contracts/feat_1/capability-resolution.json".to_string();
        write_json(
            &root.join(&capability_ref),
            &serde_json::json!({
                "schema": "specpunk/contract-capability-resolution/v1",
                "version": 1,
                "contract_id": "ct_1",
                "project_capability_index_ref": ".punk/project/capabilities.json",
                "project_capability_index_sha256": "idx-sha",
                "selected_capabilities": [{"id": "rust-cargo"}],
                "ignore_rules": ["target"],
                "scope_seeds": {"entry_points": ["Cargo.toml"]},
                "target_checks": ["true"],
                "integrity_checks": ["true"],
                "generated_at": now_rfc3339()
            }),
        )
        .unwrap();
        write_json(
            &root.join(".punk/contracts/feat_1/v1.json"),
            &PersistedContract {
                contract,
                architecture_signals_ref: None,
                architecture_integrity: None,
                capability_resolution_ref: Some(capability_ref.clone()),
            },
        )
        .unwrap();

        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        attach_verification_context(&root, &mut run, &["tracked.txt"]);
        let context_ref = run.verification_context_ref.clone().unwrap();
        let context_path = root.join(&context_ref);
        let mut context: VerificationContext = read_json(&context_path).unwrap();
        context.capability_resolution_ref = Some(capability_ref.clone());
        context.capability_resolution_sha256 = Some("cap-sha".into());
        write_json(&context_path, &context).unwrap();
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        write_json(
            &root.join(".punk/runs/run_1/receipt.json"),
            &Receipt {
                id: "rcpt_1".into(),
                run_id: "run_1".into(),
                task_id: "task_1".into(),
                status: "success".into(),
                executor_name: "test-executor".into(),
                changed_files: vec!["tracked.txt".into()],
                artifacts: ReceiptArtifacts {
                    stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                    stderr_ref: ".punk/runs/run_1/stderr.log".into(),
                },
                checks_run: vec!["true".into()],
                duration_ms: 1,
                cost_usd: None,
                summary: "ok".into(),
                created_at: now_rfc3339(),
            },
        )
        .unwrap();

        fs::write(
            root.join(".punk/project/capabilities.json"),
            "{ \"active\": [{\"id\": \"swiftpm\"}] }\n",
        )
        .unwrap();

        let decision = GateService::new(&root, &global).gate_run("run_1").unwrap();
        assert_eq!(decision.decision, Decision::Accept);
        assert!(decision.decision_basis.iter().any(|reason| {
            reason.contains(
                "capability resolution frozen at .punk/contracts/feat_1/capability-resolution.json",
            )
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gate_replay_preserves_substantive_decision_payload() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("punk-gate-replay-{}-{suffix}", std::process::id()));
        let global = root.join("global");
        let workspace = root.join("isolated-workspace");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        fs::create_dir_all(workspace.join("src")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&workspace)
            .output()
            .unwrap();
        let change_ref = detect_backend(&workspace)
            .unwrap()
            .current_change_ref()
            .unwrap();
        fs::write(workspace.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();

        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "implement demo source change".into(),
            entry_points: vec!["src/lib.rs".into()],
            import_paths: vec!["src/lib.rs".into()],
            expected_interfaces: vec!["demo source edit".into()],
            behavior_requirements: vec!["change source".into()],
            allowed_scope: vec!["src/lib.rs".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();

        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: workspace.display().to_string(),
                change_ref,
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec!["src/lib.rs".into()],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        let changed_files = receipt
            .changed_files
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        attach_verification_context(&root, &mut run, &changed_files);
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();

        let gate = GateService::new(&root, &global);
        let first = gate.gate_run("run_1").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = gate.gate_run("run_1").unwrap();

        assert_ne!(first.created_at, second.created_at);
        assert_eq!(
            normalized_decision_value(&first),
            normalized_decision_value(&second)
        );

        let persisted: punk_domain::DecisionObject =
            read_json(&root.join(".punk/decisions/dec_1.json")).unwrap();
        assert_eq!(
            normalized_decision_value(&second),
            normalized_decision_value(&persisted)
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gate_blocks_when_frozen_verification_context_drifted() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-gate-verification-drift-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let workspace = root.join("isolated-workspace");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        fs::create_dir_all(workspace.join("src")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&workspace)
            .output()
            .unwrap();
        let change_ref = detect_backend(&workspace)
            .unwrap()
            .current_change_ref()
            .unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        fs::write(workspace.join("src/main.rs"), "fn main() {}\n").unwrap();

        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "bootstrap".into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec![],
            expected_interfaces: vec!["workspace scaffold".into()],
            behavior_requirements: vec!["bootstrap project".into()],
            allowed_scope: vec!["Cargo.toml".into(), "src".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "medium".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();

        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: workspace.display().to_string(),
                change_ref,
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec!["Cargo.toml".into(), "src/main.rs".into()],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "bootstrap succeeded".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        let changed_files = receipt
            .changed_files
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        attach_verification_context(&root, &mut run, &changed_files);
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();

        fs::write(
            workspace.join("src/main.rs"),
            "fn main() { println!(\"drift\"); }\n",
        )
        .unwrap();

        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();

        assert_eq!(decision.decision, Decision::Block);
        assert!(decision
            .decision_basis
            .iter()
            .any(|reason| reason.contains("frozen verification context drifted")));
        assert_eq!(
            decision.verification_context_ref.as_deref(),
            run.verification_context_ref.as_deref()
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gate_blocks_failed_receipt_even_when_trusted_checks_pass() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-gate-failed-receipt-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();

        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "bootstrap".into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec![],
            expected_interfaces: vec!["workspace scaffold".into()],
            behavior_requirements: vec!["bootstrap project".into()],
            allowed_scope: vec!["Cargo.toml".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();

        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "failure".into(),
            executor_name: "fake".into(),
            changed_files: vec!["Cargo.toml".into()],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "stalled after no progress".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        let changed_files = receipt
            .changed_files
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        attach_verification_context(&root, &mut run, &changed_files);
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();

        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();

        assert_eq!(decision.decision, Decision::Block);
        assert_eq!(decision.deterministic_status, DeterministicStatus::Fail);
        assert!(decision
            .decision_basis
            .iter()
            .any(|reason| reason.contains("run receipt status is failure")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_generated_cargo_lock_removes_new_out_of_scope_lockfile() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-gate-cargo-lock-prune-{}-{suffix}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Cargo.lock"), "generated\n").unwrap();

        prune_generated_cargo_lock_if_out_of_scope(
            &root,
            &["Cargo.toml".into()],
            "cargo test --workspace",
            false,
        )
        .unwrap();

        assert!(!root.join("Cargo.lock").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_generated_cargo_lock_keeps_preexisting_or_allowed_lockfile() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-gate-cargo-lock-keep-{}-{suffix}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        fs::write(root.join("Cargo.lock"), "existing\n").unwrap();
        prune_generated_cargo_lock_if_out_of_scope(
            &root,
            &["Cargo.toml".into()],
            "cargo test --workspace",
            true,
        )
        .unwrap();
        assert!(root.join("Cargo.lock").exists());

        prune_generated_cargo_lock_if_out_of_scope(
            &root,
            &["Cargo.toml".into(), "Cargo.lock".into()],
            "cargo test --workspace",
            false,
        )
        .unwrap();
        assert!(root.join("Cargo.lock").exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_generated_cargo_lock_for_file_scoped_cargo_checks() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-gate-file-scope-cargo-lock-prune-{}-{suffix}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Cargo.lock"), "generated\n").unwrap();

        prune_generated_cargo_lock_if_out_of_scope(
            &root,
            &["crates/pubpunk-core/src/lib.rs".into()],
            "cargo test -p pubpunk-core",
            false,
        )
        .unwrap();

        assert!(!root.join("Cargo.lock").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gate_blocks_invalid_check_command_without_running_shell_payload() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-gate-invalid-check-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["allowed.txt".into()],
            target_checks: vec!["true; touch hacked".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();
        let mut run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            verification_context_ref: None,
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        fs::write(root.join("allowed.txt"), "ok\n").unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec!["allowed.txt".into()],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        let changed_files = receipt
            .changed_files
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        attach_verification_context(&root, &mut run, &changed_files);
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();

        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();

        assert_eq!(decision.decision, Decision::Block);
        assert_eq!(decision.target_status, CheckStatus::Fail);
        assert!(decision
            .decision_basis
            .iter()
            .any(|reason| reason.contains("invalid command")));
        let invalid_command = decision
            .command_evidence
            .iter()
            .find(|item| item.command == "true; touch hacked")
            .expect("invalid target command evidence");
        assert_eq!(invalid_command.lane, "target");
        assert_eq!(invalid_command.status, CheckStatus::Fail);
        assert!(invalid_command.stdout_ref.is_none());
        assert!(invalid_command.stderr_ref.is_none());
        assert!(!root.join("hacked").exists());

        let _ = fs::remove_dir_all(&root);
    }
}
