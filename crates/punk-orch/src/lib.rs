use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use punk_adapters::{ContractDrafter, ExecuteInput, Executor};
use punk_core::{
    apply_explicit_prompt_overrides, build_bounded_fallback_proposal, canonicalize_draft_proposal,
    scan_repo, validate_draft_proposal,
};
pub use punk_core::{find_object_path, read_json, relative_ref, write_json};
use punk_domain::{
    now_rfc3339, AutonomyOutcome, AutonomyRecord, Contract, ContractStatus, DraftInput,
    DraftProposal, EventEnvelope, Feature, FeatureStatus, ModeId, Project, Receipt,
    ReceiptArtifacts, RefineInput, Run, RunStatus, Task, TaskKind, TaskStatus, VcsKind,
};
use punk_events::EventStore;
use punk_vcs::{current_snapshot_ref, detect_backend};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct RepoPaths {
    pub repo_root: PathBuf,
    pub global_root: PathBuf,
    pub dot_punk: PathBuf,
    pub features_dir: PathBuf,
    pub contracts_dir: PathBuf,
    pub tasks_dir: PathBuf,
    pub runs_dir: PathBuf,
    pub decisions_dir: PathBuf,
    pub proofs_dir: PathBuf,
    pub autonomy_dir: PathBuf,
    pub project_dir: PathBuf,
    pub harness_spec_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusSnapshot {
    pub project_id: String,
    pub events_count: usize,
    pub work_id: Option<String>,
    pub lifecycle_state: Option<String>,
    pub autonomy_outcome: Option<String>,
    pub recovery_contract_ref: Option<String>,
    pub blocked_reason: Option<String>,
    pub next_action: Option<String>,
    pub next_action_ref: Option<String>,
    pub suggested_command: Option<String>,
    pub last_contract_id: Option<String>,
    pub last_run_id: Option<String>,
    pub last_decision_id: Option<String>,
    pub vcs_backend: Option<VcsKind>,
    pub vcs_ref: Option<String>,
    pub vcs_dirty: bool,
    pub workspace_root: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectCapabilitySummary {
    pub bootstrap_ready: bool,
    pub autonomous_ready: bool,
    pub staged_ready: bool,
    pub jj_ready: bool,
    pub proof_ready: bool,
    pub project_guidance_ready: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectHarnessSummary {
    pub inspect_ready: bool,
    pub bootable_per_workspace: bool,
    pub ui_legible: bool,
    pub logs_legible: bool,
    pub metrics_legible: bool,
    pub traces_legible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedHarnessCapabilities {
    pub ui_legible: bool,
    pub logs_legible: bool,
    pub metrics_legible: bool,
    pub traces_legible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedHarnessRecipe {
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedHarnessProfile {
    pub name: String,
    pub validation_surfaces: Vec<String>,
    #[serde(default)]
    pub validation_recipes: Vec<PersistedHarnessRecipe>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedHarnessSpec {
    pub project_id: String,
    pub inspect_ready: bool,
    pub bootable_per_workspace: bool,
    pub capabilities: PersistedHarnessCapabilities,
    pub profiles: Vec<PersistedHarnessProfile>,
    pub derivation_source: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectOverlay {
    pub project_id: String,
    pub repo_root: String,
    pub vcs_mode: String,
    pub bootstrap_ref: Option<String>,
    pub agent_guidance_ref: Vec<String>,
    pub capability_summary: ProjectCapabilitySummary,
    pub harness_summary: ProjectHarnessSummary,
    pub harness_spec_ref: String,
    pub harness_spec: PersistedHarnessSpec,
    pub project_skill_refs: Vec<String>,
    pub local_constraints: Vec<String>,
    pub safe_default_checks: Vec<String>,
    pub status_scope_mode: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkLedgerView {
    pub project_id: String,
    pub work_id: String,
    pub goal_ref: Option<String>,
    pub feature_ref: String,
    pub active_contract_ref: Option<String>,
    pub latest_run_ref: Option<String>,
    pub latest_receipt_ref: Option<String>,
    pub latest_decision_ref: Option<String>,
    pub latest_proof_ref: Option<String>,
    pub latest_autonomy_ref: Option<String>,
    pub autonomy_outcome: Option<String>,
    pub recovery_contract_ref: Option<String>,
    pub lifecycle_state: String,
    pub blocked_reason: Option<String>,
    pub latest_proof_command_evidence_summary: Vec<String>,
    pub next_action: Option<String>,
    pub next_action_ref: Option<String>,
    pub updated_at: String,
}

pub struct OrchService {
    paths: RepoPaths,
    events: EventStore,
}

fn phase_error<T>(phase: &str, result: Result<T>) -> Result<T> {
    result.map_err(|err| anyhow!("phase {phase}: {err}"))
}

fn repo_has_any(repo_root: &Path, rel_paths: &[&str]) -> bool {
    rel_paths
        .iter()
        .any(|rel_path| repo_root.join(rel_path).exists())
}

impl PersistedHarnessSpec {
    fn from_summary(
        project_id: &str,
        summary: &ProjectHarnessSummary,
        bootstrap_ref: Option<&str>,
        agent_guidance_ref: &[String],
        updated_at: &str,
    ) -> Self {
        let capabilities = PersistedHarnessCapabilities {
            ui_legible: summary.ui_legible,
            logs_legible: summary.logs_legible,
            metrics_legible: summary.metrics_legible,
            traces_legible: summary.traces_legible,
        };
        let validation_recipes = derived_validation_recipes(bootstrap_ref, agent_guidance_ref);
        let mut validation_surfaces = Vec::new();
        if summary.bootable_per_workspace {
            validation_surfaces.push("command".to_string());
        }
        if summary.ui_legible {
            validation_surfaces.push("ui_snapshot".to_string());
        }
        if summary.logs_legible {
            validation_surfaces.push("log_query".to_string());
        }
        if summary.metrics_legible {
            validation_surfaces.push("metric_assertion".to_string());
        }
        if summary.traces_legible {
            validation_surfaces.push("trace_assertion".to_string());
        }
        let profiles = if validation_surfaces.is_empty() {
            Vec::new()
        } else {
            vec![PersistedHarnessProfile {
                name: "default".to_string(),
                validation_surfaces,
                validation_recipes,
            }]
        };
        Self {
            project_id: project_id.to_string(),
            inspect_ready: summary.inspect_ready,
            bootable_per_workspace: summary.bootable_per_workspace,
            capabilities,
            profiles,
            derivation_source: "repo_markers_v1".to_string(),
            updated_at: updated_at.to_string(),
        }
    }
}

fn derived_validation_recipes(
    bootstrap_ref: Option<&str>,
    agent_guidance_ref: &[String],
) -> Vec<PersistedHarnessRecipe> {
    let mut refs = Vec::new();
    if let Some(path) = bootstrap_ref {
        refs.push(path.to_string());
    }
    refs.extend(agent_guidance_ref.iter().cloned());

    let mut recipes = Vec::new();
    for path in refs {
        if path.is_empty()
            || recipes
                .iter()
                .any(|recipe: &PersistedHarnessRecipe| recipe.path == path)
        {
            continue;
        }
        recipes.push(PersistedHarnessRecipe {
            kind: "artifact_assertion".to_string(),
            path,
        });
    }
    recipes
}

impl OrchService {
    pub fn new(repo_root: impl AsRef<Path>, global_root: impl AsRef<Path>) -> Result<Self> {
        let repo_root = repo_root.as_ref().to_path_buf();
        let dot_punk = repo_root.join(".punk");
        let project_dir = dot_punk.join("project");
        let paths = RepoPaths {
            repo_root: repo_root.clone(),
            global_root: global_root.as_ref().to_path_buf(),
            dot_punk: dot_punk.clone(),
            features_dir: dot_punk.join("features"),
            contracts_dir: dot_punk.join("contracts"),
            tasks_dir: dot_punk.join("tasks"),
            runs_dir: dot_punk.join("runs"),
            decisions_dir: dot_punk.join("decisions"),
            proofs_dir: dot_punk.join("proofs"),
            autonomy_dir: dot_punk.join("autonomy"),
            project_dir: project_dir.clone(),
            harness_spec_path: project_dir.join("harness.json"),
        };
        let events = EventStore::new(paths.global_root.clone());
        let service = Self { paths, events };
        service.bootstrap_project()?;
        service.quarantine_safe_stale_runs()?;
        Ok(service)
    }

    pub fn paths(&self) -> &RepoPaths {
        &self.paths
    }

    pub fn event_store(&self) -> &EventStore {
        &self.events
    }

    pub fn bootstrap_project(&self) -> Result<Project> {
        fs::create_dir_all(&self.paths.features_dir)?;
        fs::create_dir_all(&self.paths.contracts_dir)?;
        fs::create_dir_all(&self.paths.tasks_dir)?;
        fs::create_dir_all(&self.paths.runs_dir)?;
        fs::create_dir_all(&self.paths.decisions_dir)?;
        fs::create_dir_all(&self.paths.proofs_dir)?;
        fs::create_dir_all(&self.paths.autonomy_dir)?;
        fs::create_dir_all(&self.paths.project_dir)?;
        self.events.ensure_dirs()?;

        let project_id = project_id(&self.paths.repo_root)?;
        let project_path = self.paths.dot_punk.join("project.json");
        let current_path = self.paths.repo_root.display().to_string();
        let current_vcs_backend = detect_backend(&self.paths.repo_root)
            .ok()
            .map(|backend| backend.kind());
        if project_path.exists() {
            let mut project: Project = read_json(&project_path)?;
            let mut changed = false;
            if project.id != project_id {
                project.id = project_id.clone();
                changed = true;
            }
            if project.path != current_path {
                project.path = current_path.clone();
                changed = true;
            }
            if project.vcs_backend != current_vcs_backend {
                project.vcs_backend = current_vcs_backend.clone();
                changed = true;
            }
            if changed {
                project.updated_at = now_rfc3339();
                write_json(&project_path, &project)?;
            }
            return Ok(project);
        }
        let project = Project {
            id: project_id,
            path: current_path,
            vcs_backend: current_vcs_backend,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
        };
        write_json(&project_path, &project)?;
        Ok(project)
    }

    pub fn draft_contract(&self, drafter: &dyn ContractDrafter, prompt: &str) -> Result<Contract> {
        if prompt.trim().is_empty() {
            return Err(anyhow!("prompt must not be empty"));
        }
        let trimmed_prompt = prompt.trim();
        let project = phase_error("bootstrap", self.bootstrap_project())?;
        let scan = phase_error(
            "repo scan",
            scan_repo(&self.paths.repo_root, trimmed_prompt),
        )?;
        let mut scan = scan;
        enrich_scan_with_nested_integrity_fallback(&self.paths.repo_root, &mut scan)?;
        apply_prompt_targeting_bias(&self.paths.repo_root, trimmed_prompt, &mut scan);
        if scan.candidate_integrity_checks.is_empty() {
            return Err(anyhow!(
                "phase repo scan: {}",
                "unable to infer trustworthy integrity checks from repo scan"
            ));
        }
        let input = DraftInput {
            repo_root: self.paths.repo_root.display().to_string(),
            prompt: trimmed_prompt.to_string(),
            scan: scan.clone(),
        };
        let mut proposal = match drafter.draft(input) {
            Ok(proposal) => (proposal, false),
            Err(err) if is_drafter_timeout_error(&err) => (
                phase_error(
                    "drafter timeout fallback",
                    recover_timeout_draft_proposal(&self.paths.repo_root, trimmed_prompt, &scan),
                )?,
                true,
            ),
            Err(err) => return Err(anyhow!("phase drafter request: {err}")),
        }
        .0;
        canonicalize_draft_proposal(&self.paths.repo_root, trimmed_prompt, &mut proposal);
        normalize_proposal_scope(trimmed_prompt, &scan, &mut proposal);
        let mut errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
        if errors.is_empty() {
            if let Some(mut fallback) = build_bounded_fallback_proposal(
                &self.paths.repo_root,
                trimmed_prompt,
                &proposal,
                &scan,
                &errors,
            ) {
                normalize_proposal_scope(trimmed_prompt, &scan, &mut fallback);
                proposal = fallback;
                errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
            }
        }
        if !errors.is_empty() {
            let repair_guidance = format_validation_guidance(&errors);
            let repair_current = proposal.clone();
            proposal = match drafter.refine(RefineInput {
                repo_root: self.paths.repo_root.display().to_string(),
                prompt: trimmed_prompt.to_string(),
                guidance: repair_guidance.clone(),
                current: repair_current.clone(),
                scan: scan.clone(),
            }) {
                Ok(proposal) => (proposal, false),
                Err(err) if is_drafter_timeout_error(&err) => (
                    phase_error(
                        "drafter timeout fallback",
                        recover_timeout_refine_proposal(
                            &self.paths.repo_root,
                            trimmed_prompt,
                            &repair_guidance,
                            repair_current,
                            &scan,
                        ),
                    )?,
                    true,
                ),
                Err(err) => return Err(anyhow!("phase drafter repair: {err}")),
            }
            .0;
            canonicalize_draft_proposal(&self.paths.repo_root, trimmed_prompt, &mut proposal);
            normalize_proposal_scope(trimmed_prompt, &scan, &mut proposal);
            errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
            if errors.is_empty() {
                if let Some(mut fallback) = build_bounded_fallback_proposal(
                    &self.paths.repo_root,
                    trimmed_prompt,
                    &proposal,
                    &scan,
                    &errors,
                ) {
                    normalize_proposal_scope(trimmed_prompt, &scan, &mut fallback);
                    proposal = fallback;
                    errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
                }
            }
        }
        if !errors.is_empty() {
            if let Some(mut fallback) = build_bounded_fallback_proposal(
                &self.paths.repo_root,
                trimmed_prompt,
                &proposal,
                &scan,
                &errors,
            ) {
                normalize_proposal_scope(trimmed_prompt, &scan, &mut fallback);
                proposal = fallback;
                errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
            }
        }
        if !errors.is_empty() {
            return Err(anyhow!(
                "phase validate: draft proposal invalid after repair: {}",
                format_validation_guidance(&errors)
            ));
        }
        let (feature, contract) = phase_error(
            "persist",
            self.persist_draft_contract(&project, prompt, &proposal),
        )?;
        let contract_dir = self.paths.contracts_dir.join(&feature.id);
        phase_error(
            "persist",
            fs::create_dir_all(&contract_dir).map_err(Into::into),
        )?;
        let contract_path = contract_dir.join("v1.json");
        if !self
            .paths
            .features_dir
            .join(format!("{}.json", feature.id))
            .exists()
            || !contract_path.exists()
        {
            return Err(anyhow!(
                "phase persist: draft artifacts missing after write"
            ));
        }
        phase_error(
            "persist",
            self.append_event(
                &project.id,
                Some(&feature.id),
                None,
                None,
                ModeId::Plot,
                "contract.drafted",
                Some(&contract_path),
            ),
        )?;
        Ok(contract)
    }

    pub fn refine_contract(
        &self,
        drafter: &dyn ContractDrafter,
        contract_id: &str,
        guidance: &str,
    ) -> Result<Contract> {
        let guidance = guidance.trim();
        if guidance.is_empty() {
            return Err(anyhow!("guidance must not be empty"));
        }
        let project = self.bootstrap_project()?;
        let contract_path = self.find_object_path(&self.paths.contracts_dir, contract_id)?;
        let current_contract: Contract = read_json(&contract_path)?;
        if current_contract.status != ContractStatus::Draft {
            return Err(anyhow!("only draft contracts can be refined"));
        }
        let feature_path = self
            .paths
            .features_dir
            .join(format!("{}.json", current_contract.feature_id));
        let mut feature: Feature = read_json(&feature_path)?;
        let mut scan = scan_repo(&self.paths.repo_root, &current_contract.prompt_source)?;
        enrich_scan_with_nested_integrity_fallback(&self.paths.repo_root, &mut scan)?;
        apply_prompt_targeting_bias(
            &self.paths.repo_root,
            &current_contract.prompt_source,
            &mut scan,
        );
        if scan.candidate_integrity_checks.is_empty() {
            return Err(anyhow!(
                "unable to infer trustworthy integrity checks from repo scan"
            ));
        }
        let current = contract_to_proposal(&feature, &current_contract);
        let mut proposal = match drafter.refine(RefineInput {
            repo_root: self.paths.repo_root.display().to_string(),
            prompt: current_contract.prompt_source.clone(),
            guidance: guidance.to_string(),
            current,
            scan: scan.clone(),
        }) {
            Ok(proposal) => proposal,
            Err(err) if is_drafter_timeout_error(&err) => recover_timeout_refine_proposal(
                &self.paths.repo_root,
                &current_contract.prompt_source,
                guidance,
                contract_to_proposal(&feature, &current_contract),
                &scan,
            )?,
            Err(err) => return Err(err),
        };
        let combined_guidance = format!("{}\n{}", current_contract.prompt_source, guidance);
        canonicalize_draft_proposal(
            &self.paths.repo_root,
            &current_contract.prompt_source,
            &mut proposal,
        );
        apply_explicit_prompt_overrides(&self.paths.repo_root, guidance, &mut proposal);
        normalize_proposal_scope(&combined_guidance, &scan, &mut proposal);
        let mut errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
        if errors.is_empty() {
            if let Some(mut fallback) = build_bounded_fallback_proposal(
                &self.paths.repo_root,
                &current_contract.prompt_source,
                &proposal,
                &scan,
                &errors,
            ) {
                normalize_proposal_scope(&combined_guidance, &scan, &mut fallback);
                proposal = fallback;
                apply_explicit_prompt_overrides(&self.paths.repo_root, guidance, &mut proposal);
                normalize_proposal_scope(&combined_guidance, &scan, &mut proposal);
                errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
            }
        }
        if !errors.is_empty() {
            if let Some(mut fallback) = build_bounded_fallback_proposal(
                &self.paths.repo_root,
                &current_contract.prompt_source,
                &proposal,
                &scan,
                &errors,
            ) {
                normalize_proposal_scope(&combined_guidance, &scan, &mut fallback);
                proposal = fallback;
                apply_explicit_prompt_overrides(&self.paths.repo_root, guidance, &mut proposal);
                normalize_proposal_scope(&combined_guidance, &scan, &mut proposal);
                errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
            }
        }
        if !errors.is_empty() {
            return Err(anyhow!(
                "refined proposal invalid after repair: {}",
                format_validation_guidance(&errors)
            ));
        }

        feature.title = proposal.title.clone();
        feature.summary = proposal.summary.clone();
        feature.target_surface = if proposal.entry_points.is_empty() {
            vec![current_contract.prompt_source.clone()]
        } else {
            proposal.entry_points.clone()
        };
        feature.integrity_scope = proposal.allowed_scope.clone();
        feature.updated_at = now_rfc3339();
        write_json(&feature_path, &feature)?;

        let refined = Contract {
            id: current_contract.id.clone(),
            feature_id: current_contract.feature_id.clone(),
            version: current_contract.version,
            status: ContractStatus::Draft,
            prompt_source: current_contract.prompt_source.clone(),
            entry_points: proposal.entry_points,
            import_paths: proposal.import_paths,
            expected_interfaces: proposal.expected_interfaces,
            behavior_requirements: proposal.behavior_requirements,
            allowed_scope: proposal.allowed_scope,
            target_checks: proposal.target_checks,
            integrity_checks: proposal.integrity_checks,
            risk_level: proposal.risk_level,
            created_at: current_contract.created_at.clone(),
            approved_at: None,
        };
        write_json(&contract_path, &refined)?;
        self.append_event(
            &project.id,
            Some(&feature.id),
            None,
            None,
            ModeId::Plot,
            "contract.refined",
            Some(&contract_path),
        )?;
        Ok(refined)
    }

    pub fn approve_contract(&self, contract_id: &str) -> Result<Contract> {
        let project = self.bootstrap_project()?;
        let contract_path = self.find_object_path(&self.paths.contracts_dir, contract_id)?;
        let mut contract: Contract = read_json(&contract_path)?;
        if contract.status != ContractStatus::Draft {
            return Err(anyhow!("only draft contracts can be approved"));
        }
        let feature_path = self
            .paths
            .features_dir
            .join(format!("{}.json", contract.feature_id));
        let feature: Feature = read_json(&feature_path)?;
        let proposal = contract_to_proposal(&feature, &contract);
        let errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
        if !errors.is_empty() {
            return Err(anyhow!(
                "contract must be approve-ready before approve: {}",
                format_validation_guidance(&errors)
            ));
        }
        contract.status = ContractStatus::Approved;
        contract.approved_at = Some(now_rfc3339());
        write_json(&contract_path, &contract)?;
        self.append_event(
            &project.id,
            Some(&contract.feature_id),
            None,
            None,
            ModeId::Plot,
            "contract.approved",
            Some(&contract_path),
        )?;
        Ok(contract)
    }

    fn persist_draft_contract(
        &self,
        project: &Project,
        prompt: &str,
        proposal: &DraftProposal,
    ) -> Result<(Feature, Contract)> {
        let feature_id = new_id("feat");
        let feature = Feature {
            id: feature_id.clone(),
            project_id: project.id.clone(),
            title: proposal.title.clone(),
            summary: proposal.summary.clone(),
            status: FeatureStatus::Draft,
            target_surface: if proposal.entry_points.is_empty() {
                vec![summarize_prompt(prompt)]
            } else {
                proposal.entry_points.clone()
            },
            integrity_scope: proposal.allowed_scope.clone(),
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
        };
        let feature_path = self.paths.features_dir.join(format!("{}.json", feature.id));
        write_json(&feature_path, &feature)?;
        self.append_event(
            &project.id,
            Some(&feature.id),
            None,
            None,
            ModeId::Plot,
            "feature.created",
            Some(&feature_path),
        )?;

        let contract = Contract {
            id: format!("ct_{}_v1", &feature_id.trim_start_matches("feat_")),
            feature_id: feature.id.clone(),
            version: 1,
            status: ContractStatus::Draft,
            prompt_source: prompt.trim().to_string(),
            entry_points: proposal.entry_points.clone(),
            import_paths: proposal.import_paths.clone(),
            expected_interfaces: proposal.expected_interfaces.clone(),
            behavior_requirements: proposal.behavior_requirements.clone(),
            allowed_scope: proposal.allowed_scope.clone(),
            target_checks: proposal.target_checks.clone(),
            integrity_checks: proposal.integrity_checks.clone(),
            risk_level: proposal.risk_level.clone(),
            created_at: now_rfc3339(),
            approved_at: None,
        };
        let contract_dir = self.paths.contracts_dir.join(&feature.id);
        fs::create_dir_all(&contract_dir)?;
        let contract_path = contract_dir.join("v1.json");
        write_json(&contract_path, &contract)?;
        Ok((feature, contract))
    }

    pub fn cut_run(&self, executor: &dyn Executor, contract_id: &str) -> Result<(Run, Receipt)> {
        let project = self.bootstrap_project()?;
        let contract_path = self.find_object_path(&self.paths.contracts_dir, contract_id)?;
        let contract: Contract = read_json(&contract_path)?;
        if contract.status != ContractStatus::Approved {
            return Err(anyhow!("cut run requires an approved contract"));
        }

        let task = Task {
            id: new_id("task"),
            feature_id: contract.feature_id.clone(),
            contract_id: contract.id.clone(),
            kind: TaskKind::Implement,
            status: TaskStatus::Claimed,
            requested_by: "operator".to_string(),
            created_at: now_rfc3339(),
            claimed_at: Some(now_rfc3339()),
        };
        let task_path = self.paths.tasks_dir.join(format!("{}.json", task.id));
        write_json(&task_path, &task)?;
        self.append_event(
            &project.id,
            Some(&task.feature_id),
            Some(&task.id),
            None,
            ModeId::Cut,
            "task.queued",
            Some(&task_path),
        )?;
        self.append_event(
            &project.id,
            Some(&task.feature_id),
            Some(&task.id),
            None,
            ModeId::Cut,
            "task.claimed",
            Some(&task_path),
        )?;

        let backend = detect_backend(&self.paths.repo_root)?;
        let preexisting_changed_files = backend.changed_files().unwrap_or_default();
        let isolated = backend.create_isolated_change(&task.id)?;
        let workspace_root = PathBuf::from(&isolated.workspace_ref);
        sync_present_repo_root_changes_to_isolated_workspace(
            &self.paths.repo_root,
            &workspace_root,
            &preexisting_changed_files,
        )?;
        let isolated_backend = detect_backend(&workspace_root)?;
        let run_id = new_id("run");
        let run_dir = self.paths.runs_dir.join(&run_id);
        fs::create_dir_all(&run_dir)?;
        let mut run = Run {
            id: run_id.clone(),
            task_id: task.id.clone(),
            feature_id: task.feature_id.clone(),
            contract_id: contract.id.clone(),
            attempt: 1,
            status: RunStatus::Running,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: backend.kind(),
                workspace_ref: isolated.workspace_ref,
                change_ref: isolated.change_ref,
                base_ref: isolated.base_ref,
            },
            started_at: now_rfc3339(),
            ended_at: None,
        };
        let run_path = run_dir.join("run.json");
        write_json(&run_path, &run)?;
        let mut run_finalizer = RunFinalizer::new(run_path.clone(), &run);
        let stdout_path = run_dir.join("stdout.log");
        let stderr_path = run_dir.join("stderr.log");
        let executor_pid_path = run_dir.join("executor.json");
        let heartbeat_path = run_dir.join("heartbeat.json");
        let cargo_lock_existed_before_run = workspace_root.join("Cargo.lock").exists();
        fs::write(&stdout_path, b"")?;
        fs::write(&stderr_path, b"")?;
        write_run_heartbeat(
            &heartbeat_path,
            &run.id,
            "running",
            &stdout_path,
            &stderr_path,
        )?;
        let mut heartbeat = RunHeartbeatGuard::new(
            heartbeat_path,
            run.id.clone(),
            stdout_path.clone(),
            stderr_path.clone(),
        );
        if let Err(error) = self.append_event(
            &project.id,
            Some(&run.feature_id),
            Some(&task.id),
            Some(&run.id),
            ModeId::Cut,
            "run.started",
            Some(&run_path),
        ) {
            let summary = format!("run startup failed before executor handoff: {error}");
            fs::write(&stderr_path, &summary)?;
            run.status = RunStatus::Failed;
            run.ended_at = Some(now_rfc3339());
            run_finalizer.sync(&run);
            write_json(&run_path, &run)?;
            run_finalizer.disarm();
            heartbeat.finish("failed");
            let receipt = Receipt {
                id: new_id("rcpt"),
                run_id: run.id.clone(),
                task_id: task.id.clone(),
                status: "failure".to_string(),
                executor_name: executor.name().to_string(),
                changed_files: Vec::new(),
                artifacts: ReceiptArtifacts {
                    stdout_ref: relative_ref(&self.paths.repo_root, &stdout_path)?,
                    stderr_ref: relative_ref(&self.paths.repo_root, &stderr_path)?,
                },
                checks_run: Vec::new(),
                duration_ms: 0,
                cost_usd: None,
                summary,
                created_at: now_rfc3339(),
            };
            let receipt_path = run_dir.join("receipt.json");
            write_json(&receipt_path, &receipt)?;
            return Ok((run, receipt));
        }
        let provenance_baseline = isolated_backend.capture_provenance_baseline().ok();
        let execution = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            executor.execute_contract(ExecuteInput {
                repo_root: workspace_root.clone(),
                contract: contract.clone(),
                stdout_path: stdout_path.clone(),
                stderr_path: stderr_path.clone(),
                executor_pid_path: executor_pid_path.clone(),
            })
        }));
        let execution = match execution {
            Ok(result) => result,
            Err(panic) => {
                let _ = fs::write(&stderr_path, b"executor panicked");
                run.status = RunStatus::Failed;
                run.ended_at = Some(now_rfc3339());
                run_finalizer.sync(&run);
                heartbeat.finish("failed");
                std::panic::resume_unwind(panic);
            }
        };

        let (mut status, mut summary, checks_run, cost_usd, duration_ms) = match execution {
            Ok(output) => {
                let already_satisfied_before_dispatch = !output.success
                    && should_treat_cut_run_as_already_satisfied(
                        &contract,
                        &output.summary,
                        &preexisting_changed_files,
                    );
                run.status = if output.success || already_satisfied_before_dispatch {
                    RunStatus::Finished
                } else {
                    RunStatus::Failed
                };
                (
                    if output.success || already_satisfied_before_dispatch {
                        "success"
                    } else {
                        "failure"
                    }
                    .to_string(),
                    if already_satisfied_before_dispatch {
                        already_satisfied_before_dispatch_summary(
                            &contract.entry_points,
                            &output.summary,
                        )
                    } else {
                        output.summary
                    },
                    output.checks_run,
                    output.cost_usd,
                    output.duration_ms,
                )
            }
            Err(error) => {
                fs::write(&stderr_path, error.to_string())?;
                run.status = RunStatus::Failed;
                (
                    "failure".to_string(),
                    error.to_string(),
                    Vec::new(),
                    None,
                    0,
                )
            }
        };
        prune_generated_cargo_lock_if_out_of_scope(
            &workspace_root,
            &contract,
            cargo_lock_existed_before_run,
        )?;
        let changed_files = provenance_baseline
            .as_ref()
            .and_then(|baseline| isolated_backend.changed_files_since(baseline).ok())
            .unwrap_or_default();
        if backend.kind() == VcsKind::Git && workspace_root != self.paths.repo_root {
            sync_present_isolated_changes_to_repo_root(
                &self.paths.repo_root,
                &workspace_root,
                &changed_files,
            )?;
        }
        if should_reject_empty_successful_bounded_run(&contract, &status, &summary, &changed_files)
        {
            status = "failure".to_string();
            summary = empty_successful_bounded_run_summary(&contract.entry_points, &summary);
            run.status = RunStatus::Failed;
        }
        if status == "success" {
            ensure_default_gitignore_coverage(&workspace_root)?;
            if workspace_root != self.paths.repo_root {
                ensure_default_gitignore_coverage(&self.paths.repo_root)?;
            }
        }
        run.ended_at = Some(now_rfc3339());
        run_finalizer.sync(&run);
        write_json(&run_path, &run)?;
        run_finalizer.disarm();
        heartbeat.finish(match run.status {
            RunStatus::Finished => "finished",
            RunStatus::Failed => "failed",
            RunStatus::Cancelled => "cancelled",
            RunStatus::Running => "running",
        });

        let receipt = Receipt {
            id: new_id("rcpt"),
            run_id: run.id.clone(),
            task_id: task.id.clone(),
            status,
            executor_name: executor.name().to_string(),
            changed_files,
            artifacts: ReceiptArtifacts {
                stdout_ref: relative_ref(&self.paths.repo_root, &stdout_path)?,
                stderr_ref: relative_ref(&self.paths.repo_root, &stderr_path)?,
            },
            checks_run,
            duration_ms,
            cost_usd,
            summary,
            created_at: now_rfc3339(),
        };
        let receipt_path = run_dir.join("receipt.json");
        write_json(&receipt_path, &receipt)?;
        self.append_event(
            &project.id,
            Some(&run.feature_id),
            Some(&task.id),
            Some(&run.id),
            ModeId::Cut,
            "receipt.written",
            Some(&receipt_path),
        )?;
        self.append_event(
            &project.id,
            Some(&run.feature_id),
            Some(&task.id),
            Some(&run.id),
            ModeId::Cut,
            "run.finished",
            Some(&run_path),
        )?;
        Ok((run, receipt))
    }

    pub fn status(&self, id: Option<&str>) -> Result<StatusSnapshot> {
        let project = self.bootstrap_project()?;
        let events = self.events.load_all()?;
        let vcs_snapshot = current_snapshot_ref(&self.paths.repo_root).ok();
        let workspace_root = detect_backend(&self.paths.repo_root)
            .ok()
            .and_then(|backend| backend.workspace_root().ok())
            .map(|path| path.display().to_string());
        let ledger = match id {
            Some(id) if id == project.id => self.inspect_work_ledger(None).ok(),
            Some(id) => self.inspect_work_ledger(Some(id)).ok(),
            None => self.inspect_work_ledger(None).ok(),
        };

        Ok(StatusSnapshot {
            project_id: project.id,
            events_count: events.len(),
            work_id: ledger.as_ref().map(|ledger| ledger.work_id.clone()),
            lifecycle_state: ledger.as_ref().map(|ledger| ledger.lifecycle_state.clone()),
            autonomy_outcome: ledger
                .as_ref()
                .and_then(|ledger| ledger.autonomy_outcome.clone()),
            recovery_contract_ref: ledger
                .as_ref()
                .and_then(|ledger| ledger.recovery_contract_ref.clone()),
            blocked_reason: ledger
                .as_ref()
                .and_then(|ledger| ledger.blocked_reason.clone()),
            next_action: ledger
                .as_ref()
                .and_then(|ledger| ledger.next_action.clone()),
            next_action_ref: ledger
                .as_ref()
                .and_then(|ledger| ledger.next_action_ref.clone()),
            suggested_command: ledger.as_ref().and_then(|ledger| {
                work_suggested_command(
                    ledger.next_action.as_deref(),
                    ledger.next_action_ref.as_deref(),
                )
            }),
            last_contract_id: ledger.as_ref().and_then(|ledger| {
                work_object_id_from_ref(
                    &self.paths.repo_root,
                    ledger.active_contract_ref.as_deref(),
                    "ct_",
                )
            }),
            last_run_id: ledger.as_ref().and_then(|ledger| {
                work_object_id_from_ref(
                    &self.paths.repo_root,
                    ledger.latest_run_ref.as_deref(),
                    "run_",
                )
            }),
            last_decision_id: ledger.as_ref().and_then(|ledger| {
                work_object_id_from_ref(
                    &self.paths.repo_root,
                    ledger.latest_decision_ref.as_deref(),
                    "dec_",
                )
            }),
            vcs_backend: vcs_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.vcs.clone()),
            vcs_ref: vcs_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.head_ref.clone()),
            vcs_dirty: vcs_snapshot
                .as_ref()
                .map(|snapshot| snapshot.dirty)
                .unwrap_or(false),
            workspace_root,
        })
    }

    pub fn inspect(&self, id: &str) -> Result<serde_json::Value> {
        let project = self.bootstrap_project()?;
        if id == "project" {
            return Ok(serde_json::to_value(self.inspect_project_overlay()?)?);
        }
        if id == project.id {
            let mut value = serde_json::to_value(project)?;
            attach_live_vcs(&mut value, self.live_vcs_value());
            return Ok(value);
        }

        for dir in [
            &self.paths.features_dir,
            &self.paths.contracts_dir,
            &self.paths.tasks_dir,
            &self.paths.runs_dir,
            &self.paths.decisions_dir,
            &self.paths.proofs_dir,
            &self.paths.autonomy_dir,
        ] {
            if let Ok(path) = self.find_object_path(dir, id) {
                let mut value = read_json(&path)?;
                if dir == &self.paths.runs_dir {
                    attach_live_vcs(&mut value, self.live_vcs_value());
                }
                return Ok(value);
            }
        }
        Err(anyhow!("unknown id: {id}"))
    }

    pub fn inspect_proofpack(&self, proof_id: &str) -> Result<punk_domain::Proofpack> {
        let proof_path = self.find_object_path(&self.paths.proofs_dir, proof_id)?;
        read_json(&proof_path)
    }

    pub fn inspect_work_ledger(&self, id: Option<&str>) -> Result<WorkLedgerView> {
        let project = self.bootstrap_project()?;
        let feature_id = match id {
            Some(id) => self.resolve_feature_id_for_work(id)?,
            None => self.latest_feature_id_for_project(&project)?,
        };
        self.build_work_ledger_view(&project, &feature_id)
    }

    pub fn inspect_project_overlay(&self) -> Result<ProjectOverlay> {
        let project = self.bootstrap_project()?;
        let project_label = project_label(&self.paths.repo_root);
        let bootstrap_path = self
            .paths
            .dot_punk
            .join("bootstrap")
            .join(format!("{project_label}-core.md"));
        let repo_agents_path = self.paths.repo_root.join("AGENTS.md");
        let agent_start_path = self.paths.dot_punk.join("AGENT_START.md");

        let bootstrap_ref = bootstrap_path
            .exists()
            .then(|| relative_ref(&self.paths.repo_root, &bootstrap_path))
            .transpose()?;

        let mut agent_guidance_ref = Vec::new();
        if repo_agents_path.exists() {
            agent_guidance_ref.push(relative_ref(&self.paths.repo_root, &repo_agents_path)?);
        }
        if agent_start_path.exists() {
            agent_guidance_ref.push(relative_ref(&self.paths.repo_root, &agent_start_path)?);
        }

        let mut local_constraints = Vec::new();
        let safe_default_checks =
            match scan_repo(&self.paths.repo_root, "project overlay safe default checks") {
                Ok(scan) => scan.candidate_integrity_checks,
                Err(error) => {
                    local_constraints.push(format!("unable to infer safe default checks: {error}"));
                    Vec::new()
                }
            };

        let project_skill_refs = active_project_skill_refs(&project_label);
        if bootstrap_ref.is_none() {
            local_constraints.push("repo-local bootstrap guidance missing".to_string());
        }
        if !repo_agents_path.exists() {
            local_constraints.push("repo-root AGENTS.md missing".to_string());
        }
        if !agent_start_path.exists() {
            local_constraints.push(".punk/AGENT_START.md missing".to_string());
        }
        if project_skill_refs.is_empty() {
            local_constraints.push("no active project-scoped skill detected".to_string());
        }

        let vcs_mode = project_overlay_vcs_mode(&self.paths.repo_root);
        if vcs_mode != "jj" {
            local_constraints.push("jj is not enabled for this repo".to_string());
        }

        let project_guidance_ready = repo_agents_path.exists() && agent_start_path.exists();
        let staged_ready = !safe_default_checks.is_empty();
        let capability_summary = ProjectCapabilitySummary {
            bootstrap_ready: bootstrap_ref.is_some(),
            autonomous_ready: project_guidance_ready && staged_ready,
            staged_ready,
            jj_ready: vcs_mode == "jj",
            proof_ready: staged_ready,
            project_guidance_ready,
        };
        let ui_legible = repo_has_any(
            &self.paths.repo_root,
            &[
                "playwright.config.ts",
                "playwright.config.js",
                "playwright.config.mjs",
                "playwright.config.cjs",
                "tests/e2e",
                "e2e",
            ],
        );
        let logs_legible = repo_has_any(
            &self.paths.repo_root,
            &[
                "logs",
                "observability/logs",
                "config/logging.yaml",
                "config/logging.yml",
                "vector.toml",
            ],
        );
        let metrics_legible = repo_has_any(
            &self.paths.repo_root,
            &[
                "metrics",
                "observability/metrics",
                "prometheus.yml",
                "prometheus.yaml",
            ],
        );
        let traces_legible = repo_has_any(
            &self.paths.repo_root,
            &[
                "traces",
                "observability/traces",
                "otel-collector.yaml",
                "otel-collector.yml",
            ],
        );
        let bootable_per_workspace =
            bootstrap_ref.is_some() && staged_ready && vcs_mode != "no_vcs";
        let harness_summary = ProjectHarnessSummary {
            inspect_ready: bootable_per_workspace
                || ui_legible
                || logs_legible
                || metrics_legible
                || traces_legible,
            bootable_per_workspace,
            ui_legible,
            logs_legible,
            metrics_legible,
            traces_legible,
        };
        let persisted_harness_spec = PersistedHarnessSpec::from_summary(
            &project.id,
            &harness_summary,
            bootstrap_ref.as_deref(),
            &agent_guidance_ref,
            &project.updated_at,
        );
        write_json(&self.paths.harness_spec_path, &persisted_harness_spec)?;
        let harness_spec: PersistedHarnessSpec = read_json(&self.paths.harness_spec_path)?;
        let harness_spec_ref = relative_ref(&self.paths.repo_root, &self.paths.harness_spec_path)?;

        Ok(ProjectOverlay {
            project_id: project.id.clone(),
            repo_root: project.path,
            vcs_mode,
            bootstrap_ref,
            agent_guidance_ref,
            capability_summary,
            harness_summary,
            harness_spec_ref,
            harness_spec,
            project_skill_refs,
            local_constraints,
            safe_default_checks,
            status_scope_mode: format!("project:{}", project.id),
            updated_at: project.updated_at,
        })
    }

    pub fn gc_stale_dry_run(&self) -> Result<StaleGcReport> {
        let project = self.bootstrap_project()?;
        let mut safe_to_archive = Vec::new();
        let mut manual_review = Vec::new();

        for entry in fs::read_dir(&self.paths.features_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let feature: Feature = read_json(&path)?;
            if feature.project_id != project.id {
                continue;
            }
            for run_record in work_run_records(&self.paths.runs_dir, &feature.id)? {
                if let Some(candidate) =
                    classify_stale_run_candidate(&self.paths, &feature.id, &run_record)?
                {
                    match candidate {
                        StaleRunDisposition::SafeToArchive(candidate) => {
                            safe_to_archive.push(candidate)
                        }
                        StaleRunDisposition::ManualReview(candidate) => {
                            manual_review.push(candidate)
                        }
                    }
                }
            }
        }

        safe_to_archive.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
        manual_review.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));

        Ok(StaleGcReport {
            project_id: project.id,
            generated_at: now_rfc3339(),
            safe_to_archive,
            manual_review,
        })
    }

    fn quarantine_safe_stale_runs(&self) -> Result<Vec<StaleArtifactCandidate>> {
        let report = self.gc_stale_dry_run()?;
        let mut archived = Vec::new();
        let archive_root = self.paths.dot_punk.join("archive").join("runs");
        fs::create_dir_all(&archive_root)?;

        for candidate in report.safe_to_archive {
            let source_dir = self
                .paths
                .repo_root
                .join(candidate.artifact_ref.trim_end_matches("/run.json"))
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.paths.runs_dir.join(&candidate.artifact_id));
            let source_dir = if source_dir.ends_with(&candidate.artifact_id) {
                source_dir
            } else {
                self.paths.runs_dir.join(&candidate.artifact_id)
            };
            if !source_dir.exists() {
                continue;
            }

            let target_dir = archive_root.join(&candidate.artifact_id);
            if target_dir.exists() {
                fs::remove_dir_all(&target_dir)?;
            }
            fs::rename(&source_dir, &target_dir)?;

            let archive_record = StaleArchiveRecord {
                archived_at: now_rfc3339(),
                run_id: candidate.artifact_id.clone(),
                work_id: candidate.work_id.clone(),
                original_ref: candidate.artifact_ref.clone(),
                reason: candidate.reason.clone(),
                last_progress_at: candidate.last_progress_at.clone(),
                executor_pid: candidate.executor_pid,
            };
            write_json(&target_dir.join("quarantine.json"), &archive_record)?;
            archived.push(candidate);
        }

        Ok(archived)
    }

    pub fn record_autonomy_outcome(
        &self,
        proof_id: &str,
        recovery_contract_id: Option<&str>,
    ) -> Result<AutonomyRecord> {
        let project = self.bootstrap_project()?;
        let proof_path = self.find_object_path(&self.paths.proofs_dir, proof_id)?;
        let proof: punk_domain::Proofpack = read_json(&proof_path)?;
        let decision_path = self.find_object_path(&self.paths.decisions_dir, &proof.decision_id)?;
        let decision: punk_domain::DecisionObject = read_json(&decision_path)?;
        let run_path = self.find_object_path(&self.paths.runs_dir, &proof.run_id)?;
        let run: Run = read_json(&run_path)?;
        let contract_path =
            self.find_object_path(&self.paths.contracts_dir, &decision.contract_id)?;
        let contract: Contract = read_json(&contract_path)?;
        let contract_ref = relative_ref(&self.paths.repo_root, &contract_path)?;
        let run_ref = relative_ref(&self.paths.repo_root, &run_path)?;
        let decision_ref = relative_ref(&self.paths.repo_root, &decision_path)?;
        let proof_ref = relative_ref(&self.paths.repo_root, &proof_path)?;
        let recovery_contract_ref = recovery_contract_id
            .map(|id| self.find_object_path(&self.paths.contracts_dir, id))
            .transpose()?
            .map(|path| relative_ref(&self.paths.repo_root, &path))
            .transpose()?;
        let autonomy_outcome = match decision.decision {
            punk_domain::Decision::Accept => AutonomyOutcome::Succeeded,
            punk_domain::Decision::Block => AutonomyOutcome::Blocked,
            punk_domain::Decision::Escalate => AutonomyOutcome::Escalated,
        };
        let basis_summary = summarize_decision_basis(&decision.decision_basis);
        let (next_action, next_action_ref) =
            autonomy_next_action(&decision, recovery_contract_id, &proof.id);
        let record = AutonomyRecord {
            id: format!("auto_{}", run.id.trim_start_matches("run_")),
            work_id: run.feature_id.clone(),
            goal_ref: Some(contract.prompt_source.clone()),
            contract_ref,
            run_ref,
            decision_ref,
            proof_ref,
            autonomy_outcome,
            basis_summary,
            recovery_contract_ref,
            next_action: next_action.to_string(),
            next_action_ref,
            recorded_at: now_rfc3339(),
        };
        let record_dir = self.paths.autonomy_dir.join(&run.feature_id);
        fs::create_dir_all(&record_dir)?;
        let record_path = record_dir.join(format!("{}.json", record.id));
        write_json(&record_path, &record)?;
        self.append_event(
            &project.id,
            Some(&run.feature_id),
            None,
            Some(&run.id),
            ModeId::Gate,
            "autonomy.recorded",
            Some(&record_path),
        )?;
        Ok(record)
    }

    fn build_work_ledger_view(
        &self,
        project: &Project,
        feature_id: &str,
    ) -> Result<WorkLedgerView> {
        let feature_path = self.paths.features_dir.join(format!("{feature_id}.json"));
        let feature: Feature = read_json(&feature_path)?;
        let feature_ref = relative_ref(&self.paths.repo_root, &feature_path)?;

        let contracts = work_contract_records(&self.paths.contracts_dir, feature_id)?;
        let active_contract = contracts.into_iter().max_by(|left, right| {
            left.contract
                .version
                .cmp(&right.contract.version)
                .then_with(|| left.contract.created_at.cmp(&right.contract.created_at))
        });

        let runs = work_run_records(&self.paths.runs_dir, feature_id)?;
        let latest_run = runs
            .into_iter()
            .filter(|record| {
                !matches!(
                    classify_stale_run_candidate(&self.paths, feature_id, record),
                    Ok(Some(StaleRunDisposition::SafeToArchive(_)))
                )
            })
            .max_by(|left, right| {
                left.run
                    .started_at
                    .cmp(&right.run.started_at)
                    .then_with(|| left.run.id.cmp(&right.run.id))
            });

        let latest_decision = match latest_run.as_ref() {
            Some(run_record) => {
                latest_decision_record(&self.paths.decisions_dir, &run_record.run.id)?
            }
            None => None,
        };
        let latest_proof = match latest_decision.as_ref() {
            Some(decision_record) => {
                latest_proof_record(&self.paths.proofs_dir, &decision_record.decision.id)?
            }
            None => None,
        };
        let latest_autonomy = latest_autonomy_record(
            &self.paths.repo_root,
            &self.paths.autonomy_dir,
            feature_id,
            latest_run.as_ref().map(|record| record.run.id.as_str()),
        )?;

        let active_contract_ref = active_contract
            .as_ref()
            .map(|record| relative_ref(&self.paths.repo_root, &record.path))
            .transpose()?;
        let latest_run_ref = latest_run
            .as_ref()
            .map(|record| relative_ref(&self.paths.repo_root, &record.run_path))
            .transpose()?;
        let latest_receipt_ref = latest_run
            .as_ref()
            .and_then(|record| {
                record
                    .receipt
                    .as_ref()
                    .map(|_| relative_ref(&self.paths.repo_root, &record.receipt_path))
            })
            .transpose()?;
        let latest_decision_ref = latest_decision
            .as_ref()
            .map(|record| relative_ref(&self.paths.repo_root, &record.path))
            .transpose()?;
        let latest_proof_ref = latest_proof
            .as_ref()
            .map(|record| relative_ref(&self.paths.repo_root, &record.path))
            .transpose()?;
        let latest_autonomy_ref = latest_autonomy
            .as_ref()
            .map(|record| relative_ref(&self.paths.repo_root, &record.path))
            .transpose()?;

        let lifecycle_state = work_lifecycle_state(
            active_contract.as_ref().map(|record| &record.contract),
            latest_run.as_ref().map(|record| &record.run),
            latest_decision.as_ref().map(|record| &record.decision),
            latest_autonomy.as_ref().map(|record| &record.record),
        );
        let blocked_reason = work_blocked_reason(
            latest_run
                .as_ref()
                .and_then(|record| record.receipt.as_ref()),
            latest_decision.as_ref().map(|record| &record.decision),
            latest_autonomy.as_ref().map(|record| &record.record),
        );
        let (next_action, next_action_ref) = work_next_action(
            active_contract.as_ref().map(|record| &record.contract),
            latest_run.as_ref().map(|record| &record.run),
            latest_decision.as_ref().map(|record| &record.decision),
            latest_proof.as_ref().map(|record| &record.proof),
            latest_autonomy.as_ref().map(|record| &record.record),
        );
        let autonomy_outcome = latest_autonomy
            .as_ref()
            .map(|record| autonomy_outcome_label(&record.record.autonomy_outcome).to_string());
        let recovery_contract_ref = latest_autonomy
            .as_ref()
            .and_then(|record| record.record.recovery_contract_ref.clone());
        let latest_proof_command_evidence_summary = latest_proof
            .as_ref()
            .map(|record| summarize_command_evidence(&record.proof.command_evidence))
            .unwrap_or_default();

        Ok(WorkLedgerView {
            project_id: project.id.clone(),
            work_id: feature.id.clone(),
            goal_ref: active_contract
                .as_ref()
                .map(|record| record.contract.prompt_source.clone()),
            feature_ref,
            active_contract_ref,
            latest_run_ref,
            latest_receipt_ref,
            latest_decision_ref,
            latest_proof_ref,
            latest_autonomy_ref,
            autonomy_outcome,
            recovery_contract_ref,
            lifecycle_state: lifecycle_state.to_string(),
            blocked_reason,
            latest_proof_command_evidence_summary,
            next_action,
            next_action_ref,
            updated_at: work_updated_at(
                &feature,
                active_contract.as_ref().map(|record| &record.contract),
                latest_run.as_ref().map(|record| &record.run),
                latest_run
                    .as_ref()
                    .and_then(|record| record.receipt.as_ref()),
                latest_decision.as_ref().map(|record| &record.decision),
                latest_proof.as_ref().map(|record| &record.proof),
                latest_autonomy.as_ref().map(|record| &record.record),
            ),
        })
    }

    fn resolve_feature_id_for_work(&self, id: &str) -> Result<String> {
        let feature_path = self.paths.features_dir.join(format!("{id}.json"));
        if feature_path.exists() {
            let feature: Feature = read_json(&feature_path)?;
            return Ok(feature.id);
        }

        if let Ok(path) = self.find_object_path(&self.paths.contracts_dir, id) {
            let contract: Contract = read_json(&path)?;
            return Ok(contract.feature_id);
        }
        if let Ok(path) = self.find_object_path(&self.paths.runs_dir, id) {
            let run: Run = read_json(&path)?;
            return Ok(run.feature_id);
        }
        if let Ok(path) = self.find_object_path(&self.paths.tasks_dir, id) {
            let task: Task = read_json(&path)?;
            return Ok(task.feature_id);
        }
        if let Ok(path) = self.find_object_path(&self.paths.decisions_dir, id) {
            let decision: punk_domain::DecisionObject = read_json(&path)?;
            let run_path = self.find_object_path(&self.paths.runs_dir, &decision.run_id)?;
            let run: Run = read_json(&run_path)?;
            return Ok(run.feature_id);
        }
        if let Ok(path) = self.find_object_path(&self.paths.proofs_dir, id) {
            let proof: punk_domain::Proofpack = read_json(&path)?;
            let run_path = self.find_object_path(&self.paths.runs_dir, &proof.run_id)?;
            let run: Run = read_json(&run_path)?;
            return Ok(run.feature_id);
        }
        if let Ok(path) = self.find_object_path(&self.paths.autonomy_dir, id) {
            let record: AutonomyRecord = read_json(&path)?;
            return Ok(record.work_id);
        }

        Err(anyhow!("unknown work id: {id}"))
    }

    fn latest_feature_id_for_project(&self, project: &Project) -> Result<String> {
        let mut latest: Option<(String, String, String)> = None;
        for entry in fs::read_dir(&self.paths.features_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let feature: Feature = read_json(&path)?;
            if feature.project_id != project.id {
                continue;
            }
            let activity_updated_at = self
                .build_work_ledger_view(project, &feature.id)
                .map(|ledger| ledger.updated_at)
                .unwrap_or_else(|_| feature.updated_at.clone());
            let replace = latest
                .as_ref()
                .map(|current| {
                    activity_updated_at
                        .cmp(&current.1)
                        .then_with(|| feature.created_at.cmp(&current.2))
                        .is_gt()
                })
                .unwrap_or(true);
            if replace {
                latest = Some((feature.id, activity_updated_at, feature.created_at));
            }
        }

        latest
            .map(|feature| feature.0)
            .ok_or_else(|| anyhow!("no work items found for project"))
    }

    fn live_vcs_value(&self) -> serde_json::Value {
        let snapshot = current_snapshot_ref(&self.paths.repo_root).ok();
        let workspace_root = detect_backend(&self.paths.repo_root)
            .ok()
            .and_then(|backend| backend.workspace_root().ok())
            .map(|path| path.display().to_string());
        serde_json::json!({
            "backend": snapshot.as_ref().and_then(|s| s.vcs.clone()),
            "ref": snapshot.as_ref().and_then(|s| s.head_ref.clone()),
            "dirty": snapshot.as_ref().map(|s| s.dirty).unwrap_or(false),
            "workspace_root": workspace_root,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn append_event(
        &self,
        project_id: &str,
        feature_id: Option<&str>,
        task_id: Option<&str>,
        run_id: Option<&str>,
        mode: ModeId,
        kind: &str,
        payload_path: Option<&Path>,
    ) -> Result<()> {
        let payload_ref = payload_path
            .map(|path| relative_ref(&self.paths.repo_root, path))
            .transpose()?;
        let payload_sha256 = payload_path
            .map(|path| self.events.file_sha256(path))
            .transpose()?;
        let event = EventEnvelope {
            event_id: new_id("evt"),
            ts: now_rfc3339(),
            project_id: project_id.to_string(),
            feature_id: feature_id.map(ToOwned::to_owned),
            task_id: task_id.map(ToOwned::to_owned),
            run_id: run_id.map(ToOwned::to_owned),
            actor: "operator".to_string(),
            mode,
            kind: kind.to_string(),
            payload_ref,
            payload_sha256,
        };
        self.events.append(&event)
    }

    pub fn find_object_path(&self, dir: &Path, id: &str) -> Result<PathBuf> {
        find_object_path(dir, id)
    }
}

fn attach_live_vcs(value: &mut serde_json::Value, live_vcs: serde_json::Value) {
    if let Some(object) = value.as_object_mut() {
        object.insert("live_vcs".to_string(), live_vcs);
    }
}

pub fn project_id(root: &Path) -> Result<String> {
    let canonical_root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let basename = canonical_root
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| anyhow!("unable to infer project id from repo root"))?;
    let mut hasher = Sha256::new();
    hasher.update(canonical_root.to_string_lossy().as_bytes());
    let digest = hex::encode(hasher.finalize());
    Ok(format!("{basename}-{}", &digest[..10]))
}

fn project_label(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project")
        .to_string()
}

fn project_overlay_vcs_mode(repo_root: &Path) -> String {
    use punk_vcs::VcsMode;

    match punk_vcs::detect_mode(repo_root) {
        VcsMode::Jj => "jj".to_string(),
        VcsMode::GitWithJjAvailableButDisabled => "git_degraded".to_string(),
        VcsMode::GitOnly => "git".to_string(),
        VcsMode::NoVcs => "none".to_string(),
    }
}

fn active_project_skill_refs(project_label: &str) -> Vec<String> {
    let bus_dir = std::env::var("PUNK_BUS_DIR")
        .map(PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME")
                .map(PathBuf::from)
                .map(|home| home.join("vicc/state/bus"))
        })
        .unwrap_or_else(|_| PathBuf::from("."));
    let skills_dir = bus_dir.parent().unwrap_or(&bus_dir).join("skills");
    let Ok(entries) = fs::read_dir(&skills_dir) else {
        return Vec::new();
    };

    let mut refs = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md"))
        .filter(|path| path_matches_project_skill(path, project_label))
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    refs.sort();
    refs.dedup();
    refs
}

fn path_matches_project_skill(path: &Path, project_label: &str) -> bool {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    if stem == format!("{project_label}-core") {
        return true;
    }
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    skill_projects(&content)
        .iter()
        .any(|project| project == project_label)
}

fn skill_projects(content: &str) -> Vec<String> {
    let Some(rest) = content.strip_prefix("---\n") else {
        return Vec::new();
    };
    let Some((frontmatter, _)) = rest.split_once("\n---") else {
        return Vec::new();
    };
    for line in frontmatter.lines() {
        let Some(raw) = line.strip_prefix("project:") else {
            continue;
        };
        let trimmed = raw.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            return trimmed[1..trimmed.len() - 1]
                .split(',')
                .map(|item| item.trim().trim_matches('"').to_string())
                .filter(|item| !item.is_empty())
                .collect();
        }
        if !trimmed.is_empty() {
            return vec![trimmed.trim_matches('"').to_string()];
        }
    }
    Vec::new()
}

