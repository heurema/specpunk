use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use chrono::Utc;
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
use serde::Serialize;
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

#[derive(Debug, Clone, Serialize)]
pub struct ProjectOverlay {
    pub project_id: String,
    pub repo_root: String,
    pub vcs_mode: String,
    pub bootstrap_ref: Option<String>,
    pub agent_guidance_ref: Vec<String>,
    pub capability_summary: ProjectCapabilitySummary,
    pub harness_summary: ProjectHarnessSummary,
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

impl OrchService {
    pub fn new(repo_root: impl AsRef<Path>, global_root: impl AsRef<Path>) -> Result<Self> {
        let repo_root = repo_root.as_ref().to_path_buf();
        let dot_punk = repo_root.join(".punk");
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
        };
        let events = EventStore::new(paths.global_root.clone());
        let service = Self { paths, events };
        service.bootstrap_project()?;
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
        let mut proposal = phase_error("drafter request", drafter.draft(input))?;
        canonicalize_draft_proposal(&self.paths.repo_root, trimmed_prompt, &mut proposal);
        let mut errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
        if errors.is_empty() {
            if let Some(fallback) = build_bounded_fallback_proposal(
                &self.paths.repo_root,
                trimmed_prompt,
                &proposal,
                &scan,
                &errors,
            ) {
                proposal = fallback;
                errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
            }
        }
        if !errors.is_empty() {
            proposal = phase_error(
                "drafter repair",
                drafter.refine(RefineInput {
                    repo_root: self.paths.repo_root.display().to_string(),
                    prompt: trimmed_prompt.to_string(),
                    guidance: format_validation_guidance(&errors),
                    current: proposal,
                    scan: scan.clone(),
                }),
            )?;
            canonicalize_draft_proposal(&self.paths.repo_root, trimmed_prompt, &mut proposal);
            errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
            if errors.is_empty() {
                if let Some(fallback) = build_bounded_fallback_proposal(
                    &self.paths.repo_root,
                    trimmed_prompt,
                    &proposal,
                    &scan,
                    &errors,
                ) {
                    proposal = fallback;
                    errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
                }
            }
        }
        if !errors.is_empty() {
            if let Some(fallback) = build_bounded_fallback_proposal(
                &self.paths.repo_root,
                trimmed_prompt,
                &proposal,
                &scan,
                &errors,
            ) {
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
        let scan = scan_repo(&self.paths.repo_root, &current_contract.prompt_source)?;
        if scan.candidate_integrity_checks.is_empty() {
            return Err(anyhow!(
                "unable to infer trustworthy integrity checks from repo scan"
            ));
        }
        let current = contract_to_proposal(&feature, &current_contract);
        let mut proposal = drafter.refine(RefineInput {
            repo_root: self.paths.repo_root.display().to_string(),
            prompt: current_contract.prompt_source.clone(),
            guidance: guidance.to_string(),
            current,
            scan: scan.clone(),
        })?;
        canonicalize_draft_proposal(
            &self.paths.repo_root,
            &current_contract.prompt_source,
            &mut proposal,
        );
        apply_explicit_prompt_overrides(&self.paths.repo_root, guidance, &mut proposal);
        let mut errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
        if errors.is_empty() {
            if let Some(fallback) = build_bounded_fallback_proposal(
                &self.paths.repo_root,
                &current_contract.prompt_source,
                &proposal,
                &scan,
                &errors,
            ) {
                proposal = fallback;
                apply_explicit_prompt_overrides(&self.paths.repo_root, guidance, &mut proposal);
                errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
            }
        }
        if !errors.is_empty() {
            if let Some(fallback) = build_bounded_fallback_proposal(
                &self.paths.repo_root,
                &current_contract.prompt_source,
                &proposal,
                &scan,
                &errors,
            ) {
                proposal = fallback;
                apply_explicit_prompt_overrides(&self.paths.repo_root, guidance, &mut proposal);
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
        let isolated = backend.create_isolated_change(&task.id)?;
        let workspace_root = PathBuf::from(&isolated.workspace_ref);
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

        let (status, summary, checks_run, cost_usd, duration_ms) = match execution {
            Ok(output) => {
                run.status = if output.success {
                    RunStatus::Finished
                } else {
                    RunStatus::Failed
                };
                (
                    if output.success { "success" } else { "failure" }.to_string(),
                    output.summary,
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
            changed_files: provenance_baseline
                .as_ref()
                .and_then(|baseline| isolated_backend.changed_files_since(baseline).ok())
                .unwrap_or_default(),
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

    pub fn inspect_work_ledger(&self, id: Option<&str>) -> Result<WorkLedgerView> {
        let project = self.bootstrap_project()?;
        let feature_id = match id {
            Some(id) => self.resolve_feature_id_for_work(id)?,
            None => self.latest_feature_id_for_project(&project.id)?,
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

        Ok(ProjectOverlay {
            project_id: project.id.clone(),
            repo_root: project.path,
            vcs_mode,
            bootstrap_ref,
            agent_guidance_ref,
            capability_summary,
            harness_summary,
            project_skill_refs,
            local_constraints,
            safe_default_checks,
            status_scope_mode: format!("project:{}", project.id),
            updated_at: project.updated_at,
        })
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
        let latest_run = runs.into_iter().max_by(|left, right| {
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

    fn latest_feature_id_for_project(&self, project_id: &str) -> Result<String> {
        let mut latest: Option<Feature> = None;
        for entry in fs::read_dir(&self.paths.features_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let feature: Feature = read_json(&path)?;
            if feature.project_id != project_id {
                continue;
            }
            let replace = latest
                .as_ref()
                .map(|current| {
                    feature
                        .updated_at
                        .cmp(&current.updated_at)
                        .then_with(|| feature.created_at.cmp(&current.created_at))
                        .is_gt()
                })
                .unwrap_or(true);
            if replace {
                latest = Some(feature);
            }
        }

        latest
            .map(|feature| feature.id)
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

fn summarize_prompt(prompt: &str) -> String {
    let trimmed = prompt.trim();
    trimmed.chars().take(60).collect()
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

#[derive(Debug, Clone, Serialize)]
struct RunHeartbeat {
    run_id: String,
    state: String,
    last_progress_at: String,
    stdout_bytes: u64,
    stderr_bytes: u64,
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
    use punk_domain::{DraftInput, RefineInput};
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

    struct PanicExecutor;

    impl Executor for PanicExecutor {
        fn name(&self) -> &'static str {
            "panic"
        }

        fn execute_contract(&self, _input: ExecuteInput) -> Result<ExecuteOutput> {
            panic!("executor interrupted");
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
            .draft_contract(&ScanTargetChecksDrafter, "tighten run reporting in punk-orch")
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
