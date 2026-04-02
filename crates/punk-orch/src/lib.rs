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
    build_bounded_fallback_proposal, canonicalize_draft_proposal, scan_repo,
    validate_draft_proposal,
};
pub use punk_core::{find_object_path, read_json, relative_ref, write_json};
use punk_domain::{
    now_rfc3339, Contract, ContractStatus, DraftInput, DraftProposal, EventEnvelope, Feature,
    FeatureStatus, ModeId, Project, Receipt, ReceiptArtifacts, RefineInput, Run, RunStatus, Task,
    TaskKind, TaskStatus, VcsKind,
};
use punk_events::EventStore;
use punk_vcs::{current_snapshot_ref, detect_backend};
use serde::Serialize;

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
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusSnapshot {
    pub project_id: String,
    pub events_count: usize,
    pub last_contract_id: Option<String>,
    pub last_run_id: Option<String>,
    pub last_decision_id: Option<String>,
    pub vcs_backend: Option<VcsKind>,
    pub vcs_ref: Option<String>,
    pub vcs_dirty: bool,
    pub workspace_root: Option<String>,
}

pub struct OrchService {
    paths: RepoPaths,
    events: EventStore,
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
        let project = self.bootstrap_project()?;
        let scan = scan_repo(&self.paths.repo_root, prompt)?;
        if scan.candidate_integrity_checks.is_empty() {
            return Err(anyhow!(
                "unable to infer trustworthy integrity checks from repo scan"
            ));
        }
        let input = DraftInput {
            repo_root: self.paths.repo_root.display().to_string(),
            prompt: prompt.trim().to_string(),
            scan: scan.clone(),
        };
        let mut proposal = drafter.draft(input)?;
        canonicalize_draft_proposal(&self.paths.repo_root, prompt.trim(), &mut proposal);
        let mut errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
        if errors.is_empty() {
            if let Some(fallback) = build_bounded_fallback_proposal(
                &self.paths.repo_root,
                prompt.trim(),
                &proposal,
                &scan,
                &errors,
            ) {
                proposal = fallback;
                errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
            }
        }
        if !errors.is_empty() {
            proposal = drafter.refine(RefineInput {
                repo_root: self.paths.repo_root.display().to_string(),
                prompt: prompt.trim().to_string(),
                guidance: format_validation_guidance(&errors),
                current: proposal,
                scan: scan.clone(),
            })?;
            canonicalize_draft_proposal(&self.paths.repo_root, prompt.trim(), &mut proposal);
            errors = validate_draft_proposal(&self.paths.repo_root, &proposal);
            if errors.is_empty() {
                if let Some(fallback) = build_bounded_fallback_proposal(
                    &self.paths.repo_root,
                    prompt.trim(),
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
                prompt.trim(),
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
                "draft proposal invalid after repair: {}",
                format_validation_guidance(&errors)
            ));
        }
        let (feature, contract) = self.persist_draft_contract(&project, prompt, &proposal)?;
        let contract_dir = self.paths.contracts_dir.join(&feature.id);
        fs::create_dir_all(&contract_dir)?;
        let contract_path = contract_dir.join("v1.json");
        self.append_event(
            &project.id,
            Some(&feature.id),
            None,
            None,
            ModeId::Plot,
            "contract.drafted",
            Some(&contract_path),
        )?;
        Ok(contract)
    }

    pub fn refine_contract(
        &self,
        drafter: &dyn ContractDrafter,
        contract_id: &str,
        guidance: &str,
    ) -> Result<Contract> {
        if guidance.trim().is_empty() {
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
            guidance: guidance.trim().to_string(),
            current,
            scan: scan.clone(),
        })?;
        canonicalize_draft_proposal(
            &self.paths.repo_root,
            &current_contract.prompt_source,
            &mut proposal,
        );
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
        let provenance_baseline = backend.capture_provenance_baseline().ok();
        let execution = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            executor.execute_contract(ExecuteInput {
                repo_root: self.paths.repo_root.clone(),
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
                .and_then(|baseline| backend.changed_files_since(baseline).ok())
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
        if let Some(id) = id {
            let value = self.inspect(id)?;
            let last_run_id = value
                .get("run_id")
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned)
                .or_else(|| {
                    value
                        .get("id")
                        .and_then(|v| v.as_str())
                        .filter(|v| v.starts_with("run_"))
                        .map(ToOwned::to_owned)
                });
            let last_contract_id = value
                .get("contract_id")
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned)
                .or_else(|| {
                    value
                        .get("id")
                        .and_then(|v| v.as_str())
                        .filter(|v| v.starts_with("ct_"))
                        .map(ToOwned::to_owned)
                });
            let last_decision_id = value
                .get("decision_id")
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned)
                .or_else(|| {
                    value
                        .get("id")
                        .and_then(|v| v.as_str())
                        .filter(|v| v.starts_with("dec_"))
                        .map(ToOwned::to_owned)
                });
            return Ok(StatusSnapshot {
                project_id: project.id,
                events_count: events.len(),
                last_contract_id,
                last_run_id,
                last_decision_id,
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
            });
        }
        let mut last_contract_id = None;
        let mut last_run_id = None;
        let mut last_decision_id = None;
        for event in events.iter().filter(|event| event.project_id == project.id) {
            match event.kind.as_str() {
                "contract.drafted" | "contract.refined" | "contract.approved" => {
                    if let Some(path) = &event.payload_ref {
                        if let Ok(value) =
                            read_json::<serde_json::Value>(&self.paths.repo_root.join(path))
                        {
                            last_contract_id = value
                                .get("id")
                                .and_then(|v| v.as_str())
                                .map(ToOwned::to_owned);
                        }
                    }
                }
                "run.started" | "run.finished" => {
                    if let Some(path) = &event.payload_ref {
                        if let Ok(value) =
                            read_json::<serde_json::Value>(&self.paths.repo_root.join(path))
                        {
                            last_run_id = value
                                .get("id")
                                .and_then(|v| v.as_str())
                                .map(ToOwned::to_owned);
                        }
                    }
                }
                "decision.written" => {
                    if let Some(path) = &event.payload_ref {
                        if let Ok(value) =
                            read_json::<serde_json::Value>(&self.paths.repo_root.join(path))
                        {
                            last_decision_id = value
                                .get("id")
                                .and_then(|v| v.as_str())
                                .map(ToOwned::to_owned);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(StatusSnapshot {
            project_id: project.id,
            events_count: events.len(),
            last_contract_id,
            last_run_id,
            last_decision_id,
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
    root.file_name()
        .and_then(|v| v.to_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("unable to infer project id from repo root"))
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

    struct FakeExecutor;

    impl Executor for FakeExecutor {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn execute_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput> {
            fs::write(&input.repo_root.join("demo.txt"), b"ok")?;
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
            fs::write(&input.repo_root.join("carry.txt"), b"changed during run")?;
            fs::write(&input.repo_root.join("demo.txt"), b"ok")?;
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
}