#[derive(Debug)]
struct ContractRecord {
    path: PathBuf,
    contract: Contract,
}

#[derive(Debug)]
struct RunRecord {
    run_path: PathBuf,
    receipt_path: PathBuf,
    run: Run,
    receipt: Option<Receipt>,
}

#[derive(Debug)]
struct DecisionRecord {
    path: PathBuf,
    decision: punk_domain::DecisionObject,
}

#[derive(Debug)]
struct ProofRecord {
    path: PathBuf,
    proof: punk_domain::Proofpack,
}

enum StaleRunDisposition {
    SafeToArchive(StaleArtifactCandidate),
    ManualReview(StaleArtifactCandidate),
}

struct AutonomyRecordFile {
    path: PathBuf,
    record: AutonomyRecord,
}

fn work_contract_records(contracts_dir: &Path, feature_id: &str) -> Result<Vec<ContractRecord>> {
    let feature_dir = contracts_dir.join(feature_id);
    if !feature_dir.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(feature_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let contract: Contract = read_json(&path)?;
        records.push(ContractRecord { path, contract });
    }
    Ok(records)
}

fn work_run_records(runs_dir: &Path, feature_id: &str) -> Result<Vec<RunRecord>> {
    let mut records = Vec::new();
    if !runs_dir.exists() {
        return Ok(records);
    }
    for entry in fs::read_dir(runs_dir)? {
        let run_dir = entry?.path();
        if !run_dir.is_dir() {
            continue;
        }
        let run_path = run_dir.join("run.json");
        if !run_path.exists() {
            continue;
        }
        let run: Run = read_json(&run_path)?;
        if run.feature_id != feature_id {
            continue;
        }
        let receipt_path = run_dir.join("receipt.json");
        let receipt = if receipt_path.exists() {
            Some(read_json(&receipt_path)?)
        } else {
            None
        };
        records.push(RunRecord {
            run_path,
            receipt_path,
            run,
            receipt,
        });
    }
    Ok(records)
}

fn classify_stale_run_candidate(
    paths: &RepoPaths,
    feature_id: &str,
    run_record: &RunRecord,
) -> Result<Option<StaleRunDisposition>> {
    if run_record.run.status != RunStatus::Running || run_record.receipt.is_some() {
        return Ok(None);
    }

    let run_dir = run_record
        .run_path
        .parent()
        .ok_or_else(|| anyhow!("run path missing parent: {}", run_record.run_path.display()))?;
    let executor = read_executor_process_info(&run_dir.join("executor.json"))?;
    let heartbeat = read_run_heartbeat_optional(&run_dir.join("heartbeat.json"))?;
    let latest_decision = latest_decision_record(&paths.decisions_dir, &run_record.run.id)?;
    if latest_decision.is_some() {
        return Ok(None);
    }

    let Some(reason) = stale_run_reason(
        &run_record.run,
        executor.as_ref(),
        heartbeat.as_ref(),
        false,
    ) else {
        return Ok(None);
    };

    let candidate = StaleArtifactCandidate {
        artifact_kind: "run".to_string(),
        artifact_id: run_record.run.id.clone(),
        work_id: feature_id.to_string(),
        artifact_ref: relative_ref(&paths.repo_root, &run_record.run_path)?,
        reason,
        last_progress_at: heartbeat
            .as_ref()
            .map(|value| value.last_progress_at.clone()),
        executor_pid: executor.as_ref().map(|value| value.child_pid),
    };

    if executor.is_none() || heartbeat.is_none() {
        return Ok(Some(StaleRunDisposition::ManualReview(candidate)));
    }

    Ok(Some(StaleRunDisposition::SafeToArchive(candidate)))
}

fn read_executor_process_info(path: &Path) -> Result<Option<ExecutorProcessInfo>> {
    if !path.exists() {
        return Ok(None);
    }
    read_json(path).map(Some)
}

fn read_run_heartbeat_optional(path: &Path) -> Result<Option<RunHeartbeat>> {
    if !path.exists() {
        return Ok(None);
    }
    read_json(path).map(Some)
}

fn stale_run_reason(
    _run: &Run,
    executor: Option<&ExecutorProcessInfo>,
    heartbeat: Option<&RunHeartbeat>,
    has_decision: bool,
) -> Option<String> {
    if has_decision {
        return None;
    }
    let executor = executor?;
    let heartbeat = heartbeat?;
    if heartbeat.state != "running" {
        return None;
    }
    if process_is_alive(executor.child_pid) {
        return None;
    }
    if !rfc3339_age_exceeds(&heartbeat.last_progress_at, Duration::from_secs(60)) {
        return None;
    }
    Some(format!(
        "status=running but child_pid {} is dead, heartbeat.state=running, last_progress_at={}, no receipt, no decision, no proof",
        executor.child_pid, heartbeat.last_progress_at
    ))
}

fn rfc3339_age_exceeds(timestamp: &str, threshold: Duration) -> bool {
    let Ok(parsed) = DateTime::parse_from_rfc3339(timestamp) else {
        return false;
    };
    let age = Utc::now().signed_duration_since(parsed.with_timezone(&Utc));
    age.to_std()
        .map(|value| value >= threshold)
        .unwrap_or(false)
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn process_is_alive(_pid: u32) -> bool {
    true
}

fn latest_decision_record(decisions_dir: &Path, run_id: &str) -> Result<Option<DecisionRecord>> {
    if !decisions_dir.exists() {
        return Ok(None);
    }
    let mut latest: Option<DecisionRecord> = None;
    for entry in fs::read_dir(decisions_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let decision: punk_domain::DecisionObject = read_json(&path)?;
        if decision.run_id != run_id {
            continue;
        }
        let replace = latest
            .as_ref()
            .map(|current| decision.created_at > current.decision.created_at)
            .unwrap_or(true);
        if replace {
            latest = Some(DecisionRecord { path, decision });
        }
    }
    Ok(latest)
}

fn latest_proof_record(proofs_dir: &Path, decision_id: &str) -> Result<Option<ProofRecord>> {
    if !proofs_dir.exists() {
        return Ok(None);
    }
    let mut latest: Option<ProofRecord> = None;
    let decision_dir = proofs_dir.join(decision_id);
    if !decision_dir.exists() {
        return Ok(None);
    }
    for entry in fs::read_dir(decision_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let proof: punk_domain::Proofpack = read_json(&path)?;
        let replace = latest
            .as_ref()
            .map(|current| proof.created_at > current.proof.created_at)
            .unwrap_or(true);
        if replace {
            latest = Some(ProofRecord { path, proof });
        }
    }
    Ok(latest)
}

fn latest_autonomy_record(
    repo_root: &Path,
    autonomy_dir: &Path,
    feature_id: &str,
    latest_run_id: Option<&str>,
) -> Result<Option<AutonomyRecordFile>> {
    if !autonomy_dir.exists() {
        return Ok(None);
    }
    let feature_dir = autonomy_dir.join(feature_id);
    if !feature_dir.exists() {
        return Ok(None);
    }
    let mut latest: Option<AutonomyRecordFile> = None;
    for entry in fs::read_dir(feature_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let record: AutonomyRecord = read_json(&path)?;
        if let Some(run_id) = latest_run_id {
            let record_run_id =
                work_object_id_from_ref(repo_root, Some(record.run_ref.as_str()), "run_");
            if record_run_id.as_deref() != Some(run_id) {
                continue;
            }
        }
        let replace = latest
            .as_ref()
            .map(|current| record.recorded_at > current.record.recorded_at)
            .unwrap_or(true);
        if replace {
            latest = Some(AutonomyRecordFile { path, record });
        }
    }
    Ok(latest)
}

fn work_lifecycle_state(
    contract: Option<&Contract>,
    run: Option<&Run>,
    decision: Option<&punk_domain::DecisionObject>,
    autonomy: Option<&AutonomyRecord>,
) -> &'static str {
    if let Some(record) = autonomy {
        return match record.autonomy_outcome {
            AutonomyOutcome::Succeeded => "accepted",
            AutonomyOutcome::Blocked => {
                if record.recovery_contract_ref.is_some() {
                    "blocked_ready_for_recovery"
                } else {
                    "blocked"
                }
            }
            AutonomyOutcome::Escalated => {
                if record.recovery_contract_ref.is_some() {
                    "escalated_ready_for_recovery"
                } else {
                    "escalated"
                }
            }
        };
    }
    if let Some(decision) = decision {
        return match decision.decision {
            punk_domain::Decision::Accept => "accepted",
            punk_domain::Decision::Block => "blocked",
            punk_domain::Decision::Escalate => "escalated",
        };
    }
    if let Some(run) = run {
        return match run.status {
            RunStatus::Running => "running",
            RunStatus::Finished | RunStatus::Failed => "awaiting_gate",
            RunStatus::Cancelled => "cancelled",
        };
    }
    if let Some(contract) = contract {
        return match contract.status {
            ContractStatus::Draft => "awaiting_approval",
            ContractStatus::Approved => "ready_to_run",
            ContractStatus::Superseded => "superseded",
            ContractStatus::Cancelled => "cancelled",
        };
    }
    "drafting"
}

fn work_blocked_reason(
    receipt: Option<&Receipt>,
    decision: Option<&punk_domain::DecisionObject>,
    autonomy: Option<&AutonomyRecord>,
) -> Option<String> {
    if let Some(record) = autonomy {
        if matches!(
            record.autonomy_outcome,
            AutonomyOutcome::Blocked | AutonomyOutcome::Escalated
        ) && !record.basis_summary.trim().is_empty()
        {
            return Some(record.basis_summary.clone());
        }
    }
    if let Some(decision) = decision {
        if matches!(
            decision.decision,
            punk_domain::Decision::Block | punk_domain::Decision::Escalate
        ) {
            let summary = decision
                .decision_basis
                .iter()
                .map(|item| item.trim())
                .filter(|item| !item.is_empty())
                .take(2)
                .collect::<Vec<_>>()
                .join("; ");
            if !summary.is_empty() {
                return Some(summary);
            }
            return None;
        }
        return None;
    }
    receipt
        .filter(|receipt| receipt.status == "failure")
        .map(|receipt| receipt.summary.clone())
}

fn work_next_action(
    contract: Option<&Contract>,
    run: Option<&Run>,
    decision: Option<&punk_domain::DecisionObject>,
    proof: Option<&punk_domain::Proofpack>,
    autonomy: Option<&AutonomyRecord>,
) -> (Option<String>, Option<String>) {
    if let Some(record) = autonomy {
        return (
            Some(record.next_action.clone()),
            Some(record.next_action_ref.clone()),
        );
    }
    if let Some(decision) = decision {
        if let Some(proof) = proof {
            return (Some("inspect_proof".to_string()), Some(proof.id.clone()));
        }
        return (
            Some("write_proofpack".to_string()),
            Some(decision.id.clone()),
        );
    }
    if let Some(run) = run {
        return match run.status {
            RunStatus::Running => (Some("wait_for_run".to_string()), Some(run.id.clone())),
            RunStatus::Finished | RunStatus::Failed => {
                (Some("gate_run".to_string()), Some(run.id.clone()))
            }
            RunStatus::Cancelled => (None, None),
        };
    }
    if let Some(contract) = contract {
        return match contract.status {
            ContractStatus::Draft => (
                Some("approve_contract".to_string()),
                Some(contract.id.clone()),
            ),
            ContractStatus::Approved => (Some("cut_run".to_string()), Some(contract.id.clone())),
            ContractStatus::Superseded | ContractStatus::Cancelled => (None, None),
        };
    }
    (None, None)
}

fn work_updated_at(
    feature: &Feature,
    contract: Option<&Contract>,
    run: Option<&Run>,
    receipt: Option<&Receipt>,
    decision: Option<&punk_domain::DecisionObject>,
    proof: Option<&punk_domain::Proofpack>,
    autonomy: Option<&AutonomyRecord>,
) -> String {
    let mut timestamps = vec![feature.updated_at.clone()];
    if let Some(contract) = contract {
        timestamps.push(
            contract
                .approved_at
                .clone()
                .unwrap_or_else(|| contract.created_at.clone()),
        );
    }
    if let Some(run) = run {
        timestamps.push(
            run.ended_at
                .clone()
                .unwrap_or_else(|| run.started_at.clone()),
        );
    }
    if let Some(receipt) = receipt {
        timestamps.push(receipt.created_at.clone());
    }
    if let Some(decision) = decision {
        timestamps.push(decision.created_at.clone());
    }
    if let Some(proof) = proof {
        timestamps.push(proof.created_at.clone());
    }
    if let Some(autonomy) = autonomy {
        timestamps.push(autonomy.recorded_at.clone());
    }
    timestamps.into_iter().max().unwrap_or_else(now_rfc3339)
}

fn work_object_id_from_ref(
    repo_root: &Path,
    reference: Option<&str>,
    prefix: &str,
) -> Option<String> {
    let path = repo_root.join(reference?);
    let value: serde_json::Value = read_json(&path).ok()?;
    let id = value.get("id")?.as_str()?;
    if id.starts_with(prefix) {
        Some(id.to_string())
    } else {
        None
    }
}

fn summarize_decision_basis(basis: &[String]) -> String {
    basis
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join("; ")
}

fn summarize_command_evidence(command_evidence: &[punk_domain::CommandEvidence]) -> Vec<String> {
    command_evidence
        .iter()
        .map(|item| {
            format!(
                "{} {}: {}",
                item.lane,
                check_status_label(&item.status),
                item.command
            )
        })
        .collect()
}

fn check_status_label(status: &punk_domain::CheckStatus) -> &'static str {
    match status {
        punk_domain::CheckStatus::Pass => "pass",
        punk_domain::CheckStatus::Fail => "fail",
        punk_domain::CheckStatus::Partial => "partial",
        punk_domain::CheckStatus::Unverified => "unverified",
    }
}

fn autonomy_outcome_label(outcome: &AutonomyOutcome) -> &'static str {
    match outcome {
        AutonomyOutcome::Succeeded => "succeeded",
        AutonomyOutcome::Blocked => "blocked",
        AutonomyOutcome::Escalated => "escalated",
    }
}

fn autonomy_next_action(
    decision: &punk_domain::DecisionObject,
    recovery_contract_id: Option<&str>,
    proof_id: &str,
) -> (&'static str, String) {
    match (decision.decision.clone(), recovery_contract_id) {
        (punk_domain::Decision::Block | punk_domain::Decision::Escalate, Some(contract_id)) => {
            ("approve_contract", contract_id.to_string())
        }
        _ => ("inspect_proof", proof_id.to_string()),
    }
}

fn work_suggested_command(
    next_action: Option<&str>,
    next_action_ref: Option<&str>,
) -> Option<String> {
    let action = next_action?;
    let reference = next_action_ref?;
    match action {
        "approve_contract" => Some(format!("punk plot approve {reference}")),
        "cut_run" => Some(format!("punk cut run {reference}")),
        "gate_run" => Some(format!("punk gate run {reference}")),
        "write_proofpack" => Some(format!("punk gate proof {reference}")),
        "inspect_proof" => Some(format!("punk inspect {reference} --json")),
        "wait_for_run" => Some(format!("punk status {reference} --json")),
        _ => None,
    }
}

fn should_treat_cut_run_as_already_satisfied(
    contract: &Contract,
    summary: &str,
    preexisting_changed_files: &[String],
) -> bool {
    contract_is_file_bounded(contract)
        && is_cut_run_noop_summary(summary)
        && entry_points_already_changed_before_dispatch(contract, preexisting_changed_files)
}

fn is_cut_run_noop_summary(summary: &str) -> bool {
    summary
        .trim()
        .starts_with("no implementation progress after bounded context dispatch")
}

fn entry_points_already_changed_before_dispatch(
    contract: &Contract,
    preexisting_changed_files: &[String],
) -> bool {
    !contract.entry_points.is_empty()
        && contract.entry_points.iter().all(|entry_point| {
            preexisting_changed_files
                .iter()
                .any(|changed| path_covers_entry_point(changed, entry_point))
        })
}

fn contract_is_file_bounded(contract: &Contract) -> bool {
    !contract.entry_points.is_empty()
        && contract
            .entry_points
            .iter()
            .all(|entry_point| is_file_like_contract_path(entry_point))
        && contract
            .allowed_scope
            .iter()
            .all(|scope| is_file_like_contract_path(scope))
}

fn is_file_like_contract_path(path: &str) -> bool {
    Path::new(path).extension().is_some()
        || matches!(
            path,
            "Cargo.toml" | "Cargo.lock" | "README.md" | "rust-toolchain.toml"
        )
}

fn path_covers_entry_point(changed_path: &str, entry_point: &str) -> bool {
    changed_path == entry_point
        || changed_path
            .strip_prefix(entry_point)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn prune_generated_cargo_lock_if_out_of_scope(
    repo_root: &Path,
    contract: &Contract,
    cargo_lock_existed_before_run: bool,
) -> Result<()> {
    if cargo_lock_existed_before_run || !contract_implies_generated_cargo_lock(contract) {
        return Ok(());
    }
    let cargo_lock = repo_root.join("Cargo.lock");
    if cargo_lock.exists() {
        fs::remove_file(cargo_lock)?;
    }
    Ok(())
}

fn sync_present_isolated_changes_to_repo_root(
    repo_root: &Path,
    workspace_root: &Path,
    changed_files: &[String],
) -> Result<()> {
    if repo_root == workspace_root {
        return Ok(());
    }
    for path in changed_files.iter().filter(|path| {
        !path.starts_with(".punk/")
            && !path.starts_with("target/")
            && !path.starts_with(".playwright-mcp/")
    }) {
        let source = workspace_root.join(path);
        if !source.is_file() {
            continue;
        }
        let destination = repo_root.join(path);
        if source == destination {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&source, &destination)?;
    }
    Ok(())
}

fn sync_present_repo_root_changes_to_isolated_workspace(
    repo_root: &Path,
    workspace_root: &Path,
    changed_files: &[String],
) -> Result<()> {
    if repo_root == workspace_root {
        return Ok(());
    }
    for path in changed_files.iter().filter(|path| {
        !path.starts_with(".punk/")
            && !path.starts_with("target/")
            && !path.starts_with(".playwright-mcp/")
    }) {
        let source = repo_root.join(path);
        if !source.is_file() {
            continue;
        }
        let destination = workspace_root.join(path);
        if source == destination {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&source, &destination)?;
    }
    Ok(())
}

fn ensure_default_gitignore_coverage(project_root: &Path) -> Result<()> {
    let gitignore_path = project_root.join(".gitignore");
    let existing = if gitignore_path.exists() {
        fs::read_to_string(&gitignore_path)?
    } else {
        String::new()
    };
    let merged = merge_default_gitignore_entries(&existing);
    if merged != existing {
        fs::write(gitignore_path, merged)?;
    }
    Ok(())
}

fn merge_default_gitignore_entries(existing: &str) -> String {
    let mut lines = if existing.is_empty() {
        Vec::new()
    } else {
        existing.lines().map(str::to_string).collect::<Vec<_>>()
    };
    if !gitignore_covers_pattern(&lines, ".punk/") {
        lines.push(".punk/".to_string());
    }
    if !gitignore_covers_pattern(&lines, "target/") {
        lines.push("target/".to_string());
    }
    if !gitignore_covers_pattern(&lines, ".playwright-mcp/") {
        lines.push(".playwright-mcp/".to_string());
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn gitignore_covers_pattern(lines: &[String], required: &str) -> bool {
    let aliases: &[&str] = match required {
        ".punk/" => &[".punk/", ".punk"],
        "target/" => &["target/", "target"],
        ".playwright-mcp/" => &[".playwright-mcp/", ".playwright-mcp"],
        _ => &[required],
    };
    lines.iter().any(|line| {
        let trimmed = line.trim();
        aliases.iter().any(|alias| trimmed == *alias)
    })
}

#[derive(Default)]
struct NestedCheckInference {
    manifests: Vec<String>,
    target_checks: Vec<String>,
    integrity_checks: Vec<String>,
}

fn enrich_scan_with_nested_integrity_fallback(
    repo_root: &Path,
    scan: &mut punk_domain::RepoScanSummary,
) -> Result<()> {
    if !scan.candidate_integrity_checks.is_empty() {
        return Ok(());
    }
    let inference = infer_nested_trustworthy_checks(repo_root)?;
    if inference.integrity_checks.is_empty() {
        return Ok(());
    }
    for manifest in inference.manifests {
        push_unique_string(&mut scan.manifests, manifest);
    }
    for check in inference.target_checks {
        push_unique_string(&mut scan.candidate_target_checks, check);
    }
    for check in inference.integrity_checks {
        push_unique_string(&mut scan.candidate_integrity_checks, check);
    }
    push_unique_string(
        &mut scan.notes,
        "inferred trustworthy integrity checks from nested manifests because the repo root had no explicit integrity story".to_string(),
    );
    Ok(())
}

fn infer_nested_trustworthy_checks(repo_root: &Path) -> Result<NestedCheckInference> {
    let mut inference = NestedCheckInference::default();
    collect_nested_trustworthy_checks(repo_root, repo_root, &mut inference)?;
    Ok(inference)
}

fn collect_nested_trustworthy_checks(
    repo_root: &Path,
    current: &Path,
    inference: &mut NestedCheckInference,
) -> Result<()> {
    if current != repo_root {
        let relative_dir = current
            .strip_prefix(repo_root)
            .map_err(|_| anyhow!("failed to relativize {}", current.display()))?;
        if current.join("Cargo.toml").exists() {
            let manifest = relative_dir.join("Cargo.toml");
            let manifest = manifest.to_string_lossy().replace('\\', "/");
            push_unique_string(&mut inference.manifests, manifest.clone());
            let command = format!("cargo test --manifest-path {manifest}");
            push_unique_string(&mut inference.target_checks, command.clone());
            push_unique_string(&mut inference.integrity_checks, command);
        }
        if current.join("package.json").exists() {
            let manifest = relative_dir.join("package.json");
            let manifest = manifest.to_string_lossy().replace('\\', "/");
            push_unique_string(&mut inference.manifests, manifest);
            let scripts = read_package_scripts(&current.join("package.json"))?;
            let package_manager = detect_nested_package_manager(current);
            for script in ["check", "test", "lint", "typecheck"] {
                if scripts.contains_key(script) {
                    let command = nested_package_manager_run(
                        package_manager.as_deref(),
                        relative_dir,
                        script,
                    );
                    push_unique_string(&mut inference.target_checks, command.clone());
                    push_unique_string(&mut inference.integrity_checks, command);
                }
            }
        }
        if current.join("Makefile").exists() {
            let manifest = relative_dir.join("Makefile");
            let manifest = manifest.to_string_lossy().replace('\\', "/");
            push_unique_string(&mut inference.manifests, manifest);
            let makefile = fs::read_to_string(current.join("Makefile"))?;
            if makefile
                .lines()
                .any(|line| line.trim_start().starts_with("test:"))
            {
                let command = format!("make -C {} test", relative_dir.to_string_lossy());
                push_unique_string(&mut inference.target_checks, command.clone());
                push_unique_string(&mut inference.integrity_checks, command);
            }
        }
    }

    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if ignored_nested_name(&name) {
            continue;
        }
        let relative = path
            .strip_prefix(repo_root)
            .map_err(|_| anyhow!("failed to relativize {}", path.display()))?;
        if ignored_nested_relative_path(relative) {
            continue;
        }
        collect_nested_trustworthy_checks(repo_root, &path, inference)?;
    }

    Ok(())
}

fn read_package_scripts(package_json: &Path) -> Result<BTreeMap<String, String>> {
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(package_json).map_err(|err| anyhow!("read {}: {err}", package_json.display()))?,
    )
    .map_err(|err| anyhow!("parse {}: {err}", package_json.display()))?;
    Ok(value
        .get("scripts")
        .and_then(|value| value.as_object())
        .map(|scripts| {
            scripts
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default())
}

fn detect_nested_package_manager(package_dir: &Path) -> Option<String> {
    for (file, pm) in [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lockb", "bun"),
        ("bun.lock", "bun"),
        ("package-lock.json", "npm"),
    ] {
        if package_dir.join(file).exists() {
            return Some(pm.to_string());
        }
    }
    Some("npm".to_string())
}

fn nested_package_manager_run(
    package_manager: Option<&str>,
    relative_dir: &Path,
    script: &str,
) -> String {
    let directory = relative_dir.to_string_lossy();
    match package_manager.unwrap_or("npm") {
        "pnpm" => format!("pnpm --dir {directory} {script}"),
        "yarn" => format!("yarn --cwd {directory} {script}"),
        "bun" => format!("bun --cwd {directory} run {script}"),
        _ => format!("npm --prefix {directory} run {script}"),
    }
}

fn ignored_nested_name(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".jj" | ".punk" | "target" | "node_modules" | ".playwright-mcp"
    )
}

fn ignored_nested_relative_path(relative: &Path) -> bool {
    relative.starts_with("docs/reference-repos")
        || relative.starts_with("docs/research/_delve_runs")
        || relative.starts_with(".build")
}

fn apply_prompt_targeting_bias(
    repo_root: &Path,
    prompt: &str,
    scan: &mut punk_domain::RepoScanSummary,
) {
    augment_mixed_service_backend_candidates(repo_root, prompt, scan);
    prefer_mixed_service_checks(repo_root, prompt, scan);
    prune_generated_noise_candidates(prompt, scan);
    if !prompt_prefers_backend_data(prompt) || prompt_prefers_ui(prompt) {
        return;
    }
    let mut rebalanced = false;
    rebalanced |= rebalance_candidate_paths(
        &mut scan.candidate_file_scope_paths,
        path_looks_backend_data,
        path_looks_ui_surface,
    );
    rebalanced |= prune_demoted_candidates_if_preferred_exists(
        &mut scan.candidate_file_scope_paths,
        path_looks_backend_data,
        path_looks_ui_surface,
    );
    rebalanced |= rebalance_candidate_paths(
        &mut scan.candidate_entry_points,
        path_looks_backend_data,
        path_looks_ui_surface,
    );
    rebalanced |= prune_demoted_candidates_if_preferred_exists(
        &mut scan.candidate_entry_points,
        path_looks_backend_data,
        path_looks_ui_surface,
    );
    rebalanced |= rebalance_candidate_paths(
        &mut scan.candidate_scope_paths,
        path_looks_backend_data,
        path_looks_ui_surface,
    );
    rebalanced |= rebalance_candidate_paths(
        &mut scan.candidate_directory_scope_paths,
        path_looks_backend_data,
        path_looks_ui_surface,
    );
    if rebalanced {
        push_unique_string(
            &mut scan.notes,
            "biased candidate targeting toward backend/data surfaces because the prompt looked non-UI".to_string(),
        );
    }
}

fn augment_mixed_service_backend_candidates(
    repo_root: &Path,
    prompt: &str,
    scan: &mut punk_domain::RepoScanSummary,
) {
    if !prompt_requests_mixed_service_scope(prompt, scan) {
        return;
    }
    let Ok(anchors) = collect_mixed_service_backend_anchors(repo_root) else {
        return;
    };
    if anchors.files.is_empty() && anchors.directories.is_empty() {
        return;
    }

    let mut files = anchors.files;
    let mut directories = anchors.directories;
    files.sort_by_key(|path| mixed_service_file_anchor_rank(path));
    directories.sort_by_key(|path| mixed_service_directory_anchor_rank(path));

    prepend_candidate_paths(&mut scan.candidate_entry_points, &files, 10);
    prepend_candidate_paths(&mut scan.candidate_file_scope_paths, &files, 20);
    prepend_candidate_paths(&mut scan.candidate_scope_paths, &files, 20);
    prepend_candidate_paths(&mut scan.candidate_directory_scope_paths, &directories, 20);
    prepend_candidate_paths(&mut scan.candidate_scope_paths, &directories, 20);
    push_unique_string(
        &mut scan.notes,
        "augmented candidate targeting with mixed service backend anchors from the repo tree"
            .to_string(),
    );
}

#[derive(Default)]
struct MixedServiceBackendAnchors {
    files: Vec<String>,
    directories: Vec<String>,
}

fn collect_mixed_service_backend_anchors(repo_root: &Path) -> Result<MixedServiceBackendAnchors> {
    let mut anchors = MixedServiceBackendAnchors::default();
    collect_mixed_service_backend_anchors_inner(repo_root, repo_root, &mut anchors)?;
    Ok(anchors)
}

fn collect_mixed_service_backend_anchors_inner(
    repo_root: &Path,
    current: &Path,
    anchors: &mut MixedServiceBackendAnchors,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if ignored_nested_name(&name) {
            continue;
        }
        let relative = path
            .strip_prefix(repo_root)
            .map_err(|_| anyhow!("failed to relativize {}", path.display()))?;
        if ignored_nested_relative_path(relative) {
            continue;
        }
        if path.is_dir() {
            let relative_str = relative.to_string_lossy().replace('\\', "/");
            if path_looks_mixed_service_directory_anchor(&relative_str) {
                push_unique_string(&mut anchors.directories, relative_str);
            }
            collect_mixed_service_backend_anchors_inner(repo_root, &path, anchors)?;
            continue;
        }

        let relative_str = relative.to_string_lossy().replace('\\', "/");
        if path_looks_mixed_service_file_anchor(&relative_str) {
            push_unique_string(&mut anchors.files, relative_str.clone());
        }
        if let Some(parent) = Path::new(&relative_str).parent() {
            let parent = parent.to_string_lossy().replace('\\', "/");
            if !parent.is_empty() && path_looks_mixed_service_directory_anchor(&parent) {
                push_unique_string(&mut anchors.directories, parent);
            }
        }
    }
    Ok(())
}

fn prepend_candidate_paths(existing: &mut Vec<String>, preferred: &[String], limit: usize) {
    let mut merged = preferred.to_vec();
    for candidate in existing.drain(..) {
        if !merged.iter().any(|existing| existing == &candidate) {
            merged.push(candidate);
        }
    }
    *existing = merged.into_iter().take(limit).collect();
}

fn prefer_mixed_service_checks(
    repo_root: &Path,
    prompt: &str,
    scan: &mut punk_domain::RepoScanSummary,
) {
    if !prompt_requests_mixed_service_scope(prompt, scan) {
        return;
    }

    let mut target_checks = Vec::new();
    let mut integrity_checks = Vec::new();

    if let Some(command) = preferred_mixed_service_rust_check(repo_root, prompt, scan) {
        push_unique_string(&mut target_checks, command);
    }

    if let Some((integrity, target)) = preferred_mixed_service_node_checks(repo_root, scan) {
        push_unique_string(&mut integrity_checks, integrity.clone());
        push_unique_string(&mut target_checks, target.unwrap_or(integrity));
    }

    if target_checks.is_empty() || integrity_checks.is_empty() {
        return;
    }

    scan.candidate_target_checks = target_checks;
    scan.candidate_integrity_checks = integrity_checks;
    push_unique_string(
        &mut scan.notes,
        "preferred mixed service checks for combined Node and Rust service work".to_string(),
    );
}

fn preferred_mixed_service_rust_check(
    repo_root: &Path,
    prompt: &str,
    scan: &punk_domain::RepoScanSummary,
) -> Option<String> {
    let prompt_tokens = prompt_tokens(prompt);
    let mut manifests = scan
        .candidate_file_scope_paths
        .iter()
        .filter(|path| path.ends_with("Cargo.toml") && path.contains("crates/"))
        .cloned()
        .collect::<Vec<_>>();
    manifests.sort_by_key(|path| mixed_service_rust_manifest_rank(path, &prompt_tokens));
    for manifest in manifests {
        let manifest_path = repo_root.join(&manifest);
        let parent = manifest_path.parent()?;
        if !parent.join("src/main.rs").exists() {
            continue;
        }
        let Some(package_name) = cargo_package_name_from_manifest(&manifest_path) else {
            continue;
        };
        return Some(format!("cargo check -p {package_name}"));
    }
    None
}

fn mixed_service_rust_manifest_rank(path: &str, prompt_tokens: &[String]) -> u8 {
    let lowered = path.to_ascii_lowercase();
    if lowered.contains("baseline-cli") || lowered.contains("/cli/") {
        return 0;
    }
    if prompt_tokens
        .iter()
        .any(|token| lowered.contains(token.as_str()))
    {
        return 1;
    }
    2
}

fn cargo_package_name_from_manifest(manifest_path: &Path) -> Option<String> {
    let contents = fs::read_to_string(manifest_path).ok()?;
    let mut in_package = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package || !trimmed.starts_with("name") {
            continue;
        }
        let (_, value) = trimmed.split_once('=')?;
        let package_name = value.trim().trim_matches('"');
        if !package_name.is_empty() {
            return Some(package_name.to_string());
        }
    }
    None
}

fn preferred_mixed_service_node_checks(
    repo_root: &Path,
    scan: &punk_domain::RepoScanSummary,
) -> Option<(String, Option<String>)> {
    let package_json = scan
        .candidate_file_scope_paths
        .iter()
        .find(|path| path.ends_with("package.json") && path.contains('/'))
        .or_else(|| {
            scan.candidate_file_scope_paths
                .iter()
                .find(|path| path.ends_with("package.json"))
        })?
        .clone();
    let package_dir = Path::new(&package_json).parent()?.to_path_buf();
    let scripts = read_package_scripts(&repo_root.join(&package_json)).ok()?;
    if !scripts.contains_key("check") {
        return None;
    }
    let package_manager = detect_nested_package_manager(&repo_root.join(&package_dir));
    let integrity = nested_package_manager_run(package_manager.as_deref(), &package_dir, "check");
    let target = if scripts.contains_key("build:web") {
        Some(nested_package_manager_run(
            package_manager.as_deref(),
            &package_dir,
            "build:web",
        ))
    } else if scripts.contains_key("build") {
        Some(nested_package_manager_run(
            package_manager.as_deref(),
            &package_dir,
            "build",
        ))
    } else {
        None
    };
    Some((integrity, target))
}

fn prune_generated_noise_candidates(prompt: &str, scan: &mut punk_domain::RepoScanSummary) {
    if prompt_mentions_generated_surface(prompt) {
        return;
    }
    let mut pruned = false;
    pruned |= retain_non_generated_noise(&mut scan.candidate_file_scope_paths);
    pruned |= retain_non_generated_noise(&mut scan.candidate_entry_points);
    pruned |= retain_non_generated_noise(&mut scan.candidate_scope_paths);
    pruned |= retain_non_generated_noise(&mut scan.candidate_directory_scope_paths);
    if pruned {
        push_unique_string(
            &mut scan.notes,
            "pruned generated/output surfaces like dist and packs from candidate targeting"
                .to_string(),
        );
    }
}

fn retain_non_generated_noise(paths: &mut Vec<String>) -> bool {
    let before_len = paths.len();
    paths.retain(|path| !path_is_generated_noise(path));
    before_len != paths.len()
}

fn rebalance_candidate_paths(
    paths: &mut Vec<String>,
    prefer: fn(&str) -> bool,
    demote: fn(&str) -> bool,
) -> bool {
    if !paths.iter().any(|path| prefer(path)) {
        return false;
    }
    let original = paths.clone();
    let mut preferred = Vec::new();
    let mut neutral = Vec::new();
    let mut demoted = Vec::new();
    for path in original.iter() {
        if prefer(path) {
            preferred.push(path.clone());
        } else if demote(path) {
            demoted.push(path.clone());
        } else {
            neutral.push(path.clone());
        }
    }
    let mut reordered = Vec::with_capacity(original.len());
    reordered.extend(preferred);
    reordered.extend(neutral);
    reordered.extend(demoted);
    if reordered == original {
        return false;
    }
    *paths = reordered;
    true
}

fn prune_demoted_candidates_if_preferred_exists(
    paths: &mut Vec<String>,
    prefer: fn(&str) -> bool,
    demote: fn(&str) -> bool,
) -> bool {
    if !paths.iter().any(|path| prefer(path)) {
        return false;
    }
    let before_len = paths.len();
    paths.retain(|path| !demote(path) || prefer(path));
    before_len != paths.len()
}

fn prompt_tokens(prompt: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    for token in prompt
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| token.len() >= 3)
    {
        seen.insert(token);
    }
    seen.into_iter().collect()
}

fn prompt_prefers_backend_data(prompt: &str) -> bool {
    let lowered = prompt.to_ascii_lowercase();
    [
        " db",
        "db ",
        "database",
        "schema",
        "migration",
        "session",
        "seed",
        "service",
        "services",
        "dispatch",
        "profile",
        "profiles",
        "track",
        "tracks",
        "enrollment",
        "enrollments",
        "support",
        "backend",
        "server",
        "runtime",
        "token",
        "activation",
        "cli",
        "store",
        "repo",
        "model",
        "models",
        "api",
        "enroll",
        "handshake",
    ]
    .iter()
    .any(|token| lowered.contains(token))
}

fn prompt_prefers_ui(prompt: &str) -> bool {
    let lowered = prompt.to_ascii_lowercase();
    [
        " ui",
        "frontend",
        "front-end",
        "header",
        "footer",
        "layout",
        "layouts",
        "page",
        "pages",
        "component",
        "components",
        ".astro",
        "tailwind",
        "css",
        "landing",
    ]
    .iter()
    .any(|token| lowered.contains(token))
}

fn prompt_mentions_generated_surface(prompt: &str) -> bool {
    let lowered = prompt.to_ascii_lowercase();
    lowered.contains("dist")
        || lowered.contains("pack")
        || lowered.contains("bundle")
        || lowered.contains(".astro")
}

fn path_is_generated_noise(path: &str) -> bool {
    let lowered = path.to_ascii_lowercase();
    lowered == "dist"
        || lowered == "packs"
        || lowered == ".astro"
        || lowered.starts_with("dist/")
        || lowered.starts_with("packs/")
        || lowered.starts_with(".astro/")
        || lowered.contains("/dist/")
        || lowered.contains("/packs/")
        || lowered.contains("/.astro/")
}

fn path_looks_ui_surface(path: &str) -> bool {
    let lowered = path.to_ascii_lowercase();
    if lowered.contains("/pages/api/") {
        return false;
    }
    lowered.ends_with(".astro")
        || lowered.contains("astro.config")
        || lowered.ends_with(".css")
        || lowered.ends_with(".scss")
        || lowered.ends_with(".sass")
        || lowered.contains("/components/")
        || lowered.contains("/layouts/")
        || lowered.contains("/pages/")
        || lowered.contains("/styles/")
        || lowered.contains("/public/")
        || lowered.contains("header")
        || lowered.contains("footer")
}

fn path_looks_backend_data(path: &str) -> bool {
    let lowered = path.to_ascii_lowercase();
    lowered.ends_with("package.json")
        || lowered.contains("drizzle.config")
        || lowered.ends_with(".sql")
        || lowered.contains("/db/")
        || lowered.contains("/lib/services/")
        || lowered.contains("/lib/session/")
        || lowered.contains("/lib/db/")
        || lowered.contains("/database/")
        || lowered.contains("/lib/persistence/")
        || lowered.contains("/actions/")
        || lowered.contains("/pages/api/")
        || lowered.contains("schema")
        || lowered.contains("migration")
        || lowered.contains("session")
        || lowered.contains("seed")
        || lowered.contains("service")
        || lowered.contains("dispatch")
        || lowered.contains("profile")
        || lowered.contains("track")
        || lowered.contains("enrollment")
        || lowered.contains("support")
        || lowered.contains("/server/")
        || lowered.contains("/api/")
        || lowered.contains("/store/")
        || lowered.contains("/repo/")
        || lowered.contains("/model")
        || lowered.contains("/data/")
}

fn contract_implies_generated_cargo_lock(contract: &Contract) -> bool {
    !contract
        .allowed_scope
        .iter()
        .any(|path| path == "Cargo.lock")
        && contract
            .target_checks
            .iter()
            .chain(contract.integrity_checks.iter())
            .any(|command| command.trim_start().starts_with("cargo "))
}

fn already_satisfied_before_dispatch_summary(
    entry_points: &[String],
    original_summary: &str,
) -> String {
    let original_summary = original_summary.trim();
    if original_summary.is_empty() {
        format!(
            "already satisfied in allowed scope before bounded dispatch: {}",
            entry_points.join(", ")
        )
    } else {
        format!(
            "already satisfied in allowed scope before bounded dispatch: {} (original executor summary: {})",
            entry_points.join(", "),
            original_summary
        )
    }
}

fn should_reject_empty_successful_bounded_run(
    contract: &Contract,
    status: &str,
    summary: &str,
    changed_files: &[String],
) -> bool {
    status == "success"
        && changed_files.is_empty()
        && contract_has_non_manifest_entry_points(contract)
        && !summary
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
    if !is_file_like_contract_path(path) {
        return false;
    }
    !matches!(
        path,
        "Cargo.toml" | "Cargo.lock" | "README.md" | "rust-toolchain.toml"
    )
}

fn empty_successful_bounded_run_summary(entry_points: &[String], original_summary: &str) -> String {
    let scope = if entry_points.is_empty() {
        "approved entry points".to_string()
    } else {
        entry_points.join(", ")
    };
    let trimmed = original_summary.trim();
    if trimmed.is_empty() {
        format!(
            "no implementation progress after bounded success report in {}: executor reported success without observable repo changes",
            scope
        )
    } else {
        format!(
            "no implementation progress after bounded success report in {}: executor reported success without observable repo changes (original executor summary: {})",
            scope, trimmed
        )
    }
}

fn summarize_prompt(prompt: &str) -> String {
    let trimmed = prompt.trim();
    trimmed.chars().take(60).collect()
}

fn is_drafter_timeout_error(err: &anyhow::Error) -> bool {
    err.to_string()
        .to_ascii_lowercase()
        .contains("timed out after")
}

fn recover_timeout_draft_proposal(
    repo_root: &Path,
    prompt: &str,
    scan: &punk_domain::RepoScanSummary,
) -> Result<DraftProposal> {
    finalize_timeout_fallback_proposal(
        repo_root,
        prompt,
        None,
        timeout_seed_proposal(prompt, scan),
        scan,
    )
}

fn recover_timeout_refine_proposal(
    repo_root: &Path,
    prompt: &str,
    guidance: &str,
    current: DraftProposal,
    scan: &punk_domain::RepoScanSummary,
) -> Result<DraftProposal> {
    finalize_timeout_fallback_proposal(repo_root, prompt, Some(guidance), current, scan)
}

fn finalize_timeout_fallback_proposal(
    repo_root: &Path,
    prompt: &str,
    guidance: Option<&str>,
    mut proposal: DraftProposal,
    scan: &punk_domain::RepoScanSummary,
) -> Result<DraftProposal> {
    canonicalize_draft_proposal(repo_root, prompt, &mut proposal);
    if let Some(guidance) = guidance {
        apply_explicit_prompt_overrides(repo_root, guidance, &mut proposal);
    }
    preserve_greenfield_scaffold_scope(prompt, scan, &mut proposal);
    preserve_mixed_service_scope(prompt, scan, &mut proposal);
    ensure_proposal_scope_covers_entry_points(&mut proposal);
    let mut errors = validate_draft_proposal(repo_root, &proposal);
    if errors.is_empty() {
        return Ok(proposal);
    }

    if let Some(mut fallback) =
        build_bounded_fallback_proposal(repo_root, prompt, &proposal, scan, &errors)
    {
        canonicalize_draft_proposal(repo_root, prompt, &mut fallback);
        if let Some(guidance) = guidance {
            apply_explicit_prompt_overrides(repo_root, guidance, &mut fallback);
        }
        preserve_greenfield_scaffold_scope(prompt, scan, &mut fallback);
        preserve_mixed_service_scope(prompt, scan, &mut fallback);
        ensure_proposal_scope_covers_entry_points(&mut fallback);
        errors = validate_draft_proposal(repo_root, &fallback);
        if errors.is_empty() {
            return Ok(fallback);
        }
    }

    Err(anyhow!(
        "timed-out drafter proposal could not be recovered: {}",
        format_validation_guidance(&errors)
    ))
}

fn preserve_greenfield_scaffold_scope(
    prompt: &str,
    scan: &punk_domain::RepoScanSummary,
    proposal: &mut DraftProposal,
) {
    if prompt_declares_explicit_touch_set(prompt) {
        return;
    }
    let Some((entry_points, allowed_scope)) = timeout_greenfield_scaffold_scope(prompt, scan)
    else {
        return;
    };

    for entry_point in entry_points {
        if !proposal
            .entry_points
            .iter()
            .any(|existing| existing == &entry_point)
        {
            proposal.entry_points.push(entry_point);
        }
    }

    for scope_path in allowed_scope {
        if !proposal
            .allowed_scope
            .iter()
            .any(|existing| existing == &scope_path)
        {
            proposal.allowed_scope.push(scope_path);
        }
    }
}

fn normalize_proposal_scope(
    prompt: &str,
    scan: &punk_domain::RepoScanSummary,
    proposal: &mut DraftProposal,
) {
    preserve_greenfield_scaffold_scope(prompt, scan, proposal);
    preserve_mixed_service_scope(prompt, scan, proposal);
    apply_prompt_exclusion_pruning(prompt, proposal);
    ensure_proposal_scope_covers_entry_points(proposal);
}

fn preserve_mixed_service_scope(
    prompt: &str,
    scan: &punk_domain::RepoScanSummary,
    proposal: &mut DraftProposal,
) {
    if prompt_declares_explicit_touch_set(prompt)
        || !prompt_requests_mixed_service_scope(prompt, scan)
    {
        return;
    }

    let mut preferred_files = scan
        .candidate_file_scope_paths
        .iter()
        .filter(|path| path_looks_mixed_service_file_anchor(path))
        .cloned()
        .collect::<Vec<_>>();
    let mut preferred_dirs = scan
        .candidate_directory_scope_paths
        .iter()
        .filter(|path| path_looks_mixed_service_directory_anchor(path))
        .cloned()
        .collect::<Vec<_>>();

    if preferred_files.is_empty() && preferred_dirs.is_empty() {
        return;
    }

    preferred_files.sort_by_key(|path| mixed_service_file_anchor_rank(path));
    preferred_dirs.sort_by_key(|path| mixed_service_directory_anchor_rank(path));

    for path in preferred_files.into_iter().take(8) {
        if !proposal
            .allowed_scope
            .iter()
            .any(|existing| existing == &path)
        {
            proposal.allowed_scope.push(path);
        }
    }

    for path in preferred_dirs.into_iter().take(4) {
        if !proposal
            .allowed_scope
            .iter()
            .any(|existing| existing == &path)
        {
            proposal.allowed_scope.push(path);
        }
    }
}

fn prompt_requests_mixed_service_scope(prompt: &str, scan: &punk_domain::RepoScanSummary) -> bool {
    if !prompt_prefers_backend_data(prompt) || prompt_prefers_ui(prompt) {
        return false;
    }
    let lowered = prompt.to_ascii_lowercase();
    let service_intent = [
        "session",
        "dispatch",
        "handoff",
        "bridge",
        "transaction",
        "service",
        "services",
        "api",
        "cli",
        "runtime",
        "token",
        "activation",
        "enroll",
        "operator_session",
        "probe_state",
    ]
    .iter()
    .any(|needle| lowered.contains(needle));
    if !service_intent {
        return false;
    }

    let mentions_rust = ["rust", "cargo", "crate", "crates"]
        .iter()
        .any(|needle| lowered.contains(needle))
        || scan_has_rust_service_surface(scan);
    let mentions_node = ["node", "npm", "package.json", "astro", "typescript", "ts"]
        .iter()
        .any(|needle| lowered.contains(needle))
        || scan_has_node_service_surface(scan);

    mentions_rust && mentions_node
}

fn scan_has_rust_service_surface(scan: &punk_domain::RepoScanSummary) -> bool {
    scan.candidate_file_scope_paths
        .iter()
        .any(|path| path.ends_with("Cargo.toml"))
        || scan.candidate_directory_scope_paths.iter().any(|path| {
            path == "crates" || path.starts_with("crates/") || path.contains("/crates/")
        })
}

fn scan_has_node_service_surface(scan: &punk_domain::RepoScanSummary) -> bool {
    scan.candidate_file_scope_paths
        .iter()
        .any(|path| path.ends_with("package.json"))
}

fn path_looks_mixed_service_file_anchor(path: &str) -> bool {
    let lowered = path.to_ascii_lowercase();
    lowered.ends_with("package.json")
        || lowered.ends_with("cargo.toml")
        || lowered.contains("drizzle.config")
        || lowered.contains("/lib/services/")
        || lowered.contains("/lib/session/")
        || lowered.contains("/lib/db/")
        || lowered.contains("/lib/persistence/")
        || lowered.contains("/actions/")
        || lowered.contains("/pages/api/")
        || lowered.contains("/api/")
        || lowered.contains("/server/")
        || (lowered.starts_with("crates/") && lowered.ends_with(".rs"))
}

fn mixed_service_file_anchor_rank(path: &str) -> u8 {
    let lowered = path.to_ascii_lowercase();
    if lowered.ends_with("package.json") {
        0
    } else if lowered == "cargo.toml" {
        1
    } else if lowered.ends_with("cargo.toml") {
        2
    } else if lowered.ends_with("/src/main.rs") {
        3
    } else if lowered.contains("drizzle.config") {
        4
    } else if lowered.contains("/pages/api/") {
        5
    } else if lowered.contains("/lib/services/") || lowered.contains("/lib/session/") {
        6
    } else if lowered.contains("/lib/db/") || lowered.contains("/lib/persistence/") {
        7
    } else if lowered.contains("/actions/")
        || lowered.contains("/api/")
        || lowered.contains("/server/")
    {
        8
    } else {
        9
    }
}

fn path_looks_mixed_service_directory_anchor(path: &str) -> bool {
    let lowered = path.to_ascii_lowercase();
    lowered == "crates"
        || lowered.starts_with("crates/")
        || lowered.contains("/lib/services")
        || lowered.contains("/lib/session")
        || lowered.contains("/lib/db")
        || lowered.contains("/lib/persistence")
        || lowered.contains("/actions")
        || lowered.contains("/pages/api")
        || lowered.contains("/api")
        || lowered.contains("/server")
}

fn mixed_service_directory_anchor_rank(path: &str) -> u8 {
    let lowered = path.to_ascii_lowercase();
    if lowered == "crates" || lowered.starts_with("crates/") {
        0
    } else if lowered.contains("/lib/services") || lowered.contains("/lib/session") {
        1
    } else if lowered.contains("/lib/db") || lowered.contains("/lib/persistence") {
        2
    } else if lowered.contains("/pages/api") {
        3
    } else if lowered.contains("/actions") {
        4
    } else {
        5
    }
}

fn apply_prompt_exclusion_pruning(prompt: &str, proposal: &mut DraftProposal) {
    let excluded = extract_prompt_excluded_scope_prefixes(prompt);
    if excluded.is_empty() {
        return;
    }
    proposal
        .entry_points
        .retain(|path| !path_matches_excluded_prefixes(path, &excluded));
    proposal
        .allowed_scope
        .retain(|path| !path_matches_excluded_prefixes(path, &excluded));
}

fn extract_prompt_excluded_scope_prefixes(prompt: &str) -> Vec<String> {
    let mut prefixes = Vec::new();
    for line in prompt.lines() {
        let lowered = line.to_ascii_lowercase();
        if ![
            "exclude",
            "excluding",
            "do not touch",
            "don't touch",
            "must not touch",
            "not in scope",
            "out of scope",
            "without touching",
            "without modifying",
        ]
        .iter()
        .any(|marker| lowered.contains(marker))
        {
            continue;
        }
        for token in line.split_whitespace() {
            let Some(prefix) = normalize_prompt_scope_token(token) else {
                continue;
            };
            if !prefixes.iter().any(|existing| existing == &prefix) {
                prefixes.push(prefix);
            }
        }
    }
    prefixes
}

fn normalize_prompt_scope_token(token: &str) -> Option<String> {
    let mut trimmed = token
        .trim_matches(|ch: char| {
            matches!(
                ch,
                ',' | ';' | ':' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '\'' | '`'
            )
        })
        .trim()
        .to_string();
    if trimmed.is_empty() {
        return None;
    }
    while trimmed.ends_with('*') {
        trimmed.pop();
    }
    while trimmed.ends_with('/') {
        trimmed.pop();
    }
    if trimmed.is_empty() || !trimmed.contains('/') {
        return None;
    }
    Some(trimmed)
}

fn path_matches_excluded_prefixes(path: &str, excluded: &[String]) -> bool {
    let normalized = path.trim().trim_matches('/');
    excluded.iter().any(|prefix| {
        let prefix = prefix.trim().trim_matches('/');
        normalized == prefix || normalized.starts_with(&format!("{prefix}/"))
    })
}

fn prompt_declares_explicit_touch_set(prompt: &str) -> bool {
    let lowered = prompt.to_ascii_lowercase();
    [
        "touching exactly",
        "touch exactly",
        "exact touch set",
        "requested touch set",
        "scope bounded to",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

fn ensure_proposal_scope_covers_entry_points(proposal: &mut DraftProposal) {
    let missing_entry_points = proposal
        .entry_points
        .iter()
        .filter(|entry_point| {
            !proposal_scope_covers_entry_point(&proposal.allowed_scope, entry_point)
        })
        .cloned()
        .collect::<Vec<_>>();
    for entry_point in missing_entry_points {
        if !proposal
            .allowed_scope
            .iter()
            .any(|existing| existing == &entry_point)
        {
            proposal.allowed_scope.push(entry_point);
        }
    }
}

fn proposal_scope_covers_entry_point(allowed_scope: &[String], entry_point: &str) -> bool {
    let entry = entry_point.trim().trim_matches('/');
    allowed_scope.iter().any(|scope| {
        let scope = scope.trim().trim_matches('/');
        entry == scope || entry.starts_with(&format!("{scope}/"))
    })
}

fn timeout_seed_proposal(prompt: &str, scan: &punk_domain::RepoScanSummary) -> DraftProposal {
    let greenfield_scaffold_scope = timeout_greenfield_scaffold_scope(prompt, scan);
    let entry_points: Vec<String> = if let Some((entry_points, _)) = &greenfield_scaffold_scope {
        entry_points.clone()
    } else if scan.candidate_entry_points.is_empty() {
        scan.candidate_file_scope_paths
            .iter()
            .take(2)
            .cloned()
            .collect()
    } else {
        scan.candidate_entry_points
            .iter()
            .take(2)
            .cloned()
            .collect()
    };
    let allowed_scope: Vec<String> = if let Some((_, allowed_scope)) = greenfield_scaffold_scope {
        allowed_scope
    } else if scan.candidate_file_scope_paths.is_empty() {
        entry_points.clone()
    } else {
        scan.candidate_file_scope_paths
            .iter()
            .take(4)
            .cloned()
            .collect()
    };
    let target_checks = if scan.candidate_target_checks.is_empty() {
        scan.candidate_integrity_checks.clone()
    } else {
        scan.candidate_target_checks.clone()
    };
    let (expected_interfaces, behavior_requirements) =
        timeout_seed_semantics(prompt, &entry_points);

    DraftProposal {
        title: summarize_prompt(prompt),
        summary: summarize_prompt(prompt),
        entry_points,
        import_paths: Vec::new(),
        expected_interfaces,
        behavior_requirements,
        allowed_scope,
        target_checks,
        integrity_checks: scan.candidate_integrity_checks.clone(),
        risk_level: "medium".to_string(),
    }
}

fn timeout_seed_semantics(prompt: &str, entry_points: &[String]) -> (Vec<String>, Vec<String>) {
    let expected_interface = match entry_points.first().map(String::as_str) {
        Some("Cargo.toml") => "initial Rust scaffold",
        Some("go.mod") => "initial Go scaffold",
        Some("pyproject.toml") => "initial Python scaffold",
        Some("package.json") => "initial TypeScript/Node scaffold",
        _ => "bounded implementation slice",
    };
    let mut expected_interfaces = vec![expected_interface.to_string()];
    let mut behavior_requirements = vec![summarize_prompt(prompt)];
    let lowered = prompt.to_ascii_lowercase();

    if lowered.contains(" init ")
        || lowered.starts_with("init ")
        || lowered.contains("init command")
    {
        push_unique_string(
            &mut expected_interfaces,
            "CLI accepts an `init` command.".to_string(),
        );
    }
    for flag in ["--json", "--force", "--project-root"] {
        if lowered.contains(flag) {
            push_unique_string(&mut expected_interfaces, format!("CLI supports `{flag}`."));
            push_unique_string(
                &mut behavior_requirements,
                format!("Support `{flag}` in the init flow."),
            );
        }
    }
    let starter_files = timeout_prompt_starter_files(prompt);
    if !starter_files.is_empty() {
        push_unique_string(
            &mut expected_interfaces,
            format!(
                "Init creates canonical starter files: {}.",
                starter_files.join(", ")
            ),
        );
        push_unique_string(
            &mut behavior_requirements,
            format!(
                "Create canonical starter files: {}.",
                starter_files.join(", ")
            ),
        );
    }
    if lowered.contains("test") {
        push_unique_string(
            &mut expected_interfaces,
            "Tests cover the init command behavior.".to_string(),
        );
    }

    (expected_interfaces, behavior_requirements)
}

fn timeout_greenfield_scaffold_scope(
    prompt: &str,
    scan: &punk_domain::RepoScanSummary,
) -> Option<(Vec<String>, Vec<String>)> {
    let manifest = scan
        .candidate_file_scope_paths
        .iter()
        .find(|path| {
            matches!(
                path.as_str(),
                "Cargo.toml" | "go.mod" | "pyproject.toml" | "package.json"
            )
        })?
        .clone();
    if !prompt_requests_timeout_greenfield_scaffold(prompt, &manifest) {
        return None;
    }

    let preferred_directories: &[&str] = match manifest.as_str() {
        "Cargo.toml" => &["crates", "src", "tests"],
        "go.mod" => &["cmd", "internal", "pkg"],
        "pyproject.toml" => &["src", "tests"],
        "package.json" => &["packages", "apps", "src", "tests"],
        _ => &[],
    };
    let preferred_files: &[&str] = match manifest.as_str() {
        "package.json" => &["tsconfig.json"],
        _ => &[],
    };
    if preferred_directories.is_empty() {
        return None;
    }

    let entry_points = vec![manifest.clone()];
    let mut allowed_scope = entry_points.clone();
    for candidate in &scan.candidate_file_scope_paths {
        if preferred_files.contains(&candidate.as_str())
            && !allowed_scope.iter().any(|existing| existing == candidate)
        {
            allowed_scope.push(candidate.clone());
        }
    }
    for candidate in &scan.candidate_directory_scope_paths {
        if preferred_directories.contains(&candidate.as_str())
            && !allowed_scope.iter().any(|existing| existing == candidate)
        {
            allowed_scope.push(candidate.clone());
        }
    }

    Some((entry_points, allowed_scope))
}

fn prompt_requests_timeout_greenfield_scaffold(prompt: &str, manifest: &str) -> bool {
    let lowered = prompt.to_ascii_lowercase();
    match manifest {
        "Cargo.toml" => {
            let requests_rust = ["rust", "cargo", "workspace", "crate", "crates"]
                .iter()
                .any(|needle| lowered.contains(needle));
            let requests_scaffold = ["scaffold", "bootstrap", "greenfield"]
                .iter()
                .any(|needle| lowered.contains(needle))
                || (lowered.contains("create") && lowered.contains("workspace"));
            requests_rust && requests_scaffold
        }
        "go.mod" => {
            let requests_go = ["go", "golang", "module"]
                .iter()
                .any(|needle| lowered.contains(needle));
            let requests_scaffold = ["scaffold", "bootstrap", "greenfield"]
                .iter()
                .any(|needle| lowered.contains(needle))
                || (lowered.contains("create") && lowered.contains("module"));
            requests_go && requests_scaffold
        }
        "pyproject.toml" => {
            let requests_python = ["python", "pytest", "pyproject", "package"]
                .iter()
                .any(|needle| lowered.contains(needle));
            let requests_scaffold = ["scaffold", "bootstrap", "greenfield"]
                .iter()
                .any(|needle| lowered.contains(needle))
                || (lowered.contains("create") && lowered.contains("project"));
            requests_python && requests_scaffold
        }
        "package.json" => {
            let requests_node = [
                "typescript",
                "javascript",
                "node",
                "npm",
                "pnpm",
                "yarn",
                "package",
                "workspace",
            ]
            .iter()
            .any(|needle| lowered.contains(needle));
            let requests_scaffold = ["scaffold", "bootstrap", "greenfield"]
                .iter()
                .any(|needle| lowered.contains(needle))
                || (lowered.contains("create")
                    && ["package", "workspace", "app"]
                        .iter()
                        .any(|needle| lowered.contains(needle)));
            requests_node && requests_scaffold
        }
        _ => false,
    }
}

fn timeout_prompt_starter_files(prompt: &str) -> Vec<String> {
    let mut files = Vec::new();
    for token in prompt.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '`' | '"' | '\'' | ',' | ';' | ':' | '(' | ')' | '[' | ']'
            )
    }) {
        let token = token.trim_end_matches('.').trim();
        if token.is_empty() {
            continue;
        }
        if !(token.contains('/')
            || token.ends_with(".toml")
            || token.ends_with(".md")
            || token.ends_with(".json")
            || token.ends_with(".gitignore"))
        {
            continue;
        }
        if token.starts_with("crates/") || token.starts_with("src/") || token.starts_with("tests/")
        {
            continue;
        }
        push_unique_string(&mut files, token.to_string());
    }
    files
}

fn push_unique_string(values: &mut Vec<String>, candidate: String) {
    if !values.iter().any(|existing| existing == &candidate) {
        values.push(candidate);
    }
}

fn contract_to_proposal(feature: &Feature, contract: &Contract) -> DraftProposal {
    DraftProposal {
        title: feature.title.clone(),
        summary: feature.summary.clone(),
        entry_points: contract.entry_points.clone(),
        import_paths: contract.import_paths.clone(),
        expected_interfaces: contract.expected_interfaces.clone(),
        behavior_requirements: contract.behavior_requirements.clone(),
        allowed_scope: contract.allowed_scope.clone(),
        target_checks: contract.target_checks.clone(),
        integrity_checks: contract.integrity_checks.clone(),
        risk_level: contract.risk_level.clone(),
    }
}

fn format_validation_guidance(errors: &[punk_domain::DraftValidationError]) -> String {
    errors
        .iter()
        .map(|error| format!("{}: {}", error.field, error.message))
        .collect::<Vec<_>>()
        .join("; ")
}

fn new_id(prefix: &str) -> String {
    format!("{}_{}", prefix, Utc::now().format("%Y%m%d%H%M%S%3f"))
}

struct RunFinalizer {
    run_path: PathBuf,
    run: Run,
    armed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunHeartbeat {
    run_id: String,
    state: String,
    last_progress_at: String,
    stdout_bytes: u64,
    stderr_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct ExecutorProcessInfo {
    child_pid: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct StaleArtifactCandidate {
    pub artifact_kind: String,
    pub artifact_id: String,
    pub work_id: String,
    pub artifact_ref: String,
    pub reason: String,
    pub last_progress_at: Option<String>,
    pub executor_pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StaleGcReport {
    pub project_id: String,
    pub generated_at: String,
    pub safe_to_archive: Vec<StaleArtifactCandidate>,
    pub manual_review: Vec<StaleArtifactCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StaleArchiveRecord {
    archived_at: String,
    run_id: String,
    work_id: String,
    original_ref: String,
    reason: String,
    last_progress_at: Option<String>,
    executor_pid: Option<u32>,
}

struct RunHeartbeatGuard {
    heartbeat_path: PathBuf,
    run_id: String,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl RunFinalizer {
    fn new(run_path: PathBuf, run: &Run) -> Self {
        Self {
            run_path,
            run: run.clone(),
            armed: true,
        }
    }

    fn sync(&mut self, run: &Run) {
        self.run = run.clone();
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl RunHeartbeatGuard {
    fn new(
        heartbeat_path: PathBuf,
        run_id: String,
        stdout_path: PathBuf,
        stderr_path: PathBuf,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = Some(spawn_run_heartbeat(
            heartbeat_path.clone(),
            run_id.clone(),
            stdout_path.clone(),
            stderr_path.clone(),
            stop.clone(),
        ));
        Self {
            heartbeat_path,
            run_id,
            stdout_path,
            stderr_path,
            stop,
            handle,
        }
    }

    fn finish(&mut self, state: &str) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        let _ = write_run_heartbeat(
            &self.heartbeat_path,
            &self.run_id,
            state,
            &self.stdout_path,
            &self.stderr_path,
        );
    }
}

impl Drop for RunHeartbeatGuard {
    fn drop(&mut self) {
        if self.handle.is_none() {
            return;
        }
        self.finish("failed");
    }
}

impl Drop for RunFinalizer {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if self.run.status == RunStatus::Running {
            self.run.status = RunStatus::Failed;
        }
        if self.run.ended_at.is_none() {
            self.run.ended_at = Some(now_rfc3339());
        }
        let _ = write_json(&self.run_path, &self.run);
    }
}

fn spawn_run_heartbeat(
    heartbeat_path: PathBuf,
    run_id: String,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut last_progress_at = now_rfc3339();
        let mut last_stdout_bytes = file_len(&stdout_path);
        let mut last_stderr_bytes = file_len(&stderr_path);
        while !stop.load(Ordering::Relaxed) {
            let stdout_bytes = file_len(&stdout_path);
            let stderr_bytes = file_len(&stderr_path);
            if stdout_bytes != last_stdout_bytes || stderr_bytes != last_stderr_bytes {
                last_progress_at = now_rfc3339();
                last_stdout_bytes = stdout_bytes;
                last_stderr_bytes = stderr_bytes;
            }
            let _ = write_run_heartbeat_snapshot(
                &heartbeat_path,
                &run_id,
                "running",
                &last_progress_at,
                stdout_bytes,
                stderr_bytes,
            );
            thread::sleep(Duration::from_millis(250));
        }
    })
}

fn write_run_heartbeat(
    heartbeat_path: &Path,
    run_id: &str,
    state: &str,
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<()> {
    write_run_heartbeat_snapshot(
        heartbeat_path,
        run_id,
        state,
        &now_rfc3339(),
        file_len(stdout_path),
        file_len(stderr_path),
    )
}

fn write_run_heartbeat_snapshot(
    heartbeat_path: &Path,
    run_id: &str,
    state: &str,
    last_progress_at: &str,
    stdout_bytes: u64,
    stderr_bytes: u64,
) -> Result<()> {
    let heartbeat = RunHeartbeat {
        run_id: run_id.to_string(),
        state: state.to_string(),
        last_progress_at: last_progress_at.to_string(),
        stdout_bytes,
        stderr_bytes,
    };
    write_json(heartbeat_path, &heartbeat)
}

fn file_len(path: &Path) -> u64 {
    fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use punk_adapters::{ContractDrafter, ExecuteInput, ExecuteOutput};
    use punk_domain::{DraftInput, RefineInput, RunVcs};
    use std::ffi::OsString;

    struct EnvVarGuard {
        key: &'static str,
        original: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let original = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct FakeExecutor;

    impl Executor for FakeExecutor {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn execute_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput> {
            fs::write(input.repo_root.join("demo.txt"), b"ok")?;
            fs::write(&input.stdout_path, b"done")?;
            fs::write(&input.stderr_path, b"")?;
            Ok(ExecuteOutput {
                success: true,
                summary: "done".into(),
                checks_run: vec![],
                cost_usd: None,
                duration_ms: 1,
            })
        }
    }

    struct ProvenanceExecutor;

    impl Executor for ProvenanceExecutor {
        fn name(&self) -> &'static str {
            "provenance"
        }

        fn execute_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput> {
            fs::write(input.repo_root.join("carry.txt"), b"changed during run")?;
            fs::write(input.repo_root.join("demo.txt"), b"ok")?;
            fs::write(&input.stdout_path, b"done")?;
            fs::write(&input.stderr_path, b"")?;
            Ok(ExecuteOutput {
                success: true,
                summary: "done".into(),
                checks_run: vec![],
                cost_usd: None,
                duration_ms: 1,
            })
        }
    }

    struct NoProgressNoOpExecutor;

    impl Executor for NoProgressNoOpExecutor {
        fn name(&self) -> &'static str {
            "no-progress-noop"
        }

        fn execute_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput> {
            fs::write(
                &input.stdout_path,
                b"no implementation progress after bounded context dispatch in src/lib.rs: bounded executor found no additional edits\n",
            )?;
            fs::write(&input.stderr_path, b"")?;
            Ok(ExecuteOutput {
                success: false,
                summary: "no implementation progress after bounded context dispatch in src/lib.rs: bounded executor found no additional edits".into(),
                checks_run: vec![],
                cost_usd: None,
                duration_ms: 1,
            })
        }
    }

    struct BlockedNoOpExecutor;

    impl Executor for BlockedNoOpExecutor {
        fn name(&self) -> &'static str {
            "blocked-noop"
        }

        fn execute_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput> {
            fs::write(
                &input.stdout_path,
                b"PUNK_EXECUTION_BLOCKED: bounded executor found no additional edits\n",
            )?;
            fs::write(&input.stderr_path, b"")?;
            Ok(ExecuteOutput {
                success: false,
                summary: "PUNK_EXECUTION_BLOCKED: bounded executor found no additional edits"
                    .into(),
                checks_run: vec![],
                cost_usd: None,
                duration_ms: 1,
            })
        }
    }

    struct SuccessNoOpExecutor;

    impl Executor for SuccessNoOpExecutor {
        fn name(&self) -> &'static str {
            "success-noop"
        }

        fn execute_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput> {
            fs::write(
                &input.stdout_path,
                b"PUNK_EXECUTION_COMPLETE: claimed success without edits\n",
            )?;
            fs::write(&input.stderr_path, b"")?;
            Ok(ExecuteOutput {
                success: true,
                summary: "PUNK_EXECUTION_COMPLETE: claimed success without edits".into(),
                checks_run: vec![],
                cost_usd: None,
                duration_ms: 1,
            })
        }
    }

    struct AlreadySatisfiedDrafter;

    impl ContractDrafter for AlreadySatisfiedDrafter {
        fn name(&self) -> &'static str {
            "already-satisfied-drafter"
        }

        fn draft(&self, input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "already satisfied".into(),
                summary: input.prompt,
                entry_points: vec!["src/lib.rs".into()],
                import_paths: vec!["src/lib.rs".into()],
                expected_interfaces: vec!["demo".into()],
                behavior_requirements: vec!["keep demo implemented".into()],
                allowed_scope: vec!["src/lib.rs".into()],
                target_checks: vec!["cargo test".into()],
                integrity_checks: vec!["cargo test".into()],
                risk_level: "low".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct DirectoryScopedAlreadySatisfiedDrafter;

    impl ContractDrafter for DirectoryScopedAlreadySatisfiedDrafter {
        fn name(&self) -> &'static str {
            "directory-scoped-already-satisfied-drafter"
        }

        fn draft(&self, input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "directory scoped already satisfied".into(),
                summary: input.prompt,
                entry_points: vec![
                    "crates/pubpunk-cli/Cargo.toml".into(),
                    "crates/pubpunk-core/Cargo.toml".into(),
                ],
                import_paths: vec![
                    "crates/pubpunk-cli".into(),
                    "crates/pubpunk-core".into(),
                    "tests".into(),
                ],
                expected_interfaces: vec!["bounded implementation slice".into()],
                behavior_requirements: vec!["implement init logic".into()],
                allowed_scope: vec![
                    "crates/pubpunk-cli".into(),
                    "crates/pubpunk-core".into(),
                    "tests".into(),
                ],
                target_checks: vec!["cargo test -p pubpunk-cli".into()],
                integrity_checks: vec!["cargo test --workspace".into()],
                risk_level: "medium".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct PanicExecutor;

    impl Executor for PanicExecutor {
        fn name(&self) -> &'static str {
            "panic"
        }

        fn execute_contract(&self, _input: ExecuteInput) -> Result<ExecuteOutput> {
            panic!("executor interrupted");
        }
    }

    struct CargoLockExecutor;

    impl Executor for CargoLockExecutor {
        fn name(&self) -> &'static str {
            "cargo-lock"
        }

        fn execute_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput> {
            fs::write(input.repo_root.join("Cargo.lock"), b"# generated\n")?;
            fs::write(&input.stdout_path, b"done")?;
            fs::write(&input.stderr_path, b"")?;
            Ok(ExecuteOutput {
                success: true,
                summary: "done".into(),
                checks_run: vec!["cargo test --workspace".into()],
                cost_usd: None,
                duration_ms: 1,
            })
        }
    }

    struct CargoLockDrafter;

    impl ContractDrafter for CargoLockDrafter {
        fn name(&self) -> &'static str {
            "cargo-lock-drafter"
        }

        fn draft(&self, input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "cargo lock".into(),
                summary: input.prompt,
                entry_points: vec!["Cargo.toml".into()],
                import_paths: vec![],
                expected_interfaces: vec!["demo".into()],
                behavior_requirements: vec!["keep bootstrap bounded".into()],
                allowed_scope: vec!["Cargo.toml".into()],
                target_checks: vec!["cargo test --workspace".into()],
                integrity_checks: vec!["cargo test --workspace".into()],
                risk_level: "low".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct HeartbeatObservingExecutor;

    impl Executor for HeartbeatObservingExecutor {
        fn name(&self) -> &'static str {
            "heartbeat-observer"
        }

        fn execute_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput> {
            let heartbeat_path = input.stdout_path.parent().unwrap().join("heartbeat.json");
            thread::sleep(Duration::from_millis(350));
            let first: serde_json::Value = read_json(&heartbeat_path)?;
            let first_progress = first["last_progress_at"].as_str().unwrap().to_string();
            assert_eq!(first["state"].as_str(), Some("running"));

            thread::sleep(Duration::from_millis(350));
            let second: serde_json::Value = read_json(&heartbeat_path)?;
            let second_progress = second["last_progress_at"].as_str().unwrap().to_string();
            assert_eq!(second["state"].as_str(), Some("running"));
            assert_eq!(first_progress, second_progress);

            fs::write(&input.stdout_path, b"progress")?;
            thread::sleep(Duration::from_millis(350));
            let third: serde_json::Value = read_json(&heartbeat_path)?;
            let third_progress = third["last_progress_at"].as_str().unwrap().to_string();
            assert_eq!(third["state"].as_str(), Some("running"));
            assert_ne!(second_progress, third_progress);
            assert_eq!(third["stdout_bytes"].as_u64(), Some(8));

            fs::write(&input.stdout_path, b"done")?;
            fs::write(&input.stderr_path, b"")?;
            Ok(ExecuteOutput {
                success: true,
                summary: "done".into(),
                checks_run: vec![],
                cost_usd: None,
                duration_ms: 1,
            })
        }
    }

    struct FakeDrafter;

    impl ContractDrafter for FakeDrafter {
        fn name(&self) -> &'static str {
            "fake-drafter"
        }

        fn draft(&self, input: DraftInput) -> Result<DraftProposal> {
            let scope = if input.scan.candidate_file_scope_paths.is_empty() {
                vec!["demo.txt".into()]
            } else {
                vec![input.scan.candidate_file_scope_paths[0].clone()]
            };
            let check = input
                .scan
                .candidate_integrity_checks
                .first()
                .cloned()
                .unwrap_or_else(|| "true".into());
            Ok(DraftProposal {
                title: "demo contract".into(),
                summary: input.prompt,
                entry_points: input.scan.candidate_entry_points,
                import_paths: vec![],
                expected_interfaces: vec!["demo interface".into()],
                behavior_requirements: vec!["implement the requested behavior".into()],
                allowed_scope: scope,
                target_checks: vec![check.clone()],
                integrity_checks: vec![check],
                risk_level: "medium".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            let mut current = input.current;
            if current.allowed_scope.is_empty() {
                current.allowed_scope = vec!["demo.txt".into()];
            }
            if current.target_checks.is_empty() {
                current.target_checks = vec!["true".into()];
            }
            if current.integrity_checks.is_empty() {
                current.integrity_checks = vec!["true".into()];
            }
            if current.entry_points.is_empty() {
                current.entry_points = vec!["demo.txt".into()];
            }
            if current.behavior_requirements.is_empty() {
                current.behavior_requirements = vec!["do the thing".into()];
            }
            Ok(current)
        }
    }

    struct InvalidThenRepairDrafter;

    impl ContractDrafter for InvalidThenRepairDrafter {
        fn name(&self) -> &'static str {
            "invalid-then-repair"
        }

        fn draft(&self, _input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "broken".into(),
                summary: "broken".into(),
                entry_points: vec![],
                import_paths: vec![],
                expected_interfaces: vec!["x".into()],
                behavior_requirements: vec![],
                allowed_scope: vec![],
                target_checks: vec![],
                integrity_checks: vec![],
                risk_level: "medium".into(),
            })
        }

        fn refine(&self, _input: RefineInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "fixed".into(),
                summary: "fixed".into(),
                entry_points: vec!["demo.txt".into()],
                import_paths: vec![],
                expected_interfaces: vec!["x".into()],
                behavior_requirements: vec!["fix it".into()],
                allowed_scope: vec!["demo.txt".into()],
                target_checks: vec!["true".into()],
                integrity_checks: vec!["true".into()],
                risk_level: "medium".into(),
            })
        }
    }

    struct EntryPointScopeLeakDrafter;

    impl ContractDrafter for EntryPointScopeLeakDrafter {
        fn name(&self) -> &'static str {
            "entry-point-scope-leak"
        }

        fn draft(&self, _input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "pubpunk init".into(),
                summary: "scope leak".into(),
                entry_points: vec!["src/lib.rs".into()],
                import_paths: vec![],
                expected_interfaces: vec!["library init surface".into()],
                behavior_requirements: vec!["implement init".into()],
                allowed_scope: vec!["Cargo.toml".into()],
                target_checks: vec!["cargo test".into()],
                integrity_checks: vec!["cargo test".into()],
                risk_level: "medium".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct ExplicitDetailsIgnoringDrafter;

    impl ContractDrafter for ExplicitDetailsIgnoringDrafter {
        fn name(&self) -> &'static str {
            "ignores-explicit-details"
        }

        fn draft(&self, _input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "scaffold council".into(),
                summary: "scaffold council".into(),
                entry_points: vec!["crates/punk-core/src/lib.rs".into()],
                import_paths: vec![],
                expected_interfaces: vec!["crate scaffold".into()],
                behavior_requirements: vec!["create council crate".into()],
                allowed_scope: vec!["crates/punk-core".into()],
                target_checks: vec!["cargo test".into()],
                integrity_checks: vec!["cargo test".into()],
                risk_level: "low".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct ScanTargetChecksDrafter;

    impl ContractDrafter for ScanTargetChecksDrafter {
        fn name(&self) -> &'static str {
            "scan-target-checks"
        }

        fn draft(&self, input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "scan target checks".into(),
                summary: input.prompt,
                entry_points: vec!["crates/punk-orch/src/lib.rs".into()],
                import_paths: vec![],
                expected_interfaces: vec!["keep target checks bounded".into()],
                behavior_requirements: vec!["use scan candidate target checks".into()],
                allowed_scope: vec!["crates/punk-orch/src/lib.rs".into()],
                target_checks: input.scan.candidate_target_checks,
                integrity_checks: input.scan.candidate_integrity_checks,
                risk_level: "low".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct GreenfieldRustDrafter;

    impl ContractDrafter for GreenfieldRustDrafter {
        fn name(&self) -> &'static str {
            "greenfield-rust-drafter"
        }

        fn draft(&self, input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "greenfield rust intake".into(),
                summary: input.prompt,
                entry_points: vec!["Cargo.toml".into(), "src/lib.rs".into()],
                import_paths: vec![],
                expected_interfaces: vec!["initial Rust scaffold".into()],
                behavior_requirements: vec!["allow first Rust goal after init".into()],
                allowed_scope: vec!["Cargo.toml".into(), "src/lib.rs".into()],
                target_checks: input.scan.candidate_target_checks,
                integrity_checks: input.scan.candidate_integrity_checks,
                risk_level: "low".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct GreenfieldGoDrafter;

    impl ContractDrafter for GreenfieldGoDrafter {
        fn name(&self) -> &'static str {
            "greenfield-go-drafter"
        }

        fn draft(&self, input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "greenfield go intake".into(),
                summary: input.prompt,
                entry_points: vec!["go.mod".into(), "cmd/pubpunk/main.go".into()],
                import_paths: vec![],
                expected_interfaces: vec!["initial Go scaffold".into()],
                behavior_requirements: vec!["allow first Go goal after init".into()],
                allowed_scope: vec!["go.mod".into(), "cmd".into(), "internal".into()],
                target_checks: input.scan.candidate_target_checks,
                integrity_checks: input.scan.candidate_integrity_checks,
                risk_level: "low".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct GreenfieldPythonDrafter;

    impl ContractDrafter for GreenfieldPythonDrafter {
        fn name(&self) -> &'static str {
            "greenfield-python-drafter"
        }

        fn draft(&self, input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "greenfield python intake".into(),
                summary: input.prompt,
                entry_points: vec!["pyproject.toml".into(), "src/pubpunk/__init__.py".into()],
                import_paths: vec![],
                expected_interfaces: vec!["initial Python scaffold".into()],
                behavior_requirements: vec!["allow first Python goal after init".into()],
                allowed_scope: vec!["pyproject.toml".into(), "src".into(), "tests".into()],
                target_checks: input.scan.candidate_target_checks,
                integrity_checks: input.scan.candidate_integrity_checks,
                risk_level: "low".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct GreenfieldNodeDrafter;

    impl ContractDrafter for GreenfieldNodeDrafter {
        fn name(&self) -> &'static str {
            "greenfield-node-drafter"
        }

        fn draft(&self, input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "greenfield node intake".into(),
                summary: input.prompt,
                entry_points: vec!["package.json".into(), "src/index.ts".into()],
                import_paths: vec![],
                expected_interfaces: vec!["initial TypeScript/Node scaffold".into()],
                behavior_requirements: vec!["allow first TypeScript/Node goal after init".into()],
                allowed_scope: vec![
                    "package.json".into(),
                    "tsconfig.json".into(),
                    "src".into(),
                    "tests".into(),
                ],
                target_checks: input.scan.candidate_target_checks,
                integrity_checks: input.scan.candidate_integrity_checks,
                risk_level: "low".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct BroadBootstrapDrafter;

    impl ContractDrafter for BroadBootstrapDrafter {
        fn name(&self) -> &'static str {
            "broad-bootstrap"
        }

        fn draft(&self, _input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "bootstrap".into(),
                summary: "bootstrap".into(),
                entry_points: vec!["Cargo.toml".into()],
                import_paths: vec![],
                expected_interfaces: vec!["initial Rust scaffold".into()],
                behavior_requirements: vec!["scaffold rust workspace".into()],
                allowed_scope: vec!["Cargo.toml".into(), "crates".into(), "tests".into()],
                target_checks: vec!["cargo test --workspace".into()],
                integrity_checks: vec!["cargo test --workspace".into()],
                risk_level: "medium".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct PlaceholderScopeDrafter;

    impl ContractDrafter for PlaceholderScopeDrafter {
        fn name(&self) -> &'static str {
            "placeholder-scope"
        }

        fn draft(&self, _input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "proposal phase".into(),
                summary: "proposal phase".into(),
                entry_points: vec![
                    "crates/punk-council/src/lib.rs".into(),
                    "crates/punk-council/src/proposal.rs".into(),
                ],
                import_paths: vec![],
                expected_interfaces: vec!["proposal result".into()],
                behavior_requirements: vec!["persist proposal artifacts".into()],
                allowed_scope: vec!["punk/council/<id>/proposals/".into()],
                target_checks: vec!["cargo test".into()],
                integrity_checks: vec!["cargo test".into()],
                risk_level: "medium".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct StructurallyInvalidSynthesisDrafter;

    impl ContractDrafter for StructurallyInvalidSynthesisDrafter {
        fn name(&self) -> &'static str {
            "structurally-invalid-synthesis"
        }

        fn draft(&self, _input: DraftInput) -> Result<DraftProposal> {
            Ok(DraftProposal {
                title: "synthesis phase".into(),
                summary: "synthesis phase".into(),
                entry_points: vec!["synthesis.json".into(), "record.json".into()],
                import_paths: vec![
                    "crates/punk-council/src/packet.rs".into(),
                    "crates/punk-domain/src/council.rs".into(),
                ],
                expected_interfaces: vec!["final record".into()],
                behavior_requirements: vec!["write final record.json".into()],
                allowed_scope: vec!["synthesis.json".into(), "record.json".into()],
                target_checks: vec![
                    "cargo test -p punk-council".into(),
                    "cargo test -p punk-domain".into(),
                    "cargo test -p punk-core".into(),
                ],
                integrity_checks: vec!["cargo test --workspace".into()],
                risk_level: "medium".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct FailingDrafter;

    impl ContractDrafter for FailingDrafter {
        fn name(&self) -> &'static str {
            "failing-drafter"
        }

        fn draft(&self, _input: DraftInput) -> Result<DraftProposal> {
            Err(anyhow!("simulated drafter failure"))
        }

        fn refine(&self, _input: RefineInput) -> Result<DraftProposal> {
            Err(anyhow!("simulated refine failure"))
        }
    }

    struct TimeoutDrafter;

    impl ContractDrafter for TimeoutDrafter {
        fn name(&self) -> &'static str {
            "timeout-drafter"
        }

        fn draft(&self, _input: DraftInput) -> Result<DraftProposal> {
            Err(anyhow!("codex command timed out after 30s: 58,249"))
        }

        fn refine(&self, _input: RefineInput) -> Result<DraftProposal> {
            Err(anyhow!("codex command timed out after 30s: 58,249"))
        }
    }

    #[test]
    fn draft_and_approve_contract() {
        let root = std::env::temp_dir().join(format!("punk-orch-{}", std::process::id()));
        let global = std::env::temp_dir().join(format!(
            "punk-orch-status-vcs-global-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&global);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", ".gitignore"])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();
        let service = OrchService::new(&root, &global).unwrap();
        let contract = service.draft_contract(&FakeDrafter, "add file").unwrap();
        let approved = service.approve_contract(&contract.id).unwrap();
        assert_eq!(approved.status, ContractStatus::Approved);
        let (_run, receipt) = service.cut_run(&FakeExecutor, &contract.id).unwrap();
        assert_eq!(receipt.status, "success");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cut_run_receipt_tracks_only_current_run_changes() {
        let root =
            std::env::temp_dir().join(format!("punk-orch-provenance-{}", std::process::id()));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        fs::write(root.join("ambient.txt"), "clean\n").unwrap();
        fs::write(root.join("carry.txt"), "clean\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join("ambient.txt"), "dirty before run\n").unwrap();
        fs::write(root.join("carry.txt"), "dirty before run\n").unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service.draft_contract(&FakeDrafter, "add file").unwrap();
        service.approve_contract(&contract.id).unwrap();

        let (_run, receipt) = service.cut_run(&ProvenanceExecutor, &contract.id).unwrap();
        let mut changed_files = receipt.changed_files;
        changed_files.sort();
        assert_eq!(
            changed_files,
            vec!["carry.txt".to_string(), "demo.txt".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cut_run_succeeds_when_bounded_diff_is_already_satisfied_before_dispatch() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-already-satisfied-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn demo() { println!(\"done\"); }\n",
        )
        .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(&AlreadySatisfiedDrafter, "demo already satisfied")
            .unwrap();
        service.approve_contract(&contract.id).unwrap();

        let (run, receipt) = service
            .cut_run(&NoProgressNoOpExecutor, &contract.id)
            .unwrap();
        assert_eq!(run.status, RunStatus::Finished);
        assert_eq!(receipt.status, "success");
        assert!(receipt.changed_files.is_empty());
        assert!(receipt
            .summary
            .contains("already satisfied in allowed scope before bounded dispatch"));
        assert!(receipt.summary.contains("src/lib.rs"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cut_run_does_not_upgrade_blocked_file_slice_to_already_satisfied() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-blocked-not-already-satisfied-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn demo() { println!(\"done\"); }\n",
        )
        .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(&AlreadySatisfiedDrafter, "demo cleanup slice")
            .unwrap();
        service.approve_contract(&contract.id).unwrap();

        let (run, receipt) = service.cut_run(&BlockedNoOpExecutor, &contract.id).unwrap();
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(receipt.status, "failure");
        assert!(receipt.changed_files.is_empty());
        assert!(receipt.summary.starts_with("PUNK_EXECUTION_BLOCKED:"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cut_run_does_not_upgrade_directory_scoped_no_progress_to_already_satisfied() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-directory-already-satisfied-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/pubpunk-cli/src")).unwrap();
        fs::create_dir_all(root.join("crates/pubpunk-core/src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers=['crates/pubpunk-cli','crates/pubpunk-core']\nresolver='2'\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/Cargo.toml"),
            "[package]\nname='pubpunk-cli'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/Cargo.toml"),
            "[package]\nname='pubpunk-core'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            "pub fn init() {}\n",
        )
        .unwrap();
        fs::write(root.join("tests/README.md"), "tests\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget/\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/Cargo.toml"),
            "[package]\nname='pubpunk-cli'\nversion='0.2.0'\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/Cargo.toml"),
            "[package]\nname='pubpunk-core'\nversion='0.2.0'\n",
        )
        .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &DirectoryScopedAlreadySatisfiedDrafter,
                "implement pubpunk init in bounded dirs",
            )
            .unwrap();
        service.approve_contract(&contract.id).unwrap();

        let (run, receipt) = service.cut_run(&BlockedNoOpExecutor, &contract.id).unwrap();
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(receipt.status, "failure");
        assert!(receipt
            .summary
            .starts_with("PUNK_EXECUTION_BLOCKED: bounded executor found no additional edits"));
        assert!(!receipt
            .summary
            .contains("already satisfied in allowed scope before bounded dispatch"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cut_run_rejects_successful_noop_for_bounded_source_slice() {
        let root =
            std::env::temp_dir().join(format!("punk-orch-empty-success-{}", std::process::id()));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget/\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(&AlreadySatisfiedDrafter, "demo implementation slice")
            .unwrap();
        service.approve_contract(&contract.id).unwrap();

        let (run, receipt) = service.cut_run(&SuccessNoOpExecutor, &contract.id).unwrap();
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(receipt.status, "failure");
        assert!(receipt.changed_files.is_empty());
        assert!(receipt
            .summary
            .contains("no implementation progress after bounded success report"));
        assert!(receipt
            .summary
            .contains("PUNK_EXECUTION_COMPLETE: claimed success without edits"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cut_run_prunes_generated_cargo_lock_when_out_of_scope() {
        let root =
            std::env::temp_dir().join(format!("punk-orch-cargo-lock-prune-{}", std::process::id()));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers=[]\nresolver='2'\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(&CargoLockDrafter, "bounded rust bootstrap")
            .unwrap();
        service.approve_contract(&contract.id).unwrap();

        let (_run, receipt) = service.cut_run(&CargoLockExecutor, &contract.id).unwrap();
        assert!(!receipt
            .changed_files
            .iter()
            .any(|path| path == "Cargo.lock"));
        assert!(!root.join("Cargo.lock").exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn merge_default_gitignore_entries_adds_runtime_artifact_ignores_when_missing() {
        let merged = merge_default_gitignore_entries("");
        assert_eq!(merged, ".punk/\ntarget/\n.playwright-mcp/\n");

        let already_covered = merge_default_gitignore_entries("target\n.punk\n.playwright-mcp\n");
        assert_eq!(already_covered, "target\n.punk\n.playwright-mcp\n");
    }

    #[test]
    fn draft_contract_accepts_nested_package_checks_when_root_has_no_integrity_story() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-orch-nested-integrity-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("baseline-site")).unwrap();
        fs::write(
            root.join("baseline-site/package.json"),
            r#"{
  "name": "baseline-site",
  "scripts": {
    "check": "echo check",
    "test": "echo test"
  }
}
"#,
        )
        .unwrap();
        fs::write(root.join(".gitignore"), ".playwright-mcp/\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(&FakeDrafter, "tighten the baseline-site homepage")
            .unwrap();

        assert_eq!(
            contract.target_checks,
            vec!["npm --prefix baseline-site run check".to_string()]
        );
        assert_eq!(
            contract.integrity_checks,
            vec!["npm --prefix baseline-site run check".to_string()]
        );
        assert!(contract
            .allowed_scope
            .iter()
            .any(|path| path == "baseline-site/package.json"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_prefers_backend_candidates_over_ui_for_nested_repo() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-orch-nested-backend-bias-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("baseline-site/src/components")).unwrap();
        fs::create_dir_all(root.join("baseline-site/src/db")).unwrap();
        fs::create_dir_all(root.join("baseline-site/src/lib/persistence")).unwrap();
        fs::create_dir_all(root.join("baseline-site/src/actions")).unwrap();
        fs::create_dir_all(root.join("baseline-site/.astro/integrations/astro_db")).unwrap();
        fs::create_dir_all(root.join("baseline-site/dist")).unwrap();
        fs::create_dir_all(root.join("packs")).unwrap();
        fs::write(
            root.join("baseline-site/package.json"),
            r#"{
  "name": "baseline-site",
  "scripts": {
    "check": "echo check",
    "test": "echo test"
  }
}
"#,
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/components/SiteHeader.astro"),
            "---\n---\n<header />\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/components/SiteFooter.astro"),
            "---\n---\n<footer />\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/db/schema.ts"),
            "export const schema = {}\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/lib/persistence/store.ts"),
            "export const store = {}\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/actions/create-session.ts"),
            "export async function createSession() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/drizzle.config.ts"),
            "export default {};\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/astro.config.mjs"),
            "export default {};\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/.astro/integrations/astro_db/db.d.ts"),
            "export {};\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/dist/app.js"),
            "console.log('dist')\n",
        )
        .unwrap();
        fs::write(root.join("packs/generated.json"), "{}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &FakeDrafter,
                "implement DB session seed services for baseline dispatch",
            )
            .unwrap();

        let first_allowed = contract.allowed_scope.first().map(String::as_str);
        let first_entry = contract.entry_points.first().map(String::as_str);
        let preferred_backend_paths = [
            "baseline-site/src/db/schema.ts",
            "baseline-site/src/lib/persistence/store.ts",
            "baseline-site/src/actions/create-session.ts",
            "baseline-site/drizzle.config.ts",
            "baseline-site/package.json",
        ];
        assert!(first_allowed.is_some_and(|path| preferred_backend_paths.contains(&path)));
        assert!(first_entry.is_some_and(|path| preferred_backend_paths.contains(&path)));
        assert!(!contract
            .entry_points
            .iter()
            .any(|path| path.ends_with(".astro")));
        assert!(!contract
            .entry_points
            .iter()
            .any(|path| path.contains("/dist/") || path.starts_with("packs/")));
        assert!(!contract
            .entry_points
            .iter()
            .any(|path| path.contains("/.astro/") || path.contains("astro.config")));
        assert!(contract
            .allowed_scope
            .iter()
            .any(|path| path == "baseline-site/package.json"
                || path == "baseline-site/drizzle.config.ts"
                || path == "baseline-site/src/db/schema.ts"
                || path == "baseline-site/src/lib/persistence/store.ts"
                || path == "baseline-site/src/actions/create-session.ts"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_preserves_mixed_node_rust_service_scope() {
        struct NarrowMixedServiceDrafter;

        impl ContractDrafter for NarrowMixedServiceDrafter {
            fn name(&self) -> &'static str {
                "narrow-mixed-service"
            }

            fn draft(&self, input: DraftInput) -> Result<DraftProposal> {
                Ok(DraftProposal {
                    title: "session service".into(),
                    summary: input.prompt,
                    entry_points: vec!["baseline-site/src/lib/persistence/leads.ts".into()],
                    import_paths: vec![],
                    expected_interfaces: vec!["session handoff service".into()],
                    behavior_requirements: vec!["implement session dispatch bridge".into()],
                    allowed_scope: vec!["baseline-site/src/lib/persistence/leads.ts".into()],
                    target_checks: vec!["npm --prefix baseline-site test".into()],
                    integrity_checks: vec!["npm --prefix baseline-site test".into()],
                    risk_level: "medium".into(),
                })
            }

            fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
                Ok(input.current)
            }
        }

        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-orch-mixed-node-rust-service-{suffix}-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("baseline-site/src/lib/db")).unwrap();
        fs::create_dir_all(root.join("baseline-site/src/lib/persistence")).unwrap();
        fs::create_dir_all(root.join("baseline-site/src/actions")).unwrap();
        fs::create_dir_all(root.join("crates/session-bridge/src")).unwrap();
        fs::write(
            root.join("baseline-site/package.json"),
            r#"{
  "name": "baseline-site",
  "scripts": {
    "test": "echo test"
  }
}
"#,
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/lib/db/probe-state.ts"),
            "export const probeState = {};\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/lib/persistence/leads.ts"),
            "export const leads = {};\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/actions/report.ts"),
            "export async function report() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/session-bridge\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/session-bridge/Cargo.toml"),
            "[package]\nname = \"session-bridge\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/session-bridge/src/lib.rs"),
            "pub fn handoff() {}\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &NarrowMixedServiceDrafter,
                "implement session dispatch service bridge between baseline-site and Rust crates with transactional report handoff",
            )
            .unwrap();

        assert!(contract
            .allowed_scope
            .iter()
            .any(|path| path == "baseline-site/package.json"));
        assert!(contract
            .allowed_scope
            .iter()
            .any(|path| path == "Cargo.toml" || path == "crates"));
        assert!(contract.allowed_scope.iter().any(|path| {
            path == "baseline-site/src/lib/db"
                || path == "baseline-site/src/lib/persistence"
                || path == "baseline-site/src/actions"
                || path == "baseline-site/src/lib/db/probe-state.ts"
                || path == "baseline-site/src/actions/report.ts"
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_prefers_mixed_service_backend_anchors_over_ui_pages() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-orch-mixed-service-anchors-{suffix}-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("baseline-site/src/components")).unwrap();
        fs::create_dir_all(root.join("baseline-site/src/pages/api/cli")).unwrap();
        fs::create_dir_all(root.join("baseline-site/src/pages")).unwrap();
        fs::create_dir_all(root.join("baseline-site/src/lib/services")).unwrap();
        fs::create_dir_all(root.join("baseline-site/src/lib/session")).unwrap();
        fs::create_dir_all(root.join("crates/baseline-cli/src")).unwrap();
        fs::write(
            root.join("baseline-site/package.json"),
            r#"{
  "name": "baseline-site",
  "scripts": {
    "check": "echo check",
    "build:web": "echo build"
  }
}
"#,
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/baseline-cli/Cargo.toml"),
            "[package]\nname = \"baseline-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/baseline-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/pages/dispatch.astro"),
            "---\n---\n<div>dispatch</div>\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/components/SiteHeader.astro"),
            "---\n---\n<header />\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/components/SiteFooter.astro"),
            "---\n---\n<footer />\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/pages/api/cli/enroll.ts"),
            "export async function post() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/lib/services/enrollments.ts"),
            "export async function enroll() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/lib/services/dispatch.ts"),
            "export async function dispatch() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/lib/session/operator.ts"),
            "export function operatorSession() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/lib/session/cookies.ts"),
            "export function readCookies() {}\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let prompt =
            "mixed service/session/runtime handshake around enrollment activation across Astro server layer and Rust CLI layer";
        let mut scan = scan_repo(&root, prompt).unwrap();
        apply_prompt_targeting_bias(&root, prompt, &mut scan);

        assert!(scan
            .candidate_entry_points
            .iter()
            .any(|path| path == "baseline-site/src/pages/api/cli/enroll.ts"));
        assert!(scan
            .candidate_entry_points
            .iter()
            .any(|path| path == "baseline-site/src/lib/services/enrollments.ts"));
        assert!(scan
            .candidate_entry_points
            .iter()
            .any(|path| path == "baseline-site/src/lib/session/operator.ts"));
        assert!(scan
            .candidate_entry_points
            .iter()
            .any(|path| path == "crates/baseline-cli/src/main.rs"));
        assert!(scan
            .candidate_entry_points
            .iter()
            .any(|path| path == "crates/baseline-cli/Cargo.toml"));
        assert!(scan.candidate_file_scope_paths.iter().any(|path| {
            path == "baseline-site/src/lib/services/enrollments.ts"
                || path == "baseline-site/src/lib/services/dispatch.ts"
        }));
        assert!(scan.candidate_file_scope_paths.iter().any(|path| {
            path == "baseline-site/src/lib/session/operator.ts"
                || path == "baseline-site/src/lib/session/cookies.ts"
        }));
        assert!(scan
            .candidate_file_scope_paths
            .iter()
            .any(|path| path == "baseline-site/src/pages/api/cli/enroll.ts"));
        assert!(scan
            .candidate_directory_scope_paths
            .iter()
            .any(|path| path == "baseline-site/src/lib/services"));
        assert!(scan
            .candidate_directory_scope_paths
            .iter()
            .any(|path| path == "baseline-site/src/lib/session"));
        assert!(scan
            .candidate_directory_scope_paths
            .iter()
            .any(|path| path == "baseline-site/src/pages/api"));
        assert!(scan
            .candidate_directory_scope_paths
            .iter()
            .any(|path| path == "crates/baseline-cli"));
        assert!(!scan
            .candidate_entry_points
            .iter()
            .any(|path| path.ends_with("dispatch.astro")
                || path.ends_with("SiteHeader.astro")
                || path.ends_with("SiteFooter.astro")));

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service.draft_contract(&FakeDrafter, prompt).unwrap();
        assert!(contract.allowed_scope.iter().any(|path| {
            path == "baseline-site/src/lib/services"
                || path == "baseline-site/src/lib/services/enrollments.ts"
        }));
        assert!(contract
            .allowed_scope
            .iter()
            .any(|path| path == "Cargo.toml" || path == "crates/baseline-cli"));
        assert!(!contract
            .allowed_scope
            .iter()
            .any(|path| path.ends_with("dispatch.astro")
                || path.ends_with("SiteHeader.astro")
                || path.ends_with("SiteFooter.astro")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_prefers_mixed_service_checks_for_nested_node_rust_repo() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-orch-mixed-service-checks-{suffix}-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("baseline-site/src/pages/api/cli")).unwrap();
        fs::create_dir_all(root.join("baseline-site/src/lib/services")).unwrap();
        fs::create_dir_all(root.join("baseline-site/src/lib/session")).unwrap();
        fs::create_dir_all(root.join("crates/baseline-cli/src")).unwrap();
        fs::write(
            root.join("baseline-site/package.json"),
            r#"{
  "name": "baseline-site",
  "scripts": {
    "check": "echo check",
    "build:web": "echo build"
  }
}
"#,
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/baseline-cli/Cargo.toml"),
            "[package]\nname = \"baseline-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/baseline-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/pages/api/cli/enroll.ts"),
            "export async function post() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/lib/services/enrollments.ts"),
            "export async function enroll() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/lib/session/operator.ts"),
            "export function operatorSession() {}\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &ScanTargetChecksDrafter,
                "mixed service/session/runtime handshake around enrollment activation across Astro server layer and Rust CLI layer",
            )
            .unwrap();

        assert_eq!(
            contract.target_checks,
            vec![
                "cargo check -p baseline-cli".to_string(),
                "npm --prefix baseline-site run build:web".to_string()
            ]
        );
        assert_eq!(
            contract.integrity_checks,
            vec!["npm --prefix baseline-site run check".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_prunes_prompt_excluded_generated_scope_paths() {
        struct PollutedScopeDrafter;

        impl ContractDrafter for PollutedScopeDrafter {
            fn name(&self) -> &'static str {
                "polluted-scope"
            }

            fn draft(&self, input: DraftInput) -> Result<DraftProposal> {
                Ok(DraftProposal {
                    title: "db slice".into(),
                    summary: input.prompt,
                    entry_points: vec![
                        "baseline-site/package.json".into(),
                        "baseline-site/db/config.ts".into(),
                        "baseline-site/db/seed.ts".into(),
                        "baseline-site/src/lib/persistence/leads.ts".into(),
                    ],
                    import_paths: vec![],
                    expected_interfaces: vec!["db layer".into()],
                    behavior_requirements: vec!["implement db slice".into()],
                    allowed_scope: vec![
                        "baseline-site/package.json".into(),
                        "baseline-site/db/config.ts".into(),
                        "baseline-site/db/seed.ts".into(),
                        "baseline-site/src/lib/persistence/leads.ts".into(),
                        "baseline-site/design/stitch/mock.png".into(),
                        "baseline-site/dist/server/chunk.mjs".into(),
                        "baseline-site/.astro/integrations/astro_db/db.d.ts".into(),
                        "baseline-site/.playwright-mcp/state.json".into(),
                    ],
                    target_checks: vec!["npm --prefix baseline-site test".into()],
                    integrity_checks: vec!["npm --prefix baseline-site test".into()],
                    risk_level: "medium".into(),
                })
            }

            fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
                Ok(input.current)
            }
        }

        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-orch-exclusion-prune-{suffix}-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("baseline-site/db")).unwrap();
        fs::create_dir_all(root.join("baseline-site/src/lib/persistence")).unwrap();
        fs::write(
            root.join("baseline-site/package.json"),
            r#"{"name":"baseline-site","scripts":{"test":"echo test"}}"#,
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/db/config.ts"),
            "export default {};\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/db/seed.ts"),
            "export async function seed() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("baseline-site/src/lib/persistence/leads.ts"),
            "export const leads = {};\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &PollutedScopeDrafter,
                "implement DB layer for baseline-site; exclude baseline-site/design/** baseline-site/dist/** baseline-site/.astro/** baseline-site/.playwright-mcp/**",
            )
            .unwrap();

        assert!(contract
            .allowed_scope
            .iter()
            .all(|path| !path.contains("/design/")));
        assert!(contract
            .allowed_scope
            .iter()
            .all(|path| !path.contains("/dist/")));
        assert!(contract
            .allowed_scope
            .iter()
            .all(|path| !path.contains("/.astro/")));
        assert!(contract
            .allowed_scope
            .iter()
            .all(|path| !path.contains("/.playwright-mcp/")));
        assert!(
            contract
                .allowed_scope
                .iter()
                .any(|path| path == "baseline-site/db/config.ts"),
            "{:?}",
            contract.allowed_scope
        );
        assert!(!contract.allowed_scope.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cut_run_success_ensures_default_gitignore_coverage_without_receipt_noise() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-success-gitignore-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
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
                "--allow-empty",
                "-m",
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service.draft_contract(&FakeDrafter, "add file").unwrap();
        service.approve_contract(&contract.id).unwrap();

        let (_run, receipt) = service.cut_run(&FakeExecutor, &contract.id).unwrap();
        assert_eq!(receipt.status, "success");
        assert!(!receipt
            .changed_files
            .iter()
            .any(|path| path == ".gitignore"));
        assert_eq!(
            fs::read_to_string(root.join(".gitignore")).unwrap(),
            ".punk/\ntarget/\n.playwright-mcp/\n"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_generated_cargo_lock_for_file_scoped_cargo_contract() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-orch-file-scope-cargo-lock-prune-{}-{suffix}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Cargo.lock"), "# generated\n").unwrap();
        let contract = Contract {
            id: "ct_file_scope_lock".into(),
            feature_id: "feat_file_scope_lock".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "implement pubpunk init".into(),
            entry_points: vec!["crates/pubpunk-core/src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["pubpunk init".into()],
            behavior_requirements: vec!["keep tests green".into()],
            allowed_scope: vec!["crates/pubpunk-core/src/lib.rs".into()],
            target_checks: vec!["cargo test -p pubpunk-core".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        prune_generated_cargo_lock_if_out_of_scope(&root, &contract, false).unwrap();

        assert!(!root.join("Cargo.lock").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_present_isolated_changes_to_repo_root_copies_product_files_only() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-orch-sync-isolated-{}-{suffix}",
            std::process::id()
        ));
        let workspace = root.join("workspace");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(workspace.join("crates/pubpunk-cli/src")).unwrap();
        fs::create_dir_all(workspace.join(".punk/runs/run_1")).unwrap();
        fs::create_dir_all(workspace.join(".playwright-mcp/state")).unwrap();
        fs::write(
            workspace.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(workspace.join(".punk/runs/run_1/stdout.log"), "noise\n").unwrap();
        fs::write(
            workspace.join(".playwright-mcp/state/session.json"),
            "{\"ok\":true}\n",
        )
        .unwrap();

        sync_present_isolated_changes_to_repo_root(
            &root,
            &workspace,
            &[
                "crates/pubpunk-cli/src/main.rs".into(),
                ".punk/runs/run_1/stdout.log".into(),
                ".playwright-mcp/state/session.json".into(),
            ],
        )
        .unwrap();

        assert!(root.join("crates/pubpunk-cli/src/main.rs").exists());
        assert!(!root.join(".punk/runs/run_1/stdout.log").exists());
        assert!(!root.join(".playwright-mcp/state/session.json").exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_present_repo_root_changes_to_isolated_workspace_copies_untracked_product_files() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-orch-sync-root-{}-{suffix}",
            std::process::id()
        ));
        let workspace = root.join("workspace");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/pubpunk-cli/src")).unwrap();
        fs::create_dir_all(root.join("crates/pubpunk-core/src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::create_dir_all(root.join(".playwright-mcp/state")).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/Cargo.toml"),
            "[package]\nname='pubpunk-cli'\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/Cargo.toml"),
            "[package]\nname='pubpunk-core'\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            "pub fn init() {}\n",
        )
        .unwrap();
        fs::write(root.join("tests/README.md"), "tests\n").unwrap();
        fs::write(root.join(".punk/runs/run_1/stdout.log"), "noise\n").unwrap();
        fs::write(root.join("target/debug/app"), "bin\n").unwrap();
        fs::write(
            root.join(".playwright-mcp/state/session.json"),
            "{\"ok\":true}\n",
        )
        .unwrap();

        sync_present_repo_root_changes_to_isolated_workspace(
            &root,
            &workspace,
            &[
                "Cargo.toml".into(),
                "crates/pubpunk-cli/Cargo.toml".into(),
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/Cargo.toml".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/README.md".into(),
                ".punk/runs/run_1/stdout.log".into(),
                "target/debug/app".into(),
                ".playwright-mcp/state/session.json".into(),
            ],
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(workspace.join("Cargo.toml")).unwrap(),
            "[workspace]\n"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("crates/pubpunk-cli/src/main.rs")).unwrap(),
            "fn main() {}\n"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("crates/pubpunk-core/src/lib.rs")).unwrap(),
            "pub fn init() {}\n"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("tests/README.md")).unwrap(),
            "tests\n"
        );
        assert!(!workspace.join(".punk/runs/run_1/stdout.log").exists());
        assert!(!workspace.join("target/debug/app").exists());
        assert!(!workspace
            .join(".playwright-mcp/state/session.json")
            .exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_present_repo_root_changes_to_isolated_workspace_skips_self_copy() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-orch-sync-self-root-{}-{suffix}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/pubpunk-core/src")).unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            "pub fn keep() {}
",
        )
        .unwrap();

        sync_present_repo_root_changes_to_isolated_workspace(
            &root,
            &root,
            &["crates/pubpunk-core/src/lib.rs".into()],
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(root.join("crates/pubpunk-core/src/lib.rs")).unwrap(),
            "pub fn keep() {}
"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_present_isolated_changes_to_repo_root_skips_self_copy() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-orch-sync-self-workspace-{}-{suffix}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("tests/init_json.rs"),
            "#[test]
fn ok() {}
",
        )
        .unwrap();

        sync_present_isolated_changes_to_repo_root(&root, &root, &["tests/init_json.rs".into()])
            .unwrap();

        assert_eq!(
            fs::read_to_string(root.join("tests/init_json.rs")).unwrap(),
            "#[test]
fn ok() {}
"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cut_run_panic_finalizes_run_state() {
        let root = std::env::temp_dir().join(format!("punk-orch-finalize-{}", std::process::id()));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service.draft_contract(&FakeDrafter, "add file").unwrap();
        service.approve_contract(&contract.id).unwrap();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            service.cut_run(&PanicExecutor, &contract.id)
        }));
        assert!(result.is_err());

        let run_dir = fs::read_dir(&service.paths.runs_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .next()
            .unwrap();
        let persisted: Run = read_json(&run_dir.join("run.json")).unwrap();
        assert_eq!(persisted.status, RunStatus::Failed);
        assert!(persisted.ended_at.is_some());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cut_run_persists_heartbeat_during_execution() {
        let root = std::env::temp_dir().join(format!("punk-orch-heartbeat-{}", std::process::id()));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service.draft_contract(&FakeDrafter, "add file").unwrap();
        service.approve_contract(&contract.id).unwrap();

        let (run, _receipt) = service
            .cut_run(&HeartbeatObservingExecutor, &contract.id)
            .unwrap();
        let heartbeat_path = service.paths.runs_dir.join(&run.id).join("heartbeat.json");
        let heartbeat: serde_json::Value = read_json(&heartbeat_path).unwrap();
        assert_eq!(heartbeat["state"].as_str(), Some("finished"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn invalid_draft_is_repaired_before_persist() {
        let root = std::env::temp_dir().join(format!("punk-orch-repair-{}", std::process::id()));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(&InvalidThenRepairDrafter, "repair me")
            .unwrap();
        assert_eq!(contract.allowed_scope, vec!["demo.txt".to_string()]);
        assert!(service.approve_contract(&contract.id).is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_preserves_explicit_paths_and_integrity_checks() {
        let root =
            std::env::temp_dir().join(format!("punk-orch-canonicalize-{}", std::process::id()));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let prompt = "Scaffold crate with `crates/punk-council/Cargo.toml` and `crates/punk-council/src/lib.rs`. Target checks should include cargo build -p punk-cli and cargo test -p punk-council. Integrity checks should include cargo test --workspace.";
        let contract = service
            .draft_contract(&ExplicitDetailsIgnoringDrafter, prompt)
            .unwrap();

        assert_eq!(
            contract.allowed_scope,
            vec![
                "crates/punk-council/Cargo.toml".to_string(),
                "crates/punk-council/src/lib.rs".to_string()
            ]
        );
        assert_eq!(contract.entry_points, contract.allowed_scope);
        assert_eq!(
            contract.target_checks,
            vec![
                "cargo build -p punk-cli".to_string(),
                "cargo test -p punk-council".to_string()
            ]
        );
        assert_eq!(
            contract.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_ignores_nested_non_member_workspace_target_checks() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-workspace-targets-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-core/src")).unwrap();
        fs::create_dir_all(root.join("crates/punk-orch/src")).unwrap();
        fs::create_dir_all(root.join("punk/punk-run/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-core/Cargo.toml"),
            "[package]\nname = \"punk-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-orch/Cargo.toml"),
            "[package]\nname = \"punk-orch\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("punk/punk-run/Cargo.toml"),
            "[package]\nname = \"punk-run\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &ScanTargetChecksDrafter,
                "tighten run reporting in punk-orch",
            )
            .unwrap();

        assert!(contract
            .target_checks
            .contains(&"cargo test -p punk-orch".to_string()));
        assert!(!contract
            .target_checks
            .contains(&"cargo test -p punk-run".to_string()));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_allows_bootstrapped_greenfield_rust_repo_without_existing_checks() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-greenfield-rust-intake-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(root.join("AGENTS.md"), "# AGENTS\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &GreenfieldRustDrafter,
                "scaffold Rust workspace and implement pubpunk init + validate",
            )
            .unwrap();

        assert_eq!(
            contract.target_checks,
            vec!["cargo test --workspace".to_string()]
        );
        assert_eq!(
            contract.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );
        assert!(!contract.allowed_scope.is_empty());
        assert!(!contract.entry_points.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_expands_multi_surface_bootstrap_scope_without_existing_crates_dir() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-greenfield-rust-multisurface-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(root.join("AGENTS.md"), "# AGENTS\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &GreenfieldRustDrafter,
                "bootstrap initial Rust workspace for pubpunk touching exactly Cargo.toml, crates/pubpunk-cli, crates/pubpunk-core, and tests; create workspace members and make cargo test --workspace pass",
            )
            .unwrap();

        assert_eq!(contract.entry_points, vec!["Cargo.toml".to_string()]);
        assert_eq!(
            contract.allowed_scope,
            vec![
                "Cargo.toml".to_string(),
                "crates/pubpunk-cli".to_string(),
                "crates/pubpunk-core".to_string(),
                "tests".to_string(),
            ]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_allows_bootstrapped_greenfield_go_repo_without_existing_checks() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-greenfield-go-intake-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(root.join("AGENTS.md"), "# AGENTS\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &GreenfieldGoDrafter,
                "scaffold Go module and implement pubpunk init + validate",
            )
            .unwrap();

        assert_eq!(contract.target_checks, vec!["go test ./...".to_string()]);
        assert_eq!(contract.integrity_checks, vec!["go test ./...".to_string()]);
        assert!(!contract.allowed_scope.is_empty());
        assert!(!contract.entry_points.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_allows_bootstrapped_greenfield_python_repo_without_existing_checks() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-greenfield-python-intake-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(root.join("AGENTS.md"), "# AGENTS\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &GreenfieldPythonDrafter,
                "scaffold Python package and implement pubpunk init + validate",
            )
            .unwrap();

        assert_eq!(contract.target_checks, vec!["pytest".to_string()]);
        assert_eq!(contract.integrity_checks, vec!["pytest".to_string()]);
        assert!(!contract.allowed_scope.is_empty());
        assert!(!contract.entry_points.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_allows_bootstrapped_greenfield_node_repo_without_existing_checks() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-greenfield-node-intake-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(root.join("AGENTS.md"), "# AGENTS\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &GreenfieldNodeDrafter,
                "scaffold TypeScript package and implement pubpunk init + validate",
            )
            .unwrap();

        assert_eq!(contract.target_checks, vec!["npm test".to_string()]);
        assert_eq!(contract.integrity_checks, vec!["npm test".to_string()]);
        assert!(!contract.allowed_scope.is_empty());
        assert!(!contract.entry_points.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_timeout_fallback_prefers_greenfield_rust_scaffold_scope_over_docs() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-greenfield-rust-scope-timeout-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("archive")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(root.join("AGENTS.md"), "# AGENTS\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/PUBPUNK_DEVELOPMENT_HANDOFF.md"),
            "scaffold Rust workspace and implement pubpunk init + validate\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/IMPLEMENTATION_PLAN.md"),
            "workspace scaffold validate init plan\n",
        )
        .unwrap();
        fs::write(root.join("archive/pubpunk-docs.zip"), "zip placeholder\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &TimeoutDrafter,
                "scaffold Rust workspace and implement pubpunk init + validate via go",
            )
            .unwrap();

        assert_eq!(contract.entry_points, vec!["Cargo.toml".to_string()]);
        assert_eq!(
            contract.allowed_scope,
            vec![
                "Cargo.toml".to_string(),
                "crates".to_string(),
                "tests".to_string()
            ]
        );
        assert_eq!(
            contract.target_checks,
            vec!["cargo test --workspace".to_string()]
        );
        assert_eq!(
            contract.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );
        assert_eq!(
            contract.expected_interfaces,
            vec!["initial Rust scaffold".to_string()]
        );
        assert_eq!(
            contract.behavior_requirements,
            vec![summarize_prompt(
                "scaffold Rust workspace and implement pubpunk init + validate via go",
            )]
        );
        assert!(contract
            .allowed_scope
            .iter()
            .all(|path| !path.starts_with("docs/") && !path.starts_with("archive/")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_timeout_fallback_keeps_crates_scope_when_plain_prompt_mentions_tests() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-greenfield-rust-plain-tests-scope-timeout-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(root.join("AGENTS.md"), "# AGENTS\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &TimeoutDrafter,
                "scaffold Rust workspace and implement pubpunk init command with --json output and tests",
            )
            .unwrap();

        assert_eq!(contract.entry_points, vec!["Cargo.toml".to_string()]);
        assert_eq!(
            contract.allowed_scope,
            vec![
                "tests".to_string(),
                "Cargo.toml".to_string(),
                "crates".to_string()
            ]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_explicit_follow_up_touch_set_does_not_readd_bootstrap_scope() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-explicit-followup-touch-set-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::create_dir_all(root.join("crates/pubpunk-cli/src")).unwrap();
        fs::create_dir_all(root.join("crates/pubpunk-core/src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(root.join("AGENTS.md"), "# AGENTS\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\nresolver='2'\n").unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            "pub fn init() {}\n",
        )
        .unwrap();
        fs::write(root.join("tests/README.md"), "tests\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &BroadBootstrapDrafter,
                "implement pubpunk init command touching exactly crates/pubpunk-cli/src/main.rs, crates/pubpunk-core/src/lib.rs, and tests; add --json output and keep cargo test --workspace green",
            )
            .unwrap();

        assert_eq!(
            contract.allowed_scope,
            vec![
                "crates/pubpunk-cli/src/main.rs".to_string(),
                "crates/pubpunk-core/src/lib.rs".to_string(),
                "tests".to_string(),
            ]
        );
        assert_eq!(
            contract.entry_points,
            vec![
                "crates/pubpunk-cli/src/main.rs".to_string(),
                "crates/pubpunk-core/src/lib.rs".to_string(),
            ]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_timeout_fallback_rich_init_prompt_keeps_file_scope_on_bootstrapped_repo() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-rich-init-file-scope-timeout-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::create_dir_all(root.join("crates/pubpunk-cli/src")).unwrap();
        fs::create_dir_all(root.join("crates/pubpunk-core/src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(root.join("AGENTS.md"), "# AGENTS\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\nresolver='2'\n").unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/Cargo.toml"),
            "[package]\nname = \"pubpunk-cli\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/Cargo.toml"),
            "[package]\nname = \"pubpunk-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {\n    let _ = \"init --json --force --project-root\";\n}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            "pub fn init() -> &'static str {\n    \"project.toml style/style.toml style/voice.md\"\n}\n",
        )
        .unwrap();
        fs::write(root.join("tests/init_json.rs"), "#[test]\nfn smoke() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &TimeoutDrafter,
                "implement pubpunk init command with canonical starter files project.toml, style/style.toml, style/voice.md, style/lexicon.toml, style/normalize.toml, agent/skill.md, local/.gitignore; support --json, --force, --project-root; add tests; keep cargo test --workspace green",
            )
            .unwrap();

        assert!(!contract.allowed_scope.iter().any(|scope| scope == "crates"));
        assert!(!contract
            .allowed_scope
            .iter()
            .any(|scope| scope == "Cargo.toml"));
        assert!(contract
            .entry_points
            .iter()
            .all(|scope| scope.contains('/') || scope.ends_with(".toml")));
        assert!(contract
            .allowed_scope
            .iter()
            .any(|scope| scope.ends_with("tests/init_json.rs")
                || scope.ends_with("tests/README.md")
                || scope == "tests"));
        assert!(contract.allowed_scope.len() <= 4);
        assert!(contract
            .expected_interfaces
            .iter()
            .any(|value| value.contains("`--json`")));
        assert!(contract
            .expected_interfaces
            .iter()
            .any(|value| value.contains("`--force`")));
        assert!(contract
            .expected_interfaces
            .iter()
            .any(|value| value.contains("`--project-root`")));
        assert!(contract
            .behavior_requirements
            .iter()
            .any(|value| value.contains("project.toml")
                && value.contains("style/style.toml")
                && value.contains("agent/skill.md")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_timeout_fallback_prefers_greenfield_go_scaffold_scope_over_docs() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-greenfield-go-scope-timeout-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("archive")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(root.join("AGENTS.md"), "# AGENTS\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/IMPLEMENTATION_PLAN.md"),
            "scaffold go module and validate pubpunk init\n",
        )
        .unwrap();
        fs::write(root.join("archive/pubpunk-docs.zip"), "zip placeholder\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &TimeoutDrafter,
                "scaffold Go module and implement pubpunk init + validate",
            )
            .unwrap();

        assert_eq!(contract.entry_points, vec!["go.mod".to_string()]);
        assert_eq!(
            contract.allowed_scope,
            vec![
                "go.mod".to_string(),
                "cmd".to_string(),
                "internal".to_string(),
                "pkg".to_string()
            ]
        );
        assert_eq!(contract.target_checks, vec!["go test ./...".to_string()]);
        assert_eq!(contract.integrity_checks, vec!["go test ./...".to_string()]);
        assert_eq!(
            contract.expected_interfaces,
            vec!["initial Go scaffold".to_string()]
        );
        assert_eq!(
            contract.behavior_requirements,
            vec![summarize_prompt(
                "scaffold Go module and implement pubpunk init + validate",
            )]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_timeout_fallback_prefers_greenfield_python_scaffold_scope_over_docs() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-greenfield-python-scope-timeout-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("archive")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(root.join("AGENTS.md"), "# AGENTS\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/SPEC.md"),
            "scaffold python package and validate pubpunk init\n",
        )
        .unwrap();
        fs::write(root.join("archive/pubpunk-docs.zip"), "zip placeholder\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &TimeoutDrafter,
                "scaffold Python package and implement pubpunk init + validate",
            )
            .unwrap();

        assert_eq!(contract.entry_points, vec!["pyproject.toml".to_string()]);
        assert_eq!(
            contract.allowed_scope,
            vec![
                "pyproject.toml".to_string(),
                "src".to_string(),
                "tests".to_string()
            ]
        );
        assert_eq!(contract.target_checks, vec!["pytest".to_string()]);
        assert_eq!(contract.integrity_checks, vec!["pytest".to_string()]);
        assert_eq!(
            contract.expected_interfaces,
            vec!["initial Python scaffold".to_string()]
        );
        assert_eq!(
            contract.behavior_requirements,
            vec![summarize_prompt(
                "scaffold Python package and implement pubpunk init + validate",
            )]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_timeout_fallback_prefers_greenfield_node_scaffold_scope_over_docs() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-greenfield-node-scope-timeout-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("archive")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(root.join("AGENTS.md"), "# AGENTS\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/SPEC.md"),
            "scaffold TypeScript package and validate pubpunk init\n",
        )
        .unwrap();
        fs::write(root.join("archive/pubpunk-docs.zip"), "zip placeholder\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &TimeoutDrafter,
                "scaffold TypeScript package and implement pubpunk init + validate",
            )
            .unwrap();

        assert_eq!(contract.entry_points, vec!["package.json".to_string()]);
        assert_eq!(
            contract.allowed_scope,
            vec![
                "package.json".to_string(),
                "tsconfig.json".to_string(),
                "src".to_string(),
                "tests".to_string()
            ]
        );
        assert_eq!(
            contract.expected_interfaces,
            vec!["initial TypeScript/Node scaffold".to_string()]
        );
        assert_eq!(
            contract.behavior_requirements,
            vec![summarize_prompt(
                "scaffold TypeScript package and implement pubpunk init + validate",
            )]
        );
        assert_eq!(contract.target_checks, vec!["npm test".to_string()]);
        assert_eq!(contract.integrity_checks, vec!["npm test".to_string()]);
        assert!(contract
            .allowed_scope
            .iter()
            .all(|path| !path.starts_with("docs/") && !path.starts_with("archive/")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn refine_requires_draft_contract() {
        let root = std::env::temp_dir().join(format!("punk-orch-refine-{}", std::process::id()));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        let service = OrchService::new(&root, &global).unwrap();
        let contract = service.draft_contract(&FakeDrafter, "add file").unwrap();
        let refined = service
            .refine_contract(&FakeDrafter, &contract.id, "narrow scope")
            .unwrap();
        assert_eq!(refined.id, contract.id);
        assert_eq!(refined.version, contract.version);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_reports_phase_for_drafter_failures() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-draft-phase-error-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let err = service
            .draft_contract(&FailingDrafter, "add file")
            .unwrap_err()
            .to_string();
        assert!(err.contains("phase drafter request"));
        assert!(err.contains("simulated drafter failure"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_uses_timeout_fallback_when_initial_drafter_times_out() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-timeout-draft-fallback-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-orch/src")).unwrap();
        fs::create_dir_all(root.join("crates/punk-core/src")).unwrap();
        fs::create_dir_all(root.join("docs/product")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-orch/Cargo.toml"),
            "[package]\nname = \"punk-orch\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-core/Cargo.toml"),
            "[package]\nname = \"punk-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-orch/src/lib.rs"),
            "pub fn orch() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-core/src/lib.rs"),
            "pub fn core() {}\n",
        )
        .unwrap();
        fs::write(root.join("docs/product/CLI.md"), "# CLI\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let prompt = "Improve drafter timeout resilience for punk start and plot refine. Restrict allowed_scope exactly to crates/punk-orch/src/lib.rs; crates/punk-core/src/lib.rs; docs/product/CLI.md. Target checks should include cargo test -p punk-core -p punk-orch. Integrity checks should include cargo test --workspace.";
        let contract = service.draft_contract(&TimeoutDrafter, prompt).unwrap();

        assert_eq!(
            contract.allowed_scope,
            vec![
                "crates/punk-orch/src/lib.rs".to_string(),
                "crates/punk-core/src/lib.rs".to_string(),
                "docs/product/CLI.md".to_string(),
            ]
        );
        assert_eq!(contract.entry_points, contract.allowed_scope);
        assert_eq!(
            contract.target_checks,
            vec!["cargo test -p punk-core -p punk-orch".to_string()]
        );
        assert_eq!(
            contract.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn proposal_scope_readds_missing_entry_points_before_validation() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-timeout-entry-point-coverage-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/pubpunk-cli/src")).unwrap();
        fs::create_dir_all(root.join("crates/pubpunk-core/src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/Cargo.toml"),
            "[package]\nname = \"pubpunk-cli\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/Cargo.toml"),
            "[package]\nname = \"pubpunk-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            "pub fn init() {}\n",
        )
        .unwrap();
        fs::write(root.join("tests/README.md"), "tests\n").unwrap();

        let mut proposal = DraftProposal {
            title: "Implement pubpunk init".to_string(),
            summary: "Timeout fallback draft".to_string(),
            entry_points: vec![
                "crates/pubpunk-cli/Cargo.toml".to_string(),
                "crates/pubpunk-core/Cargo.toml".to_string(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["pubpunk init".to_string()],
            behavior_requirements: vec!["implement pubpunk init".to_string()],
            allowed_scope: vec![
                "crates/pubpunk-cli/src/main.rs".to_string(),
                "crates/pubpunk-core/src/lib.rs".to_string(),
                "tests/README.md".to_string(),
            ],
            target_checks: vec!["cargo test --workspace".to_string()],
            integrity_checks: vec!["cargo test --workspace".to_string()],
            risk_level: "medium".to_string(),
        };

        ensure_proposal_scope_covers_entry_points(&mut proposal);

        assert!(proposal
            .allowed_scope
            .contains(&"crates/pubpunk-cli/Cargo.toml".to_string()));
        assert!(proposal
            .allowed_scope
            .contains(&"crates/pubpunk-core/Cargo.toml".to_string()));
        assert!(validate_draft_proposal(&root, &proposal).is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_repairs_missing_entry_point_scope_coverage_before_validation() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-entry-point-scope-leak-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn init() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &EntryPointScopeLeakDrafter,
                "implement init behavior in src/lib.rs and keep cargo test green",
            )
            .unwrap();

        assert!(contract.entry_points.contains(&"src/lib.rs".to_string()));
        assert!(contract.allowed_scope.contains(&"src/lib.rs".to_string()));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_timeout_fallback_keeps_entry_points_covered_for_pubpunk_init_prompt() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-timeout-pubpunk-init-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/pubpunk-cli/src")).unwrap();
        fs::create_dir_all(root.join("crates/pubpunk-core/src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/Cargo.toml"),
            "[package]\nname = \"pubpunk-cli\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/Cargo.toml"),
            "[package]\nname = \"pubpunk-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            "pub fn init() {}\n",
        )
        .unwrap();
        fs::write(root.join("tests/README.md"), "tests\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let prompt = "implement pubpunk init command in crates/pubpunk-cli and crates/pubpunk-core with tests: when run, it creates the canonical .pubpunk skeleton and returns JSON for --json; keep cargo test --workspace green";
        let contract = service.draft_contract(&TimeoutDrafter, prompt).unwrap();

        assert!(!contract.entry_points.is_empty());
        for entry_point in &contract.entry_points {
            assert!(
                contract.allowed_scope.iter().any(|scope| {
                    let scope = scope.trim_matches('/');
                    let entry = entry_point.trim_matches('/');
                    entry == scope || entry.starts_with(&format!("{scope}/"))
                }),
                "entry point {} must be covered by allowed_scope {:?}",
                entry_point,
                contract.allowed_scope
            );
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn refine_contract_applies_explicit_guidance_scope_exactly() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-refine-explicit-scope-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("Packages/InterviewCoachKit/Sources/InterviewCoachDevUI"))
            .unwrap();
        fs::create_dir_all(root.join("Packages/InterviewCoachKit/Tests/InterviewCoachDevUITests"))
            .unwrap();
        fs::create_dir_all(root.join("punk/punk-run/src")).unwrap();
        fs::write(root.join("Makefile"), "test:\n\t@echo ok\n").unwrap();
        fs::write(
            root.join(
                "Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/DevAppViewModel.swift",
            ),
            "struct DevAppViewModel {}\n",
        )
        .unwrap();
        fs::write(
            root.join(
                "Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/MainWindowView.swift",
            ),
            "struct MainWindowView {}\n",
        )
        .unwrap();
        fs::write(
            root.join("Packages/InterviewCoachKit/Tests/InterviewCoachDevUITests/DevAppViewModelTests.swift"),
            "func testExample() {}\n",
        )
        .unwrap();
        fs::write(root.join("punk/punk-run/src/main.rs"), "fn main() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &FakeDrafter,
                "Add the next bounded dev-only slice for interviewcoach.",
            )
            .unwrap();
        let guidance = "Expand allowed_scope exactly to the files needed for the copy/export trace slice: `Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/DevAppViewModel.swift`; `Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/MainWindowView.swift`; `Packages/InterviewCoachKit/Tests/InterviewCoachDevUITests/DevAppViewModelTests.swift`. Do not include `punk/punk-run` in allowed_scope. Target checks should include make test. Integrity checks should include make test.";
        let refined = service
            .refine_contract(&FakeDrafter, &contract.id, guidance)
            .unwrap();

        assert_eq!(
            refined.allowed_scope,
            vec![
                "Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/DevAppViewModel.swift"
                    .to_string(),
                "Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/MainWindowView.swift"
                    .to_string(),
                "Packages/InterviewCoachKit/Tests/InterviewCoachDevUITests/DevAppViewModelTests.swift"
                    .to_string(),
            ]
        );
        assert_eq!(refined.entry_points, refined.allowed_scope);
        assert_eq!(refined.target_checks, vec!["make test".to_string()]);
        assert_eq!(refined.integrity_checks, vec!["make test".to_string()]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn refine_contract_uses_timeout_fallback_with_explicit_guidance() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-timeout-refine-fallback-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-orch/src")).unwrap();
        fs::create_dir_all(root.join("crates/punk-core/src")).unwrap();
        fs::create_dir_all(root.join("docs/product")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-orch/Cargo.toml"),
            "[package]\nname = \"punk-orch\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-core/Cargo.toml"),
            "[package]\nname = \"punk-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-orch/src/lib.rs"),
            "pub fn orch() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-core/src/lib.rs"),
            "pub fn core() {}\n",
        )
        .unwrap();
        fs::write(root.join("docs/product/CLI.md"), "# CLI\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(&FakeDrafter, "harden drafter timeout fallback")
            .unwrap();
        let guidance = "Restrict allowed_scope exactly to crates/punk-orch/src/lib.rs; crates/punk-core/src/lib.rs; docs/product/CLI.md. target_checks must contain exactly one command: cargo test -p punk-core -p punk-orch. integrity_checks must contain exactly one command: cargo test --workspace.";
        let refined = service
            .refine_contract(&TimeoutDrafter, &contract.id, guidance)
            .unwrap();

        assert_eq!(
            refined.allowed_scope,
            vec![
                "crates/punk-orch/src/lib.rs".to_string(),
                "crates/punk-core/src/lib.rs".to_string(),
                "docs/product/CLI.md".to_string(),
            ]
        );
        assert_eq!(refined.entry_points, refined.allowed_scope);
        assert_eq!(
            refined.target_checks,
            vec!["cargo test -p punk-core -p punk-orch".to_string()]
        );
        assert_eq!(
            refined.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn refine_contract_preserves_exact_combined_target_checks() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-refine-exact-target-checks-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-core/src")).unwrap();
        fs::create_dir_all(root.join("crates/punk-orch/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-core/Cargo.toml"),
            "[package]\nname = \"punk-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-orch/Cargo.toml"),
            "[package]\nname = \"punk-orch\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-core/src/lib.rs"),
            "pub fn core() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-orch/src/lib.rs"),
            "pub fn orch() {}\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(&ScanTargetChecksDrafter, "tighten target check guidance")
            .unwrap();
        let guidance = "Keep allowed_scope exactly as-is. target_checks must contain exactly one command: cargo test -p punk-core -p punk-orch. integrity_checks must contain exactly one command: cargo test --workspace. Remove every other target check.";
        let refined = service
            .refine_contract(&ScanTargetChecksDrafter, &contract.id, guidance)
            .unwrap();

        assert_eq!(
            refined.target_checks,
            vec!["cargo test -p punk-core -p punk-orch".to_string()]
        );
        assert_eq!(
            refined.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn refine_contract_ignores_generated_runtime_artifact_paths_in_exact_scope_guidance() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-refine-generated-artifact-scope-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-orch/src")).unwrap();
        fs::create_dir_all(root.join("crates/punk-cli/src")).unwrap();
        fs::create_dir_all(root.join(".punk/project")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-orch/Cargo.toml"),
            "[package]\nname = \"punk-orch\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-cli/Cargo.toml"),
            "[package]\nname = \"punk-cli\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-orch/src/lib.rs"),
            "pub fn orch() {}\n",
        )
        .unwrap();
        fs::write(root.join("crates/punk-cli/src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join(".punk/project/harness.json"), "{}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(
                &FakeDrafter,
                "Add the next bounded harness slice for project inspect.",
            )
            .unwrap();
        let guidance = "Restrict allowed_scope exactly to these two files and nothing else: crates/punk-orch/src/lib.rs; crates/punk-cli/src/main.rs. Mention the generated packet `.punk/project/harness.json` in docs, but do not include it in allowed_scope or entry_points because it is a runtime artifact destination. target_checks must contain exactly one command: cargo test -p punk-core -p punk-orch. integrity_checks must contain exactly one command: cargo test --workspace.";
        let refined = service
            .refine_contract(&FakeDrafter, &contract.id, guidance)
            .unwrap();

        assert_eq!(
            refined.allowed_scope,
            vec![
                "crates/punk-orch/src/lib.rs".to_string(),
                "crates/punk-cli/src/main.rs".to_string(),
            ]
        );
        assert_eq!(refined.entry_points, refined.allowed_scope);
        assert_eq!(
            refined.target_checks,
            vec!["cargo test -p punk-core -p punk-orch".to_string()]
        );
        assert_eq!(
            refined.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_uses_bounded_fallback_after_refine_failure() {
        let root =
            std::env::temp_dir().join(format!("punk-orch-fallback-draft-{}", std::process::id()));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/lib.rs"),
            "pub mod proposal;\npub mod storage;\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/proposal.rs"),
            "pub fn run() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/storage.rs"),
            "pub fn persist() {}\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let prompt = "Implement the proposal phase in punk-council. Add bounded proposal orchestration that persists proposal artifacts under .punk/council/<id>/proposals/. Target checks should include cargo build -p punk-cli and cargo test -p punk-council. Integrity checks should include cargo test --workspace.";
        let contract = service
            .draft_contract(&PlaceholderScopeDrafter, prompt)
            .unwrap();

        assert_eq!(
            contract.allowed_scope,
            vec![
                "crates/punk-council/src/proposal.rs".to_string(),
                "crates/punk-council/src/lib.rs".to_string(),
                "crates/punk-council/src/storage.rs".to_string(),
            ]
        );
        assert_eq!(
            contract.target_checks,
            vec![
                "cargo build -p punk-cli".to_string(),
                "cargo test -p punk-council".to_string(),
            ]
        );
        assert_eq!(
            contract.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_contract_applies_bounded_fallback_for_structurally_invalid_clean_draft() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-structural-fallback-{}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/lib.rs"),
            "pub mod storage;\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/storage.rs"),
            "pub fn persist() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/packet.rs"),
            "pub fn packet() {}\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let prompt = "Add council synthesis and final record completion in punk-council. Take the deterministic scoreboard and produce a typed CouncilSynthesis with Leader, Hybrid, or Escalate, persist synthesis.json, and write a final record.json that points to packet, proposals, reviews, scoreboard, and synthesis artifacts. Keep the slice advisory-only and inside punk-council. Target checks should include cargo build -p punk-cli and cargo test -p punk-council. Integrity checks should include cargo test --workspace.";
        let contract = service
            .draft_contract(&StructurallyInvalidSynthesisDrafter, prompt)
            .unwrap();

        assert_eq!(
            contract.allowed_scope,
            vec![
                "crates/punk-council/src/synthesis.rs".to_string(),
                "crates/punk-council/src/lib.rs".to_string(),
                "crates/punk-council/src/storage.rs".to_string(),
            ]
        );
        assert_eq!(contract.entry_points, contract.allowed_scope);
        assert_eq!(
            contract.target_checks,
            vec![
                "cargo build -p punk-cli".to_string(),
                "cargo test -p punk-council".to_string(),
            ]
        );
        assert_eq!(
            contract.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn status_reports_current_vcs_snapshot_details() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-status-vcs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), "target\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let status = service.status(None).unwrap();
        assert_eq!(status.vcs_backend, Some(VcsKind::Git));
        assert!(status.vcs_ref.is_some());
        assert_eq!(
            status
                .workspace_root
                .as_deref()
                .map(std::path::PathBuf::from)
                .map(|path| fs::canonicalize(path).unwrap()),
            Some(fs::canonicalize(&root).unwrap())
        );

        fs::write(
            root.join("src/lib.rs"),
            "pub fn demo() { println!(\"x\"); }\n",
        )
        .unwrap();
        let dirty = service.status(None).unwrap();
        assert!(dirty.vcs_dirty);

        if std::process::Command::new("jj")
            .arg("--version")
            .output()
            .is_ok()
        {
            std::process::Command::new("jj")
                .args(["git", "init", "--colocate", "."])
                .current_dir(&root)
                .output()
                .unwrap();
            let jj_status = service.status(None).unwrap();
            assert_eq!(jj_status.vcs_backend, Some(VcsKind::Jj));
        }

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
    }

    #[test]
    fn inspect_project_and_run_include_live_vcs() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-inspect-vcs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let global = std::env::temp_dir().join(format!(
            "punk-orch-inspect-vcs-global-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let project = service.bootstrap_project().unwrap();
        let project_inspect = service.inspect(&project.id).unwrap();
        assert_eq!(project_inspect["live_vcs"]["backend"].as_str(), Some("git"));
        assert!(project_inspect["live_vcs"]["workspace_root"]
            .as_str()
            .is_some());

        let contract = service.draft_contract(&FakeDrafter, "add file").unwrap();
        service.approve_contract(&contract.id).unwrap();
        let (run, _) = service.cut_run(&FakeExecutor, &contract.id).unwrap();
        let run_inspect = service.inspect(&run.id).unwrap();
        assert_eq!(run_inspect["id"].as_str(), Some(run.id.as_str()));
        assert_eq!(run_inspect["live_vcs"]["backend"].as_str(), Some("git"));
        assert!(run_inspect["live_vcs"]["ref"].as_str().is_some());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
    }

    #[test]
    fn inspect_project_overlay_reports_guidance_capabilities_and_skill_refs() {
        let base = std::env::temp_dir().join(format!(
            "punk-orch-project-overlay-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = base.join("interviewcoach");
        let global = std::env::temp_dir().join(format!(
            "punk-orch-project-overlay-global-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bus = std::env::temp_dir().join(format!(
            "punk-orch-project-overlay-bus-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _bus_guard = EnvVarGuard::set("PUNK_BUS_DIR", &bus);
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
        let _ = fs::remove_dir_all(&bus);

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='interviewcoach'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        fs::write(root.join("Makefile"), "test:\n\tcargo test\n").unwrap();
        fs::write(root.join("playwright.config.ts"), "export default {};\n").unwrap();
        fs::create_dir_all(root.join("logs")).unwrap();
        fs::write(
            root.join("prometheus.yml"),
            "global:\n  scrape_interval: 15s\n",
        )
        .unwrap();
        fs::write(
            root.join("otel-collector.yaml"),
            "receivers: {}\nexporters: {}\nservice: {}\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(
            root.join(".punk/bootstrap/interviewcoach-core.md"),
            "Use existing architecture.\n",
        )
        .unwrap();
        fs::write(root.join("AGENTS.md"), "# AGENTS\n").unwrap();
        fs::create_dir_all(root.join(".punk")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();

        let skills_dir = bus.parent().unwrap().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(
            skills_dir.join("interviewcoach-core.md"),
            "---\nname: interviewcoach-core\ndescription: Core rules\nproject: [\"interviewcoach\"]\n---\n\nUse existing architecture.\n",
        )
        .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let overlay = service.inspect_project_overlay().unwrap();

        assert_eq!(
            overlay.bootstrap_ref.as_deref(),
            Some(".punk/bootstrap/interviewcoach-core.md")
        );
        assert_eq!(
            overlay.agent_guidance_ref,
            vec!["AGENTS.md", ".punk/AGENT_START.md"]
        );
        assert!(overlay
            .safe_default_checks
            .iter()
            .any(|check| check == "make test"));
        assert!(overlay
            .safe_default_checks
            .iter()
            .any(|check| check == "cargo test"));
        assert!(overlay
            .project_skill_refs
            .iter()
            .any(|path| path.ends_with("interviewcoach-core.md")));
        assert!(overlay.capability_summary.bootstrap_ready);
        assert!(overlay.capability_summary.project_guidance_ready);
        assert!(overlay.capability_summary.staged_ready);
        assert!(overlay.capability_summary.autonomous_ready);
        assert!(overlay.harness_summary.inspect_ready);
        assert!(overlay.harness_summary.bootable_per_workspace);
        assert!(overlay.harness_summary.ui_legible);
        assert!(overlay.harness_summary.logs_legible);
        assert!(overlay.harness_summary.metrics_legible);
        assert!(overlay.harness_summary.traces_legible);
        assert_eq!(overlay.harness_spec_ref, ".punk/project/harness.json");
        assert_eq!(overlay.harness_spec.project_id, overlay.project_id);
        assert_eq!(overlay.harness_spec.derivation_source, "repo_markers_v1");
        assert!(overlay.harness_spec.inspect_ready);
        assert!(overlay.harness_spec.bootable_per_workspace);
        assert_eq!(
            overlay.harness_spec.profiles,
            vec![PersistedHarnessProfile {
                name: "default".into(),
                validation_surfaces: vec![
                    "command".into(),
                    "ui_snapshot".into(),
                    "log_query".into(),
                    "metric_assertion".into(),
                    "trace_assertion".into(),
                ],
                validation_recipes: vec![
                    PersistedHarnessRecipe {
                        kind: "artifact_assertion".into(),
                        path: ".punk/bootstrap/interviewcoach-core.md".into(),
                    },
                    PersistedHarnessRecipe {
                        kind: "artifact_assertion".into(),
                        path: "AGENTS.md".into(),
                    },
                    PersistedHarnessRecipe {
                        kind: "artifact_assertion".into(),
                        path: ".punk/AGENT_START.md".into(),
                    },
                ],
            }]
        );
        let persisted: PersistedHarnessSpec =
            read_json(&root.join(".punk/project/harness.json")).unwrap();
        assert_eq!(persisted, overlay.harness_spec);
        assert_eq!(
            overlay.status_scope_mode,
            format!("project:{}", overlay.project_id)
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&global);
        let _ = fs::remove_dir_all(&bus);
    }

    #[test]
    fn inspect_project_overlay_persists_empty_harness_profile_when_markers_missing() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-project-overlay-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let global = std::env::temp_dir().join(format!(
            "punk-orch-project-overlay-empty-global-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let overlay = service.inspect_project_overlay().unwrap();

        assert_eq!(overlay.harness_spec_ref, ".punk/project/harness.json");
        assert!(!overlay.harness_summary.inspect_ready);
        assert!(!overlay.harness_summary.bootable_per_workspace);
        assert!(!overlay.harness_spec.inspect_ready);
        assert!(overlay.harness_spec.profiles.is_empty());
        let persisted: PersistedHarnessSpec =
            read_json(&root.join(".punk/project/harness.json")).unwrap();
        assert_eq!(persisted, overlay.harness_spec);

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
    }

    #[test]
    fn inspect_work_ledger_projects_current_artifact_chain() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-work-ledger-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let global = std::env::temp_dir().join(format!(
            "punk-orch-work-ledger-global-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(&FakeDrafter, "add demo work")
            .unwrap();
        service.approve_contract(&contract.id).unwrap();
        let (run, _receipt) = service.cut_run(&FakeExecutor, &contract.id).unwrap();

        let decision = punk_domain::DecisionObject {
            id: format!("dec_{}", run.id.trim_start_matches("run_")),
            run_id: run.id.clone(),
            contract_id: contract.id.clone(),
            decision: punk_domain::Decision::Accept,
            deterministic_status: punk_domain::DeterministicStatus::Pass,
            target_status: punk_domain::CheckStatus::Pass,
            integrity_status: punk_domain::CheckStatus::Pass,
            confidence_estimate: 1.0,
            decision_basis: vec![
                "target checks passed".into(),
                "integrity checks passed".into(),
            ],
            contract_ref: format!(".punk/contracts/{}/v1.json", run.feature_id),
            receipt_ref: format!(".punk/runs/{}/receipt.json", run.id),
            check_refs: Vec::new(),
            command_evidence: Vec::new(),
            declared_harness_evidence: Vec::new(),
            harness_evidence: Vec::new(),
            created_at: now_rfc3339(),
        };
        let decision_path = root
            .join(".punk/decisions")
            .join(format!("{}.json", decision.id));
        write_json(&decision_path, &decision).unwrap();
        let proof = punk_domain::Proofpack {
            id: format!("proof_{}", decision.id.trim_start_matches("dec_")),
            decision_id: decision.id.clone(),
            run_id: run.id.clone(),
            contract_ref: decision.contract_ref.clone(),
            receipt_ref: decision.receipt_ref.clone(),
            decision_ref: format!(".punk/decisions/{}.json", decision.id),
            check_refs: Vec::new(),
            command_evidence: vec![
                punk_domain::CommandEvidence {
                    evidence_type: "command".into(),
                    lane: "target".into(),
                    command: "cargo test -p punk-orch".into(),
                    status: punk_domain::CheckStatus::Pass,
                    summary: "target check passed".into(),
                    stdout_ref: None,
                    stderr_ref: None,
                },
                punk_domain::CommandEvidence {
                    evidence_type: "command".into(),
                    lane: "integrity".into(),
                    command: "cargo test --workspace".into(),
                    status: punk_domain::CheckStatus::Pass,
                    summary: "integrity check passed".into(),
                    stdout_ref: None,
                    stderr_ref: None,
                },
            ],
            declared_harness_evidence: Vec::new(),
            harness_evidence: Vec::new(),
            hashes: Default::default(),
            summary: format!("proof for {}", decision.id),
            created_at: now_rfc3339(),
        };
        let proof_dir = root.join(".punk/proofs").join(&decision.id);
        fs::create_dir_all(&proof_dir).unwrap();
        let proof_path = proof_dir.join("proofpack.json");
        write_json(&proof_path, &proof).unwrap();

        let contract_ref = format!(".punk/contracts/{}/v1.json", run.feature_id);
        let run_ref = format!(".punk/runs/{}/run.json", run.id);
        let receipt_ref = format!(".punk/runs/{}/receipt.json", run.id);
        let decision_ref = format!(".punk/decisions/{}.json", decision.id);
        let proof_ref = format!(".punk/proofs/{}/proofpack.json", decision.id);

        let ledger = service.inspect_work_ledger(Some(&run.feature_id)).unwrap();
        assert_eq!(ledger.work_id, run.feature_id);
        assert_eq!(ledger.goal_ref.as_deref(), Some("add demo work"));
        assert_eq!(
            ledger.active_contract_ref.as_deref(),
            Some(contract_ref.as_str())
        );
        assert_eq!(ledger.latest_run_ref.as_deref(), Some(run_ref.as_str()));
        assert_eq!(
            ledger.latest_receipt_ref.as_deref(),
            Some(receipt_ref.as_str())
        );
        assert_eq!(
            ledger.latest_decision_ref.as_deref(),
            Some(decision_ref.as_str())
        );
        assert_eq!(ledger.latest_proof_ref.as_deref(), Some(proof_ref.as_str()));
        assert_eq!(
            ledger.latest_proof_command_evidence_summary,
            vec![
                "target pass: cargo test -p punk-orch".to_string(),
                "integrity pass: cargo test --workspace".to_string()
            ]
        );
        assert_eq!(ledger.lifecycle_state, "accepted");
        assert_eq!(ledger.next_action.as_deref(), Some("inspect_proof"));
        assert_eq!(ledger.next_action_ref.as_deref(), Some(proof.id.as_str()));
        assert_eq!(ledger.blocked_reason, None);

        let latest = service.inspect_work_ledger(None).unwrap();
        assert_eq!(latest.work_id, ledger.work_id);
        assert_eq!(
            latest.latest_receipt_ref.as_deref(),
            ledger.latest_receipt_ref.as_deref()
        );
        let via_run = service.inspect_work_ledger(Some(&run.id)).unwrap();
        assert_eq!(via_run.work_id, ledger.work_id);
        let via_decision = service.inspect_work_ledger(Some(&decision.id)).unwrap();
        assert_eq!(via_decision.work_id, ledger.work_id);
        let via_proof = service.inspect_work_ledger(Some(&proof.id)).unwrap();
        assert_eq!(via_proof.work_id, ledger.work_id);

        let status = service.status(None).unwrap();
        assert_eq!(status.work_id.as_deref(), Some(ledger.work_id.as_str()));
        assert_eq!(status.lifecycle_state.as_deref(), Some("accepted"));
        assert_eq!(status.autonomy_outcome, None);
        assert_eq!(status.recovery_contract_ref, None);
        assert_eq!(status.next_action.as_deref(), Some("inspect_proof"));
        assert_eq!(status.next_action_ref.as_deref(), Some(proof.id.as_str()));
        let inspect_command = format!("punk inspect {} --json", proof.id);
        assert_eq!(
            status.suggested_command.as_deref(),
            Some(inspect_command.as_str())
        );
        assert_eq!(
            status.last_contract_id.as_deref(),
            Some(contract.id.as_str())
        );
        assert_eq!(status.last_run_id.as_deref(), Some(run.id.as_str()));
        assert_eq!(
            status.last_decision_id.as_deref(),
            Some(decision.id.as_str())
        );

        let status_for_run = service.status(Some(&run.id)).unwrap();
        assert_eq!(
            status_for_run.work_id.as_deref(),
            Some(ledger.work_id.as_str())
        );
        let status_for_project = service
            .status(Some(&service.bootstrap_project().unwrap().id))
            .unwrap();
        assert_eq!(
            status_for_project.work_id.as_deref(),
            Some(ledger.work_id.as_str())
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
    }

    #[test]
    fn status_prefers_feature_with_latest_ledger_activity_over_newer_feature_timestamp() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-latest-ledger-activity-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let global = std::env::temp_dir().join(format!(
            "punk-orch-latest-ledger-activity-global-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let project = service.bootstrap_project().unwrap();
        fs::write(root.join("demo-a.txt"), "a\n").unwrap();
        fs::write(root.join("demo-b.txt"), "b\n").unwrap();

        let proposal_a = DraftProposal {
            title: "feature a".into(),
            summary: "first feature".into(),
            entry_points: vec!["demo-a.txt".into()],
            import_paths: vec![],
            expected_interfaces: vec!["demo a".into()],
            behavior_requirements: vec!["update demo a".into()],
            allowed_scope: vec!["demo-a.txt".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
        };
        let (_feature_a, contract_a) = service
            .persist_draft_contract(&project, "first feature", &proposal_a)
            .unwrap();
        service.approve_contract(&contract_a.id).unwrap();
        let (_run_a1, _receipt_a1) = service.cut_run(&FakeExecutor, &contract_a.id).unwrap();

        let proposal_b = DraftProposal {
            title: "feature b".into(),
            summary: "second feature".into(),
            entry_points: vec!["demo-b.txt".into()],
            import_paths: vec![],
            expected_interfaces: vec!["demo b".into()],
            behavior_requirements: vec!["update demo b".into()],
            allowed_scope: vec!["demo-b.txt".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
        };
        let (_feature_b, contract_b) = service
            .persist_draft_contract(&project, "second feature", &proposal_b)
            .unwrap();
        service.approve_contract(&contract_b.id).unwrap();

        let stale_run = Run {
            id: "run_stale_b".into(),
            task_id: "task_stale_b".into(),
            feature_id: contract_b.feature_id.clone(),
            contract_id: contract_b.id.clone(),
            attempt: 1,
            status: RunStatus::Running,
            mode_origin: ModeId::Cut,
            vcs: RunVcs {
                backend: VcsKind::Git,
                workspace_ref: "HEAD".into(),
                change_ref: "HEAD".into(),
                base_ref: None,
            },
            started_at: now_rfc3339(),
            ended_at: None,
        };
        let stale_dir = service.paths.runs_dir.join(&stale_run.id);
        fs::create_dir_all(&stale_dir).unwrap();
        write_json(&stale_dir.join("run.json"), &stale_run).unwrap();

        let (run_a2, _receipt_a2) = service.cut_run(&FakeExecutor, &contract_a.id).unwrap();

        let latest = service.inspect_work_ledger(None).unwrap();
        assert_eq!(latest.work_id, contract_a.feature_id);
        let expected_run_ref = format!(".punk/runs/{}/run.json", run_a2.id);
        assert_eq!(
            latest.latest_run_ref.as_deref(),
            Some(expected_run_ref.as_str())
        );

        let status = service.status(None).unwrap();
        assert_eq!(
            status.work_id.as_deref(),
            Some(contract_a.feature_id.as_str())
        );
        assert_eq!(status.last_run_id.as_deref(), Some(run_a2.id.as_str()));

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
    }

    #[test]
    fn inspect_work_ledger_ignores_stale_orphaned_running_run() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-ignore-stale-run-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let global = std::env::temp_dir().join(format!(
            "punk-orch-ignore-stale-run-global-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("demo.txt"), "seed\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let project = service.bootstrap_project().unwrap();
        let proposal = DraftProposal {
            title: "feature".into(),
            summary: "feature".into(),
            entry_points: vec!["demo.txt".into()],
            import_paths: vec![],
            expected_interfaces: vec!["demo".into()],
            behavior_requirements: vec!["update demo".into()],
            allowed_scope: vec!["demo.txt".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
        };
        let (_feature, contract) = service
            .persist_draft_contract(&project, "feature", &proposal)
            .unwrap();
        service.approve_contract(&contract.id).unwrap();
        let (good_run, _receipt) = service.cut_run(&FakeExecutor, &contract.id).unwrap();

        let stale_run = Run {
            id: "run_stale_orphan".into(),
            task_id: "task_stale_orphan".into(),
            feature_id: contract.feature_id.clone(),
            contract_id: contract.id.clone(),
            attempt: 2,
            status: RunStatus::Running,
            mode_origin: ModeId::Cut,
            vcs: good_run.vcs.clone(),
            started_at: now_rfc3339(),
            ended_at: None,
        };
        let stale_dir = service.paths.runs_dir.join(&stale_run.id);
        fs::create_dir_all(&stale_dir).unwrap();
        write_json(&stale_dir.join("run.json"), &stale_run).unwrap();
        write_json(
            &stale_dir.join("executor.json"),
            &serde_json::json!({"child_pid": 999999_u32, "process_group_id": 999999_u32}),
        )
        .unwrap();
        write_json(
            &stale_dir.join("heartbeat.json"),
            &RunHeartbeat {
                run_id: stale_run.id.clone(),
                state: "running".into(),
                last_progress_at: "2020-01-01T00:00:00Z".into(),
                stdout_bytes: 0,
                stderr_bytes: 0,
            },
        )
        .unwrap();

        let ledger = service
            .inspect_work_ledger(Some(&contract.feature_id))
            .unwrap();
        assert_eq!(
            ledger.latest_run_ref.as_deref(),
            Some(format!(".punk/runs/{}/run.json", good_run.id).as_str())
        );
        assert_eq!(ledger.lifecycle_state, "awaiting_gate");

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
    }

    #[test]
    fn gc_stale_dry_run_reports_safe_orphaned_runs() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-gc-stale-dry-run-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let global = std::env::temp_dir().join(format!(
            "punk-orch-gc-stale-dry-run-global-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("demo.txt"), "seed\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let project = service.bootstrap_project().unwrap();
        let proposal = DraftProposal {
            title: "feature".into(),
            summary: "feature".into(),
            entry_points: vec!["demo.txt".into()],
            import_paths: vec![],
            expected_interfaces: vec!["demo".into()],
            behavior_requirements: vec!["update demo".into()],
            allowed_scope: vec!["demo.txt".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
        };
        let (_feature, contract) = service
            .persist_draft_contract(&project, "feature", &proposal)
            .unwrap();
        service.approve_contract(&contract.id).unwrap();

        let stale_run = Run {
            id: "run_gc_stale".into(),
            task_id: "task_gc_stale".into(),
            feature_id: contract.feature_id.clone(),
            contract_id: contract.id.clone(),
            attempt: 1,
            status: RunStatus::Running,
            mode_origin: ModeId::Cut,
            vcs: RunVcs {
                backend: VcsKind::Git,
                workspace_ref: "HEAD".into(),
                change_ref: "HEAD".into(),
                base_ref: None,
            },
            started_at: now_rfc3339(),
            ended_at: None,
        };
        let stale_dir = service.paths.runs_dir.join(&stale_run.id);
        fs::create_dir_all(&stale_dir).unwrap();
        write_json(&stale_dir.join("run.json"), &stale_run).unwrap();
        write_json(
            &stale_dir.join("executor.json"),
            &serde_json::json!({"child_pid": 999999_u32, "process_group_id": 999999_u32}),
        )
        .unwrap();
        write_json(
            &stale_dir.join("heartbeat.json"),
            &RunHeartbeat {
                run_id: stale_run.id.clone(),
                state: "running".into(),
                last_progress_at: "2020-01-01T00:00:00Z".into(),
                stdout_bytes: 0,
                stderr_bytes: 0,
            },
        )
        .unwrap();

        let report = service.gc_stale_dry_run().unwrap();
        assert_eq!(report.project_id, project.id);
        assert_eq!(report.safe_to_archive.len(), 1);
        assert!(report.manual_review.is_empty());
        let candidate = &report.safe_to_archive[0];
        assert_eq!(candidate.artifact_id, stale_run.id);
        assert_eq!(candidate.work_id, contract.feature_id);
        assert!(candidate.reason.contains("child_pid 999999 is dead"));
        assert_eq!(
            candidate.artifact_ref,
            format!(".punk/runs/{}/run.json", stale_run.id)
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
    }

    #[test]
    fn orch_new_quarantines_safe_stale_runs_into_archive() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-auto-quarantine-stale-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let global = std::env::temp_dir().join(format!(
            "punk-orch-auto-quarantine-stale-global-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("demo.txt"), "seed\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let project = service.bootstrap_project().unwrap();
        let proposal = DraftProposal {
            title: "feature".into(),
            summary: "feature".into(),
            entry_points: vec!["demo.txt".into()],
            import_paths: vec![],
            expected_interfaces: vec!["demo".into()],
            behavior_requirements: vec!["update demo".into()],
            allowed_scope: vec!["demo.txt".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
        };
        let (_feature, contract) = service
            .persist_draft_contract(&project, "feature", &proposal)
            .unwrap();
        service.approve_contract(&contract.id).unwrap();

        let stale_run = Run {
            id: "run_auto_quarantine".into(),
            task_id: "task_auto_quarantine".into(),
            feature_id: contract.feature_id.clone(),
            contract_id: contract.id.clone(),
            attempt: 1,
            status: RunStatus::Running,
            mode_origin: ModeId::Cut,
            vcs: RunVcs {
                backend: VcsKind::Git,
                workspace_ref: "HEAD".into(),
                change_ref: "HEAD".into(),
                base_ref: None,
            },
            started_at: now_rfc3339(),
            ended_at: None,
        };
        let stale_dir = service.paths.runs_dir.join(&stale_run.id);
        fs::create_dir_all(&stale_dir).unwrap();
        write_json(&stale_dir.join("run.json"), &stale_run).unwrap();
        write_json(
            &stale_dir.join("executor.json"),
            &serde_json::json!({"child_pid": 999999_u32, "process_group_id": 999999_u32}),
        )
        .unwrap();
        write_json(
            &stale_dir.join("heartbeat.json"),
            &RunHeartbeat {
                run_id: stale_run.id.clone(),
                state: "running".into(),
                last_progress_at: "2020-01-01T00:00:00Z".into(),
                stdout_bytes: 0,
                stderr_bytes: 0,
            },
        )
        .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        assert!(!service.paths.runs_dir.join(&stale_run.id).exists());
        let archived_dir = root.join(".punk/archive/runs").join(&stale_run.id);
        assert!(archived_dir.exists());
        let quarantine: StaleArchiveRecord =
            read_json(&archived_dir.join("quarantine.json")).unwrap();
        assert_eq!(quarantine.run_id, stale_run.id);
        assert_eq!(quarantine.work_id, contract.feature_id);
        assert!(quarantine
            .reason
            .contains("status=running but child_pid 999999 is dead"));

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
    }

    #[test]
    fn autonomy_record_makes_recovery_durable_in_work_ledger() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-autonomy-ledger-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let global = std::env::temp_dir().join(format!(
            "punk-orch-autonomy-ledger-global-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let contract = service
            .draft_contract(&FakeDrafter, "recover blocked autonomy")
            .unwrap();
        service.approve_contract(&contract.id).unwrap();
        let (run, _receipt) = service.cut_run(&FakeExecutor, &contract.id).unwrap();

        let decision = punk_domain::DecisionObject {
            id: format!("dec_{}", run.id.trim_start_matches("run_")),
            run_id: run.id.clone(),
            contract_id: contract.id.clone(),
            decision: punk_domain::Decision::Block,
            deterministic_status: punk_domain::DeterministicStatus::Fail,
            target_status: punk_domain::CheckStatus::Fail,
            integrity_status: punk_domain::CheckStatus::Pass,
            confidence_estimate: 0.82,
            decision_basis: vec![
                "trace export still missing".into(),
                "manual recovery should stay bounded".into(),
            ],
            contract_ref: format!(".punk/contracts/{}/v1.json", run.feature_id),
            receipt_ref: format!(".punk/runs/{}/receipt.json", run.id),
            check_refs: Vec::new(),
            command_evidence: Vec::new(),
            declared_harness_evidence: Vec::new(),
            harness_evidence: Vec::new(),
            created_at: now_rfc3339(),
        };
        let decision_path = root
            .join(".punk/decisions")
            .join(format!("{}.json", decision.id));
        write_json(&decision_path, &decision).unwrap();
        let proof = punk_domain::Proofpack {
            id: format!("proof_{}", decision.id.trim_start_matches("dec_")),
            decision_id: decision.id.clone(),
            run_id: run.id.clone(),
            contract_ref: decision.contract_ref.clone(),
            receipt_ref: decision.receipt_ref.clone(),
            decision_ref: format!(".punk/decisions/{}.json", decision.id),
            check_refs: Vec::new(),
            command_evidence: Vec::new(),
            declared_harness_evidence: Vec::new(),
            harness_evidence: Vec::new(),
            hashes: Default::default(),
            summary: format!("proof for {}", decision.id),
            created_at: now_rfc3339(),
        };
        let proof_dir = root.join(".punk/proofs").join(&decision.id);
        fs::create_dir_all(&proof_dir).unwrap();
        let proof_path = proof_dir.join("proofpack.json");
        write_json(&proof_path, &proof).unwrap();

        let recovery_contract = service
            .draft_contract(&FakeDrafter, "recover blocked autonomy")
            .unwrap();
        let autonomy = service
            .record_autonomy_outcome(&proof.id, Some(&recovery_contract.id))
            .unwrap();
        let autonomy_ref = format!(".punk/autonomy/{}/{}.json", run.feature_id, autonomy.id);
        let recovery_ref = format!(".punk/contracts/{}/v1.json", recovery_contract.feature_id);

        let ledger = service.inspect_work_ledger(Some(&run.id)).unwrap();
        assert_eq!(
            ledger.latest_autonomy_ref.as_deref(),
            Some(autonomy_ref.as_str())
        );
        assert_eq!(ledger.autonomy_outcome.as_deref(), Some("blocked"));
        assert_eq!(
            ledger.recovery_contract_ref.as_deref(),
            Some(recovery_ref.as_str())
        );
        assert_eq!(ledger.lifecycle_state, "blocked_ready_for_recovery");
        assert_eq!(
            ledger.blocked_reason.as_deref(),
            Some("trace export still missing; manual recovery should stay bounded")
        );
        assert_eq!(ledger.next_action.as_deref(), Some("approve_contract"));
        assert_eq!(
            ledger.next_action_ref.as_deref(),
            Some(recovery_contract.id.as_str())
        );

        let status = service.status(Some(&run.id)).unwrap();
        assert_eq!(
            status.lifecycle_state.as_deref(),
            Some("blocked_ready_for_recovery")
        );
        assert_eq!(status.autonomy_outcome.as_deref(), Some("blocked"));
        assert_eq!(
            status.recovery_contract_ref.as_deref(),
            Some(recovery_ref.as_str())
        );
        assert_eq!(status.next_action.as_deref(), Some("approve_contract"));
        assert_eq!(
            status.next_action_ref.as_deref(),
            Some(recovery_contract.id.as_str())
        );
        let approve_command = format!("punk plot approve {}", recovery_contract.id);
        assert_eq!(
            status.suggested_command.as_deref(),
            Some(approve_command.as_str())
        );

        let autonomy_inspect = service.inspect(&autonomy.id).unwrap();
        assert_eq!(autonomy_inspect["id"].as_str(), Some(autonomy.id.as_str()));
        assert_eq!(
            autonomy_inspect["recovery_contract_ref"].as_str(),
            Some(recovery_ref.as_str())
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
    }

    #[test]
    fn bootstrap_project_refreshes_persisted_vcs_backend_after_jj_enable() {
        if std::process::Command::new("jj")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }

        let root = std::env::temp_dir().join(format!(
            "punk-orch-bootstrap-jj-refresh-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let global = std::env::temp_dir().join(format!(
            "punk-orch-bootstrap-jj-refresh-global-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".punk/\ntarget\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
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
                "initial",
            ])
            .current_dir(&root)
            .output()
            .unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let project = service.bootstrap_project().unwrap();
        assert_eq!(project.vcs_backend, Some(VcsKind::Git));

        std::process::Command::new("jj")
            .args(["git", "init", "--colocate", "."])
            .current_dir(&root)
            .output()
            .unwrap();

        let refreshed = service.bootstrap_project().unwrap();
        assert_eq!(refreshed.vcs_backend, Some(VcsKind::Jj));

        let persisted: Project = read_json(&service.paths.dot_punk.join("project.json")).unwrap();
        assert_eq!(persisted.vcs_backend, Some(VcsKind::Jj));

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
    }

    #[test]
    fn project_id_is_unique_for_distinct_paths_with_same_basename() {
        let base = std::env::temp_dir().join(format!(
            "punk-orch-project-id-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = base.join("work/api");
        let second = base.join("oss/api");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();

        let first_id = project_id(&first).unwrap();
        let second_id = project_id(&second).unwrap();

        assert_ne!(first_id, second_id);
        assert!(first_id.starts_with("api-"));
        assert!(second_id.starts_with("api-"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn bootstrap_project_refreshes_legacy_project_id() {
        let root = std::env::temp_dir().join(format!(
            "punk-orch-bootstrap-project-id-refresh-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk")).unwrap();
        let legacy = Project {
            id: "demo".into(),
            path: root.display().to_string(),
            vcs_backend: None,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/project.json"), &legacy).unwrap();

        let service = OrchService::new(&root, &global).unwrap();
        let refreshed = service.bootstrap_project().unwrap();

        assert_ne!(refreshed.id, "demo");
        assert!(refreshed
            .id
            .starts_with("punk-orch-bootstrap-project-id-refresh-"));

        let persisted: Project = read_json(&service.paths.dot_punk.join("project.json")).unwrap();
        assert_eq!(persisted.id, refreshed.id);

        let _ = fs::remove_dir_all(&root);
    }
}
