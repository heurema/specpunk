mod context_pack;
pub mod council;

use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use context_pack::{
    build_context_pack, derive_plan_seed, ensure_retry_patch_seed, format_context_pack,
    format_patch_context_pack, format_plan_context_pack, hydrate_plan_seed_excerpts,
    materialize_missing_entry_points, restore_missing_materialized_entry_points,
    restore_stale_entry_point_masks, scaffold_only_entry_points, ContextPack, ContextPlanSeed,
    ContextPlanTarget, EntryPointExcerptGuard,
};
use punk_domain::{Contract, DraftInput, DraftProposal, RefineInput};
const BLOCKED_EXECUTION_SENTINEL: &str = "PUNK_EXECUTION_BLOCKED:";
const SUCCESSFUL_EXECUTION_SENTINEL: &str = "PUNK_EXECUTION_COMPLETE:";

pub struct ExecuteInput {
    pub repo_root: PathBuf,
    pub contract: Contract,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub executor_pid_path: PathBuf,
}

pub struct ExecuteOutput {
    pub success: bool,
    pub summary: String,
    pub checks_run: Vec<String>,
    pub cost_usd: Option<f64>,
    pub duration_ms: u64,
}

struct TimedOutput {
    output: std::process::Output,
    timed_out: bool,
    stalled: bool,
    orphaned: bool,
    no_progress_paths: Vec<String>,
    scaffold_only_paths: Vec<String>,
    post_check_zero_progress_paths: Vec<String>,
}

struct PatchLaneTimedOutput {
    output: std::process::Output,
    timed_out: bool,
    orphaned: bool,
    response: Option<PatchLaneResponse>,
}

struct PlanLaneTimedOutput {
    output: std::process::Output,
    timed_out: bool,
    orphaned: bool,
    response: Option<PlanPrepassResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryPointSnapshot {
    path: String,
    content: Option<String>,
}

struct AttemptOutcome {
    timed_output: TimedOutput,
    restored_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionLane {
    Exec,
    PatchApply,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PatchLaneResponse {
    Patch(String),
    Blocked(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanPrepassTarget {
    path: String,
    symbol: String,
    insertion_point: String,
    execution_sketch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlanPrepassResponse {
    Plan {
        summary: String,
        targets: Vec<PlanPrepassTarget>,
    },
    Blocked(String),
}

struct ProgressTracker {
    stdout_bytes: AtomicU64,
    stderr_bytes: AtomicU64,
    last_progress: Mutex<Instant>,
}

struct GitGuardEnv {
    dir: PathBuf,
    path: OsString,
    zdotdir: PathBuf,
}

struct OrientationGuardEnv {
    dir: PathBuf,
    path: OsString,
    zdotdir: PathBuf,
}

pub trait Executor {
    fn name(&self) -> &'static str;
    fn execute_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput>;
}

pub trait ContractDrafter {
    fn name(&self) -> &'static str;
    fn draft(&self, input: DraftInput) -> Result<DraftProposal>;
    fn refine(&self, input: RefineInput) -> Result<DraftProposal>;
}

pub struct CodexCliExecutor {
    pub model: Option<String>,
}

impl Default for CodexCliExecutor {
    fn default() -> Self {
        Self { model: None }
    }
}

impl GitGuardEnv {
    fn install() -> Result<Option<Self>> {
        let Some(real_git) = find_binary_in_path("git") else {
            return Ok(None);
        };

        let dir = std::env::temp_dir().join(format!(
            "punk-git-guard-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&dir)?;
        let wrapper_path = dir.join("git");
        let wrapper = format!(
            "#!/bin/sh\ncase \"$1\" in\n  checkout|restore|reset|clean|switch)\n    printf '%s\\n' \"{blocked} forbidden vcs restore/reset command: git $*\" >&2\n    exit 97\n    ;;\nesac\nexec {real_git} \"$@\"\n",
            blocked = BLOCKED_EXECUTION_SENTINEL,
            real_git = sh_single_quote(&real_git.to_string_lossy()),
        );
        fs::write(&wrapper_path, wrapper)?;
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&wrapper_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&wrapper_path, perms)?;
        }

        let mut paths = vec![dir.clone()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let path = std::env::join_paths(paths)?;
        let zdotdir = dir.join("zdotdir");
        fs::create_dir_all(&zdotdir)?;
        fs::write(
            zdotdir.join(".zshenv"),
            format!(
                "git() {{\n  case \"$1\" in\n    checkout|restore|reset|clean|switch)\n      print -r -- \"{blocked} forbidden vcs restore/reset command: git $*\" >&2\n      return 97\n      ;;\n  esac\n  {real_git} \"$@\"\n}}\nexport PATH={wrapper_dir}:$PATH\n",
                blocked = BLOCKED_EXECUTION_SENTINEL,
                real_git = sh_single_quote(&real_git.to_string_lossy()),
                wrapper_dir = sh_single_quote(&dir.to_string_lossy()),
            ),
        )?;
        Ok(Some(Self { dir, path, zdotdir }))
    }

    fn apply(&self, command: &mut Command) {
        command.env("PATH", &self.path);
        command.env("ZDOTDIR", &self.zdotdir);
    }
}

impl Drop for GitGuardEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

const ORIENTATION_BLOCKED_COMMANDS: &[&str] = &[
    "rg", "grep", "sed", "cat", "awk", "find", "fd", "ls", "head", "tail", "tree", "bat", "less",
    "more", "perl", "python", "python3", "ruby", "git", "bash", "sh", "zsh",
];

impl OrientationGuardEnv {
    fn install() -> Result<Self> {
        let dir = std::env::temp_dir().join(format!(
            "punk-orientation-guard-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&dir)?;
        for command_name in ORIENTATION_BLOCKED_COMMANDS {
            let wrapper_path = dir.join(command_name);
            let wrapper = format!(
                "#!/bin/sh\nprintf '%s\\n' \"{blocked} shell orientation forbidden in patch/apply lane: {command_name} $*\" >&2\nexit 97\n",
                blocked = BLOCKED_EXECUTION_SENTINEL,
                command_name = command_name,
            );
            fs::write(&wrapper_path, wrapper)?;
            #[cfg(unix)]
            {
                let mut perms = fs::metadata(&wrapper_path)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&wrapper_path, perms)?;
            }
        }

        let mut paths = vec![dir.clone()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let path = std::env::join_paths(paths)?;
        let zdotdir = dir.join("zdotdir");
        fs::create_dir_all(&zdotdir)?;
        let mut zshenv = String::new();
        zshenv.push_str(&format!(
            "export PATH={}:$PATH\n",
            sh_single_quote(&dir.to_string_lossy())
        ));
        for command_name in ORIENTATION_BLOCKED_COMMANDS {
            zshenv.push_str(&format!(
                "{name}() {{\n  print -r -- \"{blocked} shell orientation forbidden in patch/apply lane: {name} $*\" >&2\n  return 97\n}}\n",
                name = command_name,
                blocked = BLOCKED_EXECUTION_SENTINEL,
            ));
        }
        fs::write(zdotdir.join(".zshenv"), zshenv)?;
        Ok(Self { dir, path, zdotdir })
    }

    fn apply(&self, command: &mut Command) {
        command.env("PATH", &self.path);
        command.env("ZDOTDIR", &self.zdotdir);
    }
}

impl Drop for OrientationGuardEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

impl Executor for CodexCliExecutor {
    fn name(&self) -> &'static str {
        "codex-cli"
    }

    fn execute_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput> {
        match execution_lane_for_contract(&input.repo_root, &input.contract) {
            ExecutionLane::Manual => {
                let summary = manual_mode_block_summary(&input.contract)
                    .unwrap_or_else(|| "PUNK_EXECUTION_BLOCKED: manual lane required".to_string());
                return Ok(ExecuteOutput {
                    success: false,
                    summary,
                    checks_run: Vec::new(),
                    cost_usd: None,
                    duration_ms: 0,
                });
            }
            ExecutionLane::PatchApply => {
                return self.execute_patch_apply_contract(input);
            }
            ExecutionLane::Exec => {}
        }
        let start = Instant::now();
        let executor_timeout = effective_codex_executor_timeout(&input.contract);
        restore_stale_entry_point_masks(&input.repo_root)?;
        let created_entry_points = if is_fail_closed_scope_task(&input.contract) {
            materialize_missing_entry_points(&input.repo_root, &input.contract)?
        } else {
            Vec::new()
        };
        let created_bootstrap_scaffold =
            materialize_rust_workspace_bootstrap_scaffold(&input.repo_root, &input.contract)?;
        let bootstrap_scaffold_paths = controller_bootstrap_scaffold_paths(&input.contract);
        let mut created_scaffold_paths = created_entry_points.clone();
        extend_unique_paths(&mut created_scaffold_paths, &created_bootstrap_scaffold);
        let mut attempt = self.run_execution_attempt(&input, &created_scaffold_paths, false)?;
        if !attempt.restored_paths.is_empty() {
            return Ok(ExecuteOutput {
                success: false,
                summary: format!(
                    "materialized entry-point files were deleted during execution: {}",
                    attempt.restored_paths.join(", ")
                ),
                checks_run: Vec::new(),
                cost_usd: None,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }
        if !attempt.timed_output.no_progress_paths.is_empty()
            && is_fail_closed_scope_task(&input.contract)
        {
            let stdout = String::from_utf8_lossy(&attempt.timed_output.output.stdout);
            let stderr = String::from_utf8_lossy(&attempt.timed_output.output.stderr);
            if should_retry_after_no_progress(
                &input.contract,
                &attempt.timed_output.no_progress_paths,
                &stdout,
                &stderr,
            ) {
                attempt = self.run_execution_attempt(&input, &created_scaffold_paths, true)?;
                if !attempt.restored_paths.is_empty() {
                    return Ok(ExecuteOutput {
                        success: false,
                        summary: format!(
                            "materialized entry-point files were deleted during execution: {}",
                            attempt.restored_paths.join(", ")
                        ),
                        checks_run: Vec::new(),
                        cost_usd: None,
                        duration_ms: start.elapsed().as_millis() as u64,
                    });
                }
            }
        }
        let timed_output = attempt.timed_output;
        fs::write(&input.stdout_path, &timed_output.output.stdout)?;
        fs::write(&input.stderr_path, &timed_output.output.stderr)?;
        if !bootstrap_scaffold_paths.is_empty()
            && no_progress_only_in_controller_scaffold(
                &timed_output.no_progress_paths,
                &bootstrap_scaffold_paths,
            )
        {
            let checks = merged_contract_checks(&input.contract);
            match run_contract_checks(
                &input.repo_root,
                &input.contract,
                &checks,
                &input.stdout_path,
                &input.stderr_path,
            ) {
                Ok(checks_run) => {
                    return Ok(ExecuteOutput {
                        success: true,
                        summary: "PUNK_EXECUTION_COMPLETE: controller bootstrap scaffold created and checks passed".to_string(),
                        checks_run,
                        cost_usd: None,
                        duration_ms: start.elapsed().as_millis() as u64,
                    });
                }
                Err(err) => {
                    return Ok(ExecuteOutput {
                        success: false,
                        summary: format!(
                            "controller bootstrap scaffold created but verification failed: {err}"
                        ),
                        checks_run: Vec::new(),
                        cost_usd: None,
                        duration_ms: start.elapsed().as_millis() as u64,
                    });
                }
            }
        }
        let stdout = String::from_utf8_lossy(&timed_output.output.stdout);
        let stderr = String::from_utf8_lossy(&timed_output.output.stderr);
        let (success, summary) = if !timed_output.no_progress_paths.is_empty() {
            classify_no_progress_after_dispatch_result(
                &timed_output.no_progress_paths,
                &stdout,
                &stderr,
            )
        } else if !timed_output.scaffold_only_paths.is_empty() {
            classify_scaffold_only_result(&timed_output.scaffold_only_paths, &stdout, &stderr)
        } else if !timed_output.post_check_zero_progress_paths.is_empty() {
            classify_post_check_zero_progress_result(
                &timed_output.post_check_zero_progress_paths,
                &stdout,
                &stderr,
            )
        } else if let Some(blocked) =
            greenfield_manifest_blocked_summary(&input.repo_root, &input.contract)
        {
            (false, blocked)
        } else if timed_output.timed_out {
            classify_timeout_result(&stdout, &stderr, executor_timeout)
        } else if timed_output.orphaned {
            classify_orphan_result(&stdout, &stderr)
        } else if timed_output.stalled {
            classify_stall_result(&stdout, &stderr, codex_executor_stall_timeout())
        } else {
            classify_execution_result(timed_output.output.status.success(), &stdout, &stderr)
        };
        Ok(ExecuteOutput {
            success,
            summary,
            checks_run: Vec::new(),
            cost_usd: None,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}

#[derive(Default)]
pub struct CodexCliContractDrafter {
    pub model: Option<String>,
}

impl ContractDrafter for CodexCliContractDrafter {
    fn name(&self) -> &'static str {
        "codex-cli-drafter"
    }

    fn draft(&self, input: DraftInput) -> Result<DraftProposal> {
        self.run_json_prompt(
            &build_draft_prompt(&input),
            Some(build_compact_draft_prompt(&input)),
            &input.repo_root,
        )
    }

    fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
        self.run_json_prompt(
            &build_refine_prompt(&input),
            Some(build_compact_refine_prompt(&input)),
            &input.repo_root,
        )
    }
}

enum DrafterAttemptError {
    TimedOut { stdout: String, stderr: String },
    Failed(anyhow::Error),
}

impl CodexCliContractDrafter {
    fn run_json_prompt(
        &self,
        prompt: &str,
        retry_prompt: Option<String>,
        repo_root: &str,
    ) -> Result<DraftProposal> {
        let schema_path = draft_schema_path()?;
        let output_path = draft_output_path()?;
        let _ = fs::remove_file(&schema_path);
        let _ = fs::remove_file(&output_path);
        fs::write(&schema_path, serde_json::to_vec_pretty(&draft_schema())?)?;

        let total_timeout = codex_drafter_timeout();
        let (primary_timeout, retry_timeout) =
            drafter_attempt_timeouts(total_timeout, retry_prompt.is_some());
        let result = match self.run_json_prompt_once(
            prompt,
            repo_root,
            &schema_path,
            &output_path,
            primary_timeout,
        ) {
            Ok(proposal) => Ok(proposal),
            Err(DrafterAttemptError::TimedOut { stdout, stderr }) => {
                if should_retry_drafter_timeout(&stdout, &stderr) {
                    if let (Some(retry_prompt), Some(retry_timeout)) = (retry_prompt, retry_timeout)
                    {
                        match self.run_json_prompt_once(
                            &retry_prompt,
                            repo_root,
                            &schema_path,
                            &output_path,
                            retry_timeout,
                        ) {
                            Ok(proposal) => Ok(proposal),
                            Err(DrafterAttemptError::TimedOut { stdout, stderr }) => {
                                Err(anyhow!(timeout_summary(total_timeout, &stdout, &stderr)))
                            }
                            Err(DrafterAttemptError::Failed(err)) => Err(err),
                        }
                    } else {
                        Err(anyhow!(timeout_summary(total_timeout, &stdout, &stderr)))
                    }
                } else {
                    Err(anyhow!(timeout_summary(total_timeout, &stdout, &stderr)))
                }
            }
            Err(DrafterAttemptError::Failed(err)) => Err(err),
        };

        let _ = fs::remove_file(&schema_path);
        let _ = fs::remove_file(&output_path);
        result
    }

    fn run_json_prompt_once(
        &self,
        prompt: &str,
        repo_root: &str,
        schema_path: &std::path::Path,
        output_path: &std::path::Path,
        timeout: Duration,
    ) -> std::result::Result<DraftProposal, DrafterAttemptError> {
        let _ = fs::remove_file(output_path);
        let mut command = Command::new("codex");
        command
            .arg("exec")
            .arg("--full-auto")
            .arg("--ephemeral")
            .arg("-s")
            .arg("read-only")
            .arg("-C")
            .arg(repo_root)
            .arg("-c")
            .arg("model_reasoning_effort=\"low\"")
            .arg("--output-schema")
            .arg(&schema_path)
            .arg("-o")
            .arg(&output_path);
        if let Some(model) = &self.model {
            command.arg("-m").arg(model);
        }
        command.arg("--").arg(prompt);

        let output = run_command_with_timeout(&mut command, timeout).map_err(|err| {
            DrafterAttemptError::Failed(err.context(format!("spawn codex drafter in {repo_root}")))
        })?;
        let stdout = String::from_utf8_lossy(&output.output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.output.stderr).to_string();
        if output.timed_out {
            return Err(DrafterAttemptError::TimedOut { stdout, stderr });
        }
        if !output.output.status.success() {
            return Err(DrafterAttemptError::Failed(anyhow!(
                "codex drafter failed: {}",
                last_non_empty_line(&stderr)
                    .or_else(|| last_non_empty_line(&stdout))
                    .unwrap_or_else(|| "unknown error".to_string())
            )));
        }

        let payload = if output_path.exists() {
            fs::read_to_string(output_path)
                .map_err(|err| DrafterAttemptError::Failed(err.into()))?
        } else {
            last_non_empty_line(&stdout).ok_or_else(|| {
                DrafterAttemptError::Failed(anyhow!("codex drafter returned no JSON"))
            })?
        };
        let proposal = serde_json::from_str::<DraftProposal>(&payload)
            .with_context(|| format!("parse draft proposal JSON: {payload}"))
            .map_err(DrafterAttemptError::Failed)?;
        Ok(proposal)
    }
}

fn should_retry_drafter_timeout(stdout: &str, stderr: &str) -> bool {
    [stderr, stdout]
        .into_iter()
        .any(|stream| looks_like_partial_draft_json(stream))
}

fn looks_like_partial_draft_json(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "\"title\"",
        "\"summary\"",
        "\"entry_points\"",
        "\"expected_interfaces\"",
        "\"behavior_requirements\"",
        "\"allowed_scope\"",
        "\"target_checks\"",
        "\"integrity_checks\"",
    ]
    .into_iter()
    .any(|needle| lower.contains(needle))
}

impl CodexCliExecutor {
    fn execute_patch_apply_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput> {
        let start = Instant::now();
        restore_stale_entry_point_masks(&input.repo_root)?;
        let mut context_pack = build_context_pack(&input.repo_root, &input.contract)?;
        let mut controller_plan_seed = derive_plan_seed(&input.contract, &context_pack);
        hydrate_plan_seed_excerpts(&context_pack, &mut controller_plan_seed);
        if !controller_plan_seed.targets.is_empty() {
            context_pack.plan_seed = Some(controller_plan_seed.clone());
        }
        let mut excerpt_guard = EntryPointExcerptGuard::apply(&input.repo_root, &context_pack)?;
        if needs_patch_plan_prepass(&input.contract, &context_pack) {
            match self.run_patch_plan_prepass(&input, &context_pack) {
                Err(err) => {
                    if let Some(guard) = excerpt_guard.as_mut() {
                        guard.restore()?;
                    }
                    append_log_text(
                        &input.stderr_path,
                        &format!("\n[punk patch/prepass] failed: {err}\n"),
                    )?;
                    return Ok(ExecuteOutput {
                        success: false,
                        summary: format!("patch prepass failed: {err}"),
                        checks_run: Vec::new(),
                        cost_usd: None,
                        duration_ms: start.elapsed().as_millis() as u64,
                    });
                }
                Ok(PlanPrepassResponse::Blocked(reason)) => {
                    if let Some(guard) = excerpt_guard.as_mut() {
                        guard.restore()?;
                    }
                    append_log_text(
                        &input.stderr_path,
                        &format!("\n[punk patch/prepass] blocked: {reason}\n"),
                    )?;
                    return Ok(ExecuteOutput {
                        success: false,
                        summary: reason,
                        checks_run: Vec::new(),
                        cost_usd: None,
                        duration_ms: start.elapsed().as_millis() as u64,
                    });
                }
                Ok(PlanPrepassResponse::Plan { summary, targets }) => {
                    append_log_text(
                        &input.stdout_path,
                        &format!(
                            "\n[punk patch/prepass] planned targets: {}\n",
                            targets
                                .iter()
                                .map(|target| format!(
                                    "{}:{}@{}",
                                    target.path, target.symbol, target.insertion_point
                                ))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    )?;
                    let mut plan_seed = if controller_plan_seed.targets.is_empty() {
                        ContextPlanSeed {
                            title: "Controller-owned plan prepass".to_string(),
                            summary,
                            targets: targets
                                .into_iter()
                                .map(|target| ContextPlanTarget {
                                    path: target.path,
                                    symbol: target.symbol,
                                    insertion_point: target.insertion_point,
                                    execution_sketch: target.execution_sketch,
                                    anchor_excerpt: String::new(),
                                })
                                .collect(),
                        }
                    } else {
                        let mut seed = controller_plan_seed.clone();
                        seed.title = "Controller-owned plan prepass".to_string();
                        seed.summary = summary;
                        seed
                    };
                    hydrate_plan_seed_excerpts(&context_pack, &mut plan_seed);
                    context_pack.plan_seed = Some(plan_seed);
                }
            }
        }
        let prompt = build_patch_apply_prompt(&input.contract, &context_pack);

        let mut command = Command::new("codex");
        command
            .arg("exec")
            .arg("--full-auto")
            .arg("--ephemeral")
            .arg("-s")
            .arg("read-only")
            .arg("-C")
            .arg(&input.repo_root);
        if let Some(model) = &self.model {
            command.arg("-m").arg(model);
        }
        if let Some(reasoning_effort) = codex_executor_reasoning_effort(&input.contract) {
            command
                .arg("-c")
                .arg(format!("model_reasoning_effort=\"{reasoning_effort}\""));
        }
        let orientation_guard = OrientationGuardEnv::install()?;
        orientation_guard.apply(&mut command);
        command.arg("--").arg(prompt);

        let timed_output = match run_patch_lane_command_with_timeout(
            &mut command,
            codex_patch_lane_timeout(),
            input.stdout_path.clone(),
            input.stderr_path.clone(),
            input.executor_pid_path.clone(),
        ) {
            Ok(output) => {
                if let Some(guard) = excerpt_guard.as_mut() {
                    guard.restore()?;
                }
                output
            }
            Err(err) => {
                if let Some(guard) = excerpt_guard.as_mut() {
                    let _ = guard.restore();
                }
                return Err(err);
            }
        };
        let stdout = String::from_utf8_lossy(&timed_output.output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&timed_output.output.stderr).to_string();
        let response = timed_output
            .response
            .clone()
            .or_else(|| load_patch_lane_response(&stdout, &stderr).ok());
        if timed_output.timed_out && response.is_none() {
            return Ok(ExecuteOutput {
                success: false,
                summary: timeout_summary(codex_patch_lane_timeout(), &stdout, &stderr),
                checks_run: Vec::new(),
                cost_usd: None,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }
        if timed_output.orphaned {
            return Ok(ExecuteOutput {
                success: false,
                summary: orphan_summary(&stdout, &stderr),
                checks_run: Vec::new(),
                cost_usd: None,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }
        if !timed_output.output.status.success() && response.is_none() {
            let (success, summary) = classify_execution_result(false, &stdout, &stderr);
            return Ok(ExecuteOutput {
                success,
                summary,
                checks_run: Vec::new(),
                cost_usd: None,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        let response = match response {
            Some(response) => response,
            None => {
                let err = anyhow!("patch lane returned no complete patch artifact");
                append_log_text(
                    &input.stderr_path,
                    &format!("\n[punk patch/apply] failed to parse patch output: {err}\n"),
                )?;
                return Ok(ExecuteOutput {
                    success: false,
                    summary: format!("patch/apply lane returned invalid patch text: {err}"),
                    checks_run: Vec::new(),
                    cost_usd: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                });
            }
        };
        let patch = match response {
            PatchLaneResponse::Blocked(reason) => {
                append_log_text(
                    &input.stderr_path,
                    &format!("\n[punk patch/apply] blocked: {reason}\n"),
                )?;
                return Ok(ExecuteOutput {
                    success: false,
                    summary: reason,
                    checks_run: Vec::new(),
                    cost_usd: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                });
            }
            PatchLaneResponse::Patch(patch) => patch,
        };

        let updates = match validate_patch_scope(&patch, &input.contract.allowed_scope) {
            Ok(updates) => updates,
            Err(err) => {
                append_log_text(
                    &input.stderr_path,
                    &format!("\n[punk patch/apply] invalid patch scope: {err}\n"),
                )?;
                return Ok(ExecuteOutput {
                    success: false,
                    summary: format!("patch/apply lane rejected patch: {err}"),
                    checks_run: Vec::new(),
                    cost_usd: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                });
            }
        };
        let patch_paths = updates
            .iter()
            .map(|update| update.path.clone())
            .collect::<Vec<_>>();

        if let Err(err) = apply_patch_in_repo(&input.repo_root, &updates) {
            append_log_text(
                &input.stderr_path,
                &format!("\n[punk patch/apply] failed to apply patch: {err}\n"),
            )?;
            return Ok(ExecuteOutput {
                success: false,
                summary: format!("patch/apply lane failed to apply patch: {err}"),
                checks_run: Vec::new(),
                cost_usd: None,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }
        append_log_text(
            &input.stdout_path,
            &format!(
                "\n[punk patch/apply] applied patch for: {}\n",
                patch_paths.join(", ")
            ),
        )?;

        let checks = collect_contract_checks(&input.contract);
        let checks_run = match run_contract_checks(
            &input.repo_root,
            &input.contract,
            &checks,
            &input.stdout_path,
            &input.stderr_path,
        ) {
            Ok(checks_run) => checks_run,
            Err(summary) => {
                return Ok(ExecuteOutput {
                    success: false,
                    summary,
                    checks_run: Vec::new(),
                    cost_usd: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                });
            }
        };

        Ok(ExecuteOutput {
            success: true,
            summary: format!(
                "{SUCCESSFUL_EXECUTION_SENTINEL} patch/apply lane succeeded after applying patch for {}",
                patch_paths.join(", ")
            ),
            checks_run,
            cost_usd: None,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    fn run_patch_plan_prepass(
        &self,
        input: &ExecuteInput,
        context_pack: &ContextPack,
    ) -> Result<PlanPrepassResponse> {
        let mut prepass_context = context_pack.clone();
        prepass_context.plan_seed = Some(derive_plan_seed(&input.contract, context_pack));
        let prompt = build_patch_plan_prompt(&input.contract, &prepass_context);
        let mut command = Command::new("codex");
        command
            .arg("exec")
            .arg("--full-auto")
            .arg("--ephemeral")
            .arg("-s")
            .arg("read-only")
            .arg("-C")
            .arg(&input.repo_root);
        if let Some(model) = &self.model {
            command.arg("-m").arg(model);
        }
        if let Some(reasoning_effort) = codex_executor_reasoning_effort(&input.contract) {
            command
                .arg("-c")
                .arg(format!("model_reasoning_effort=\"{reasoning_effort}\""));
        }
        let orientation_guard = OrientationGuardEnv::install()?;
        orientation_guard.apply(&mut command);
        command.arg("--").arg(prompt);

        let timed_output = run_plan_lane_command_with_timeout(
            &mut command,
            codex_plan_prepass_timeout(),
            input.stdout_path.clone(),
            input.stderr_path.clone(),
            input.executor_pid_path.clone(),
        )?;
        let stdout = String::from_utf8_lossy(&timed_output.output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&timed_output.output.stderr).to_string();
        let response = timed_output
            .response
            .clone()
            .or_else(|| load_plan_prepass_response(&stdout, &stderr).ok());
        if timed_output.timed_out && response.is_none() {
            return Err(anyhow!(
                "patch prepass timed out: {}",
                timeout_summary(codex_plan_prepass_timeout(), &stdout, &stderr)
            ));
        }
        if timed_output.orphaned {
            return Err(anyhow!(
                "patch prepass orphaned: {}",
                orphan_summary(&stdout, &stderr)
            ));
        }
        if !timed_output.output.status.success() && response.is_none() {
            return Err(anyhow!(
                "patch prepass failed: {}",
                classify_execution_result(false, &stdout, &stderr).1
            ));
        }
        let response =
            response.ok_or_else(|| anyhow!("patch prepass returned no complete plan artifact"))?;
        validate_plan_prepass_scope(&response, &input.contract.allowed_scope)?;
        Ok(response)
    }

    fn run_execution_attempt(
        &self,
        input: &ExecuteInput,
        created_scaffold_paths: &[String],
        retry_mode: bool,
    ) -> Result<AttemptOutcome> {
        let context_pack = if is_fail_closed_scope_task(&input.contract) {
            let mut pack = build_context_pack(&input.repo_root, &input.contract)?;
            if retry_mode {
                ensure_retry_patch_seed(&input.repo_root, &input.contract, &mut pack);
            }
            Some(pack)
        } else {
            None
        };
        let mut excerpt_guard = if is_fail_closed_scope_task(&input.contract) {
            context_pack
                .as_ref()
                .map(|pack| EntryPointExcerptGuard::apply(&input.repo_root, pack))
                .transpose()?
                .flatten()
        } else {
            None
        };
        let mut progress_probe_paths = created_scaffold_paths.to_vec();
        extend_unique_paths(
            &mut progress_probe_paths,
            &controller_bootstrap_scaffold_paths(&input.contract),
        );
        let entry_point_snapshots = if should_capture_progress_snapshots(
            &input.contract,
            progress_probe_paths.as_slice(),
        ) {
            capture_entry_point_snapshots(&input.repo_root, &input.contract, &progress_probe_paths)?
        } else {
            Vec::new()
        };

        let prompt = build_exec_prompt_with_mode(
            &input.contract,
            context_pack.as_ref(),
            created_scaffold_paths,
            retry_mode,
        );
        let git_guard = if is_fail_closed_scope_task(&input.contract) {
            GitGuardEnv::install()?
        } else {
            None
        };
        let mut command = Command::new("codex");
        command
            .arg("exec")
            .arg("--full-auto")
            .arg("--ephemeral")
            .arg("-C")
            .arg(&input.repo_root);
        if let Some(git_guard) = git_guard.as_ref() {
            git_guard.apply(&mut command);
        }
        if let Some(model) = &self.model {
            command.arg("-m").arg(model);
        }
        if let Some(reasoning_effort) = codex_executor_reasoning_effort(&input.contract) {
            command
                .arg("-c")
                .arg(format!("model_reasoning_effort=\"{reasoning_effort}\""));
        }
        command.arg("--").arg(prompt);
        let executor_timeout = effective_codex_executor_timeout(&input.contract);
        let timed_output = match run_command_with_timeout_and_tee(
            &mut command,
            executor_timeout,
            codex_executor_stall_timeout(),
            codex_executor_no_progress_timeout(),
            codex_executor_scaffold_progress_timeout(),
            codex_executor_orphan_grace_timeout(),
            input.stdout_path.clone(),
            input.stderr_path.clone(),
            input.executor_pid_path.clone(),
            Some((&input.repo_root, &input.contract)),
            Some((&input.repo_root, entry_point_snapshots.as_slice())),
        ) {
            Ok(output) => {
                let reclassified = if !entry_point_snapshots.is_empty() {
                    reclassify_stalled_post_check_zero_progress(
                        &input.repo_root,
                        entry_point_snapshots.as_slice(),
                        output,
                    )
                } else {
                    Ok(output)
                };
                if let Some(guard) = excerpt_guard.as_mut() {
                    guard.restore()?;
                }
                reclassified?
            }
            Err(err) => {
                if let Some(guard) = excerpt_guard.as_mut() {
                    let _ = guard.restore();
                }
                let _ = restore_missing_materialized_entry_points(
                    &input.repo_root,
                    &input.contract,
                    created_scaffold_paths,
                );
                let _ = restore_rust_workspace_bootstrap_scaffold(
                    &input.repo_root,
                    &input.contract,
                    created_scaffold_paths,
                );
                return Err(err);
            }
        };

        let restored_paths = restore_missing_materialized_entry_points(
            &input.repo_root,
            &input.contract,
            created_scaffold_paths,
        )?;
        let restored_bootstrap_paths = restore_rust_workspace_bootstrap_scaffold(
            &input.repo_root,
            &input.contract,
            created_scaffold_paths,
        )?;
        let mut restored_paths = restored_paths;
        extend_unique_paths(&mut restored_paths, &restored_bootstrap_paths);
        Ok(AttemptOutcome {
            timed_output,
            restored_paths,
        })
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn build_exec_prompt(
    contract: &Contract,
    context_pack: Option<&ContextPack>,
    created_scaffold_paths: &[String],
) -> String {
    build_exec_prompt_with_mode(contract, context_pack, created_scaffold_paths, false)
}

fn build_exec_prompt_with_mode(
    contract: &Contract,
    context_pack: Option<&ContextPack>,
    created_scaffold_paths: &[String],
    retry_mode: bool,
) -> String {
    let scope_rule = fail_closed_scope_rule(contract);
    let meta_workflow_rule = forbid_meta_workflow_rule();
    let vcs_restore_rule = forbid_vcs_restore_rule();
    let created_entry_points_section = if created_scaffold_paths.is_empty() {
        String::new()
    } else {
        format!(
            "The following controller-owned scaffold files were materialized for this run and must remain present: {}. Edit those paths in place and do not delete or rename them.\n",
            created_scaffold_paths.join(", ")
        )
    };
    let context_pack_section = context_pack
        .map(format_context_pack)
        .filter(|rendered| !rendered.trim().is_empty())
        .map(|rendered| {
            format!(
                "{rendered}\nUse this controller-built bounded context as the authoritative initial implementation context. If the context pack includes a controller-owned patch seed, apply those snippets in place first and adapt them only as much as needed to compile and satisfy the contract. If the context pack includes a controller-owned recipe seed, follow it directly and make the listed edits across the listed files before doing more orientation. For bounded tasks, entry-point files may be pre-masked to production-only content above the first test boundary before execution starts; treat the visible file content as authoritative on the first pass and do not reopen full entry-point files to hunt for omitted test sections. If the context pack lists missing entry-point files at baseline, treat those paths as approved new files and create them directly inside allowed scope instead of probing for them. If compile or required check output points to a specific allowed file and line outside the excerpt above, inspect only that narrow production snippet."
            )
        })
        .unwrap_or_default();
    let retry_section = if retry_mode {
        "This is the second and final bounded implementation pass after a no-progress failure. Do not reread the same context for orientation and do not print or restate the provided excerpts. Do not discard or revert any existing worktree changes. Start editing the allowed entry-point files immediately. If direct in-place implementation is still not possible from the current files and allowed scope, emit exactly one `PUNK_EXECUTION_BLOCKED: <reason>` line and stop.\n"
    } else {
        ""
    };
    if contract.allowed_scope.is_empty() {
        return format!(
            "Implement the approved contract in the current repo. Contract id: {}. Behavior requirements: {}. Stay narrowly scoped to the contract and do not perform broad repo-wide search unless a concrete compile or verification blocker requires it. {}{} {} {} {} If you are blocked by scope, missing manifest wiring, or a similar execution blocker, do not ask the operator a question. Instead emit exactly one single-line sentinel in the form `{}` and stop without claiming success. When implementation is complete and all required checks are done, emit exactly one single-line sentinel in the form `{}`. If scope is unclear but not blocked, make the smallest safe change and explain what remains unspecified.",
            contract.id,
            contract.behavior_requirements.join("; "),
            context_pack_section,
            retry_section,
            scope_rule,
            meta_workflow_rule,
            vcs_restore_rule,
            blocked_execution_template(),
            successful_execution_template()
        );
    }
    format!(
        "Implement the approved contract in the current repo.\nContract id: {}\nBehavior requirements: {}\nAllowed scope: {}\nEntry points: {}\nExpected interfaces: {}\nTarget checks to satisfy: {}\nIntegrity checks to keep passing: {}\n{}{}\nStart by inspecting only the listed entry points and other files inside allowed scope.\nDo not perform broad repo-wide search.\n{}\n{}\n{}\nIf you are blocked by scope, missing manifest wiring, or a similar execution blocker, do not ask the operator a question. Instead emit exactly one single-line sentinel in the form `{}` and stop without claiming success.\nWhen implementation is complete and all required checks are done, emit exactly one single-line sentinel in the form `{}`.\nOnly modify files inside allowed scope.",
        contract.id,
        contract.behavior_requirements.join("; "),
        contract.allowed_scope.join(", "),
        contract.entry_points.join(", "),
        contract.expected_interfaces.join("; "),
        contract.target_checks.join("; "),
        contract.integrity_checks.join("; "),
        format!("{created_entry_points_section}{context_pack_section}"),
        retry_section,
        scope_rule,
        meta_workflow_rule,
        vcs_restore_rule,
        blocked_execution_template(),
        successful_execution_template(),
    )
}

fn build_patch_apply_prompt(contract: &Contract, context_pack: &ContextPack) -> String {
    let context_pack_section = format_patch_context_pack(context_pack);
    let plan_rule = if context_pack.plan_seed.is_some() {
        "- if the controller-owned plan prepass is present, treat its target files, symbols, and insertion points as authoritative and produce the patch directly against that plan\n"
    } else {
        ""
    };
    format!(
        "Produce plain text only for a controller-owned patch/apply lane.\n\
Return exactly one of:\n\
1. a single apply_patch-style patch starting with `*** Begin Patch` and ending with `*** End Patch`\n\
2. a single line `{blocked}` followed by a concise reason\n\
The repository is read-only for this step; do not modify files directly.\n\
Contract id: {}\n\
Behavior requirements: {}\n\
Allowed scope: {}\n\
Entry points: {}\n\
Expected interfaces: {}\n\
Target checks to satisfy after apply: {}\n\
Integrity checks to keep passing after apply: {}\n\
{}\n\
{}\n\
Requirements:\n\
- emit one complete `apply_patch` envelope when implementation is possible\n\
- touch only files inside allowed scope\n\
- use only `*** Update File:` sections in this lane\n\
- do not add, delete, move, or rename files in this lane\n\
- do not run `rg`, `grep`, `sed`, `cat`, `find`, `ls`, or any other shell command for orientation in this lane\n\
- the controller-owned plan excerpts already include the exact local edit windows; do not use python or any shell/interpreter command to rediscover them\n\
- prefer the smallest bounded patch that follows any controller-owned plan targets exactly before doing broader exploration\n\
- each update hunk must include enough unchanged or removed context lines for deterministic controller-side application\n\
- prefer the smallest bounded patch that gets the implementation started and covers the approved behavior\n\
- output patch text only; do not output JSON, commentary, bullets, or markdown fences\n\
- if implementation is blocked inside allowed scope, output exactly one `{blocked}` line and nothing else\n",
        contract.id,
        contract.behavior_requirements.join("; "),
        contract.allowed_scope.join(", "),
        contract.entry_points.join(", "),
        contract.expected_interfaces.join("; "),
        contract.target_checks.join("; "),
        contract.integrity_checks.join("; "),
        context_pack_section,
        plan_rule,
        blocked = blocked_execution_template(),
    )
}

fn build_patch_plan_prompt(contract: &Contract, context_pack: &ContextPack) -> String {
    let context_pack_section = format_plan_context_pack(context_pack);
    format!(
        "Produce plain text only for a controller-owned planning prepass.\n\
Return exactly one of:\n\
1. a single compact plan envelope starting with `PUNK_PLAN_BEGIN` and ending with `PUNK_PLAN_END`\n\
2. a single line `{blocked}` followed by a concise reason\n\
The repository is read-only for this step; do not modify files directly.\n\
Contract id: {}\n\
Behavior requirements: {}\n\
Allowed scope: {}\n\
Entry points: {}\n\
Expected interfaces: {}\n\
{}\n\
Fail-closed rules:\n\
- rely only on the bounded context and controller-supplied plan skeleton shown below\n\
- do not inspect, search, or mention any file outside allowed scope\n\
- do not run broad repo-wide search commands\n\
- do not run shell commands for orientation during this prepass\n\
- if the bounded context plus controller skeleton is insufficient, emit `{blocked}` immediately\n\
Plan format requirements:\n\
- first line inside the envelope must be `SUMMARY: <one-line summary>`\n\
- each target must use exactly these three lines:\n\
  `TARGET: <repo-path>`\n\
  `SYMBOL: <function/type/section to edit>`\n\
  `INSERT: <exact insertion point or nearby anchor>`\n\
  `SKETCH: <one-line execution sketch>`\n\
- include 1 to 3 targets only\n\
- all targets must stay inside allowed scope\n\
- prefer the smallest exact plan that directly unblocks patch generation\n\
- do not output JSON, markdown fences, or commentary\n\
- if implementation is blocked inside allowed scope, output exactly one `{blocked}` line and nothing else\n",
        contract.id,
        contract.behavior_requirements.join("; "),
        contract.allowed_scope.join(", "),
        contract.entry_points.join(", "),
        contract.expected_interfaces.join("; "),
        context_pack_section,
        blocked = blocked_execution_template(),
    )
}

fn build_draft_prompt(input: &DraftInput) -> String {
    format!(
        "Draft an approve-ready contract proposal for the current repository.\n\
Return JSON only and match the provided schema exactly.\n\
Do not invent ids, timestamps, statuses, or event metadata.\n\
Use only repo-relative paths.\n\
Keep scope bounded and conservative.\n\
If the user prompt names exact file paths or exact shell commands, prefer those explicit user-provided details over weaker scan guesses.\n\
For bounded file-level changes, prefer `allowed_scope` entries from `candidate_file_scope_paths`.\n\
Use `candidate_directory_scope_paths` only when the user explicitly requests directory/module/package scope.\n\
Prefer `entry_points` from `candidate_entry_points` when the change is file-level.\n\
Choose at least one `target_checks` command from `candidate_target_checks` when available, and keep it as specific as possible.\n\
Choose `integrity_checks` from `candidate_integrity_checks`; do not invent checks outside the scan summary.\n\
If you cannot infer a trustworthy integrity check, do not guess.\n\n\
User prompt:\n{}\n\n\
Deterministic repo scan:\n{}\n",
        input.prompt,
        serde_json::to_string_pretty(&input.scan).unwrap_or_else(|_| "{}".to_string())
    )
}

fn build_refine_prompt(input: &RefineInput) -> String {
    format!(
        "Refine the existing draft contract proposal for the current repository.\n\
Return JSON only and match the provided schema exactly.\n\
Keep the scope bounded and conservative.\n\
Do not invent ids, timestamps, statuses, or event metadata.\n\n\
Preserve exact file paths and shell commands explicitly named by the user unless they are invalid.\n\
Prefer the narrowest valid `allowed_scope` and the most specific `target_checks` from the scan summary.\n\
Keep `integrity_checks` grounded in the scan summary only.\n\n\
Original prompt:\n{}\n\n\
Guidance:\n{}\n\n\
Current proposal:\n{}\n\n\
Deterministic repo scan:\n{}\n",
        input.prompt,
        input.guidance,
        serde_json::to_string_pretty(&input.current).unwrap_or_else(|_| "{}".to_string()),
        serde_json::to_string_pretty(&input.scan).unwrap_or_else(|_| "{}".to_string()),
    )
}

fn build_compact_draft_prompt(input: &DraftInput) -> String {
    format!(
        "Draft an approve-ready contract proposal for the current repository.\n\
Return JSON only and match the provided schema exactly.\n\
Do not invent ids, timestamps, statuses, or event metadata.\n\
Use repo-relative paths only.\n\
Keep scope bounded and conservative.\n\
Prefer exact file paths and shell commands explicitly named in the user prompt.\n\
For bounded file-level changes, prefer `candidate_file_scope_paths`; use `candidate_directory_scope_paths` only if the prompt explicitly asks for directory/module/package scope.\n\
Use only target and integrity checks grounded in the repo scan.\n\n\
User prompt:\n{}\n\n\
Repo scan JSON:\n{}\n",
        input.prompt,
        serde_json::to_string(&input.scan).unwrap_or_else(|_| "{}".to_string())
    )
}

fn build_compact_refine_prompt(input: &RefineInput) -> String {
    format!(
        "Refine the existing draft contract proposal for the current repository.\n\
Return JSON only and match the provided schema exactly.\n\
Keep scope bounded and conservative.\n\
Do not invent ids, timestamps, statuses, or event metadata.\n\
Preserve exact file paths and shell commands explicitly named by the user unless invalid.\n\
Keep integrity checks grounded in the repo scan only.\n\n\
Original prompt:\n{}\n\n\
Guidance:\n{}\n\n\
Current proposal JSON:\n{}\n\n\
Repo scan JSON:\n{}\n",
        input.prompt,
        input.guidance,
        serde_json::to_string(&input.current).unwrap_or_else(|_| "{}".to_string()),
        serde_json::to_string(&input.scan).unwrap_or_else(|_| "{}".to_string()),
    )
}

fn draft_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "title",
            "summary",
            "entry_points",
            "import_paths",
            "expected_interfaces",
            "behavior_requirements",
            "allowed_scope",
            "target_checks",
            "integrity_checks",
            "risk_level"
        ],
        "properties": {
            "title": {"type": "string"},
            "summary": {"type": "string"},
            "entry_points": {"type": "array", "items": {"type": "string"}},
            "import_paths": {"type": "array", "items": {"type": "string"}},
            "expected_interfaces": {"type": "array", "items": {"type": "string"}},
            "behavior_requirements": {"type": "array", "items": {"type": "string"}},
            "allowed_scope": {"type": "array", "items": {"type": "string"}},
            "target_checks": {"type": "array", "items": {"type": "string"}},
            "integrity_checks": {"type": "array", "items": {"type": "string"}},
            "risk_level": {"type": "string"}
        }
    })
}

fn draft_schema_path() -> Result<PathBuf> {
    Ok(std::env::temp_dir().join(format!("punk-draft-schema-{}.json", std::process::id())))
}

fn draft_output_path() -> Result<PathBuf> {
    Ok(std::env::temp_dir().join(format!("punk-draft-output-{}.json", std::process::id())))
}

fn codex_executor_timeout() -> Duration {
    let seconds = std::env::var("PUNK_CODEX_EXECUTOR_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(300);
    Duration::from_secs(seconds)
}

fn effective_codex_executor_timeout(contract: &Contract) -> Duration {
    let base = codex_executor_timeout();
    if is_greenfield_manifest_no_progress(contract, &contract.entry_points) {
        return base.min(Duration::from_secs(20));
    }
    base
}

fn codex_patch_lane_timeout() -> Duration {
    let seconds = std::env::var("PUNK_CODEX_PATCH_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(90);
    Duration::from_secs(seconds)
}

fn materialize_rust_workspace_bootstrap_scaffold(
    repo_root: &Path,
    contract: &Contract,
) -> Result<Vec<String>> {
    let Some(files) = rust_workspace_bootstrap_templates(contract) else {
        return Ok(Vec::new());
    };
    let mut created = Vec::new();
    for (path, contents) in files {
        if !path_is_in_allowed_scope(&path, &contract.allowed_scope) {
            continue;
        }
        let file_path = repo_root.join(&path);
        if file_path.exists() {
            continue;
        }
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create bootstrap scaffold parent {}", path))?;
        }
        fs::write(&file_path, contents)
            .with_context(|| format!("materialize bootstrap scaffold {}", path))?;
        created.push(path);
    }
    Ok(created)
}

fn restore_rust_workspace_bootstrap_scaffold(
    repo_root: &Path,
    contract: &Contract,
    created_paths: &[String],
) -> Result<Vec<String>> {
    let Some(files) = rust_workspace_bootstrap_templates(contract) else {
        return Ok(Vec::new());
    };
    let template_map = files
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut restored = Vec::new();
    for path in created_paths {
        let Some(contents) = template_map.get(path) else {
            continue;
        };
        let file_path = repo_root.join(path);
        if file_path.exists() {
            continue;
        }
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create restored bootstrap scaffold parent {}", path))?;
        }
        fs::write(&file_path, contents)
            .with_context(|| format!("restore bootstrap scaffold {}", path))?;
        restored.push(path.clone());
    }
    Ok(restored)
}

fn rust_workspace_bootstrap_templates(contract: &Contract) -> Option<Vec<(String, String)>> {
    if contract.entry_points != vec!["Cargo.toml".to_string()] {
        return None;
    }
    if !contract
        .target_checks
        .iter()
        .chain(contract.integrity_checks.iter())
        .any(|check| check.trim() == "cargo test --workspace")
    {
        return None;
    }
    let crate_dirs = rust_workspace_bootstrap_crate_dirs(contract);
    if crate_dirs.is_empty() {
        return None;
    }

    let core_crate = crate_dirs
        .iter()
        .map(|dir| rust_crate_name_from_scope(dir))
        .find(|name| name.contains("core"));
    let mut files = vec![(
        "Cargo.toml".to_string(),
        render_rust_workspace_manifest(&crate_dirs),
    )];

    for crate_dir in &crate_dirs {
        let crate_name = rust_crate_name_from_scope(crate_dir);
        let is_binary = crate_name.contains("cli");
        files.push((
            format!("{crate_dir}/Cargo.toml"),
            render_rust_member_manifest(&crate_name, is_binary, core_crate.as_deref()),
        ));
        let source_path = if is_binary {
            format!("{crate_dir}/src/main.rs")
        } else {
            format!("{crate_dir}/src/lib.rs")
        };
        files.push((
            source_path,
            render_rust_member_source(&crate_name, is_binary, core_crate.as_deref()),
        ));
    }

    if contract
        .allowed_scope
        .iter()
        .any(|scope| scope == "tests" || scope.starts_with("tests/"))
    {
        files.push((
            "tests/README.md".to_string(),
            "# Controller-owned bootstrap placeholder\n".to_string(),
        ));
    }

    Some(files)
}

fn rust_workspace_bootstrap_crate_dirs(contract: &Contract) -> Vec<String> {
    let explicit = contract
        .allowed_scope
        .iter()
        .filter(|scope| scope.starts_with("crates/") && !is_file_like_scope(scope))
        .cloned()
        .collect::<Vec<_>>();
    if !explicit.is_empty() {
        return explicit;
    }
    if !contract.allowed_scope.iter().any(|scope| scope == "crates") {
        return Vec::new();
    }
    let Some(app_slug) = infer_rust_bootstrap_app_slug(contract) else {
        return Vec::new();
    };
    vec![
        format!("crates/{app_slug}-cli"),
        format!("crates/{app_slug}-core"),
    ]
}

fn infer_rust_bootstrap_app_slug(contract: &Contract) -> Option<String> {
    let mut candidates = Vec::new();
    candidates.extend(extract_backticked_identifiers(&contract.prompt_source));
    for item in &contract.expected_interfaces {
        candidates.extend(extract_backticked_identifiers(item));
        if let Some(cli_name) = extract_cli_name(item) {
            candidates.push(cli_name);
        }
    }
    for item in &contract.behavior_requirements {
        candidates.extend(extract_backticked_identifiers(item));
        if let Some(cli_name) = extract_cli_name(item) {
            candidates.push(cli_name);
        }
    }
    candidates.extend(extract_prompt_bootstrap_targets(&contract.prompt_source));
    candidates
        .into_iter()
        .find(|candidate| is_viable_bootstrap_app_slug(candidate))
}

fn extract_backticked_identifiers(text: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut inside = false;
    let mut current = String::new();
    for ch in text.chars() {
        match ch {
            '`' if inside => {
                let candidate = current.trim().to_string();
                if !candidate.is_empty() {
                    values.push(candidate);
                }
                current.clear();
                inside = false;
            }
            '`' => {
                inside = true;
                current.clear();
            }
            _ if inside => current.push(ch),
            _ => {}
        }
    }
    values
}

fn extract_cli_name(text: &str) -> Option<String> {
    let tokens = tokenize_ascii_words(text);
    tokens
        .windows(2)
        .find(|window| window[1] == "cli" && is_viable_bootstrap_app_slug(&window[0]))
        .map(|window| window[0].clone())
}

fn extract_prompt_bootstrap_targets(text: &str) -> Vec<String> {
    let tokens = tokenize_ascii_words(text);
    let mut out = Vec::new();
    for window in tokens.windows(3) {
        if window[0] == "implement"
            && is_viable_bootstrap_app_slug(&window[1])
            && (window[2] == "init" || window[2] == "validate")
        {
            out.push(window[1].clone());
        }
    }
    out
}

fn tokenize_ascii_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn is_viable_bootstrap_app_slug(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || normalized.contains('/')
        || normalized == "cargo"
        || normalized == "cargo.toml"
        || normalized == "rust"
        || normalized == "workspace"
        || normalized == "scaffold"
        || normalized == "crates"
        || normalized == "tests"
        || normalized == "cli"
        || normalized == "core"
        || normalized == "init"
        || normalized == "validate"
    {
        return false;
    }
    normalized
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn controller_bootstrap_scaffold_paths(contract: &Contract) -> Vec<String> {
    rust_workspace_bootstrap_templates(contract)
        .map(|files| files.into_iter().map(|(path, _)| path).collect())
        .unwrap_or_default()
}

fn rust_crate_name_from_scope(scope: &str) -> String {
    scope.rsplit('/').next().unwrap_or(scope).to_string()
}

fn render_rust_workspace_manifest(crate_dirs: &[String]) -> String {
    let members = crate_dirs
        .iter()
        .map(|dir| format!("    \"{dir}\","))
        .collect::<Vec<_>>()
        .join("\n");
    format!("[workspace]\nresolver = \"2\"\nmembers = [\n{members}\n]\n")
}

fn render_rust_member_manifest(
    crate_name: &str,
    is_binary: bool,
    core_crate: Option<&str>,
) -> String {
    let mut manifest =
        format!("[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
    if is_binary {
        if let Some(core_crate) = core_crate.filter(|core| *core != crate_name) {
            manifest.push_str("\n[dependencies]\n");
            manifest.push_str(&format!(
                "{core_crate} = {{ path = \"../{core_crate}\" }}\n"
            ));
        }
    }
    manifest
}

fn render_rust_member_source(
    crate_name: &str,
    is_binary: bool,
    core_crate: Option<&str>,
) -> String {
    if is_binary {
        if let Some(core_crate) = core_crate.filter(|core| *core != crate_name) {
            let crate_ref = core_crate.replace('-', "_");
            return format!(
                "fn main() {{\n    let _ = {crate_ref}::init();\n    let _ = {crate_ref}::validate();\n}}\n"
            );
        }
        return "fn main() {}\n".to_string();
    }

    "pub fn init() -> &'static str {\n    \"pubpunk initialized\"\n}\n\npub fn validate() -> bool {\n    true\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn bootstrap_smoke() {\n        assert_eq!(init(), \"pubpunk initialized\");\n        assert!(validate());\n    }\n}\n"
        .to_string()
}

fn extend_unique_paths(target: &mut Vec<String>, extra: &[String]) {
    for path in extra {
        if !target.iter().any(|existing| existing == path) {
            target.push(path.clone());
        }
    }
}

fn codex_plan_prepass_timeout() -> Duration {
    let seconds = std::env::var("PUNK_CODEX_PLAN_PREPASS_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(45);
    Duration::from_secs(seconds)
}

fn codex_executor_stall_timeout() -> Duration {
    let seconds = std::env::var("PUNK_CODEX_EXECUTOR_STALL_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(30);
    Duration::from_secs(seconds)
}

fn codex_executor_no_progress_timeout() -> Duration {
    let seconds = std::env::var("PUNK_CODEX_EXECUTOR_NO_PROGRESS_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(12);
    Duration::from_secs(seconds)
}

fn codex_executor_scaffold_progress_timeout() -> Duration {
    let seconds = std::env::var("PUNK_CODEX_EXECUTOR_SCAFFOLD_PROGRESS_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(5);
    Duration::from_secs(seconds)
}

fn codex_executor_orphan_grace_timeout() -> Duration {
    let millis = std::env::var("PUNK_CODEX_EXECUTOR_ORPHAN_GRACE_MILLIS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1200);
    Duration::from_millis(millis)
}

fn codex_drafter_timeout() -> Duration {
    let seconds = std::env::var("PUNK_CODEX_DRAFTER_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(30);
    Duration::from_secs(seconds)
}

fn drafter_attempt_timeouts(
    total_timeout: Duration,
    retry_enabled: bool,
) -> (Duration, Option<Duration>) {
    if !retry_enabled {
        return (total_timeout, None);
    }

    let total_millis = total_timeout.as_millis() as u64;
    if total_millis < 1_500 {
        return (total_timeout, None);
    }

    let retry_millis = (total_millis / 3)
        .max(5_000)
        .min(total_millis.saturating_sub(1_000));
    if retry_millis == 0 || retry_millis >= total_millis {
        return (total_timeout, None);
    }

    (
        Duration::from_millis(total_millis - retry_millis),
        Some(Duration::from_millis(retry_millis)),
    )
}

fn codex_executor_reasoning_effort(contract: &Contract) -> Option<&'static str> {
    if !is_bounded_execution_task(contract) {
        return None;
    }

    match contract.risk_level.trim().to_ascii_lowercase().as_str() {
        "low" => Some("low"),
        "medium" => Some("medium"),
        _ => None,
    }
}

fn execution_lane_for_contract(repo_root: &Path, contract: &Contract) -> ExecutionLane {
    if is_self_referential_reliability_slice(contract) {
        return ExecutionLane::Manual;
    }
    if is_patch_apply_lane_candidate(repo_root, contract) {
        return ExecutionLane::PatchApply;
    }
    ExecutionLane::Exec
}

fn is_explicit_repo_file_scope(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.ends_with('/') {
        return false;
    }

    Path::new(trimmed).extension().is_some()
}

fn is_bounded_execution_task(contract: &Contract) -> bool {
    !contract.allowed_scope.is_empty()
        && contract.allowed_scope.len() <= 5
        && contract
            .allowed_scope
            .iter()
            .all(|path| is_explicit_repo_file_scope(path))
        && !contract.entry_points.is_empty()
        && contract.entry_points.len() <= 5
        && contract
            .entry_points
            .iter()
            .all(|path| is_explicit_repo_file_scope(path))
}

fn should_capture_progress_snapshots(contract: &Contract, progress_probe_paths: &[String]) -> bool {
    is_fail_closed_scope_task(contract)
        || !progress_probe_paths.is_empty()
        || (!contract.allowed_scope.is_empty()
            && contract.allowed_scope.len() <= 5
            && !contract.entry_points.is_empty()
            && contract.entry_points.len() <= 5)
}

fn fail_closed_scope_rule(contract: &Contract) -> &'static str {
    if is_fail_closed_scope_task(contract) {
        "Do not read any file outside allowed scope or entry points. Prioritize production code paths first. When an allowed entry-point file contains a `#[cfg(test)]` module or a clear test-only section, treat everything below that boundary as off-limits by default unless a concrete compile error or required check output points directly to those test lines. Avoid reading `#[cfg(test)]` modules or long test sections in allowed-scope files unless that explicit signal exists. Do not create temporary type-introspection tests, debug-print probes, or `--nocapture` discovery loops just to infer Rust type shapes inside the bounded task. Implement the approved contract directly inside the allowed scope instead. If you need any out-of-scope file for compile or verification reasons, emit exactly one single-line sentinel in the form `PUNK_EXECUTION_BLOCKED: need out-of-scope file <repo-relative-path> because <reason>` before reading it, then stop immediately. If a required Rust type shape still cannot be derived safely from the in-scope source files, or direct implementation is not possible without reading past the test boundary or inventing an alternate workflow, emit exactly one single-line sentinel in the form `PUNK_EXECUTION_BLOCKED: <reason>` and stop immediately instead of adding temporary discovery code or meta-workflow artifacts."
    } else {
        "Do not read unrelated files outside allowed scope unless a concrete compile or verification blocker makes that strictly necessary. If you must inspect an additional file because of such a blocker, keep it minimal and explain the blocker in your final summary."
    }
}

fn forbid_meta_workflow_rule() -> &'static str {
    "Do not invent, invoke, or follow any alternate framework or meta-workflow such as signum phases, `.signum` artifacts, contract-engineer files, policy files, or similar contract/execute/audit pipeline outputs. The approved cut contract already defines the execution workflow; implement it directly inside the allowed scope instead."
}

fn forbid_vcs_restore_rule() -> &'static str {
    "Do not use git checkout, git restore, git reset, git clean, git switch, or similar VCS restore/reset commands to discard, reset, or reorient the worktree. Keep the current worktree state and edit the allowed files in place."
}

fn is_fail_closed_scope_task(contract: &Contract) -> bool {
    is_bounded_execution_task(contract)
        && matches!(
            contract.risk_level.trim().to_ascii_lowercase().as_str(),
            "low" | "medium"
        )
}

fn is_patch_apply_lane_candidate(repo_root: &Path, contract: &Contract) -> bool {
    if !is_fail_closed_scope_task(contract) {
        return false;
    }
    if contract.allowed_scope.len() > 2 || contract.entry_points.len() > 2 {
        return false;
    }
    if contract.allowed_scope.is_empty() || contract.entry_points.is_empty() {
        return false;
    }
    if !contract
        .allowed_scope
        .iter()
        .all(|path| repo_root.join(path).exists())
    {
        return false;
    }
    if !contract
        .entry_points
        .iter()
        .all(|path| repo_root.join(path).exists())
    {
        return false;
    }

    let mut text = contract.prompt_source.to_ascii_lowercase();
    for item in contract
        .expected_interfaces
        .iter()
        .chain(contract.behavior_requirements.iter())
    {
        text.push('\n');
        text.push_str(&item.to_ascii_lowercase());
    }

    [
        "bridge",
        "wiring",
        "summary",
        "status",
        "aggregation",
        "ratchet",
        "report",
        "cli",
        "output",
        "surface",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn needs_patch_plan_prepass(contract: &Contract, context_pack: &ContextPack) -> bool {
    if contract.allowed_scope.len() != 2 || contract.entry_points.len() != 2 {
        return false;
    }
    if context_pack.plan_seed.is_some() {
        return false;
    }

    let mut text = contract.prompt_source.to_ascii_lowercase();
    for item in contract
        .expected_interfaces
        .iter()
        .chain(contract.behavior_requirements.iter())
    {
        text.push('\n');
        text.push_str(&item.to_ascii_lowercase());
    }

    let uncertainty_markers = [
        "reuse existing",
        "derived from",
        "similar to existing",
        "backward-compatible",
        "backward compatible",
        "concise human-readable",
        "status snapshot",
        "window data",
    ];
    let surface_markers = ["status", "summary", "output", "report", "window"];

    uncertainty_markers
        .iter()
        .any(|needle| text.contains(needle))
        || surface_markers
            .iter()
            .filter(|needle| text.contains(**needle))
            .count()
            >= 3
}

fn manual_mode_block_summary(contract: &Contract) -> Option<String> {
    if !is_self_referential_reliability_slice(contract) {
        return None;
    }
    Some(
        "PUNK_EXECUTION_BLOCKED: self-referential reliability slice requires manual bounded implementation"
            .to_string(),
    )
}

fn is_self_referential_reliability_slice(contract: &Contract) -> bool {
    if !is_fail_closed_scope_task(contract) {
        return false;
    }
    let scoped_paths: Vec<&str> = contract
        .allowed_scope
        .iter()
        .chain(contract.entry_points.iter())
        .map(String::as_str)
        .collect();
    if scoped_paths.is_empty() {
        return false;
    }
    if !scoped_paths
        .iter()
        .all(|path| is_self_referential_control_plane_path(path))
    {
        return false;
    }
    let mut text = contract.prompt_source.to_ascii_lowercase();
    for item in contract
        .expected_interfaces
        .iter()
        .chain(contract.behavior_requirements.iter())
    {
        text.push('\n');
        text.push_str(&item.to_ascii_lowercase());
    }
    [
        "self-hosting",
        "reliability",
        "retry",
        "no implementation progress",
        "bounded context dispatch",
        "post-check",
        "stall",
        "patch seed",
        "hunk seed",
        "bootstrap hunk",
        "controller-owned",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn is_self_referential_control_plane_path(path: &str) -> bool {
    path.starts_with("crates/punk-adapters/")
        || path.starts_with("crates/punk-vcs/")
        || path.starts_with("crates/punk-cli/")
        || path.starts_with("crates/punk-orch/")
}

fn should_retry_after_no_progress(
    contract: &Contract,
    paths: &[String],
    stdout: &str,
    stderr: &str,
) -> bool {
    if paths.is_empty() {
        return false;
    }
    if is_greenfield_manifest_no_progress(contract, paths)
        && logs_indicate_missing_manifest_wiring(stdout, stderr, paths)
    {
        return false;
    }
    true
}

fn is_greenfield_manifest_no_progress(contract: &Contract, paths: &[String]) -> bool {
    !contract.entry_points.is_empty()
        && contract
            .entry_points
            .iter()
            .all(|path| is_greenfield_manifest_entry_point(path))
        && paths
            .iter()
            .all(|path| is_greenfield_manifest_entry_point(path))
        && paths
            .iter()
            .all(|path| contract.entry_points.contains(path))
}

fn is_greenfield_manifest_entry_point(path: &str) -> bool {
    matches!(
        path,
        "Cargo.toml" | "go.mod" | "pyproject.toml" | "package.json"
    )
}

fn logs_indicate_missing_manifest_wiring(stdout: &str, stderr: &str, paths: &[String]) -> bool {
    if paths.is_empty()
        || !paths
            .iter()
            .all(|path| is_greenfield_manifest_entry_point(path))
    {
        return false;
    }
    let haystack = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    paths.iter().all(|path| {
        let lowered = path.to_ascii_lowercase();
        haystack.contains(&format!("missing {lowered}"))
            || haystack.contains(&format!("{lowered} -> missing"))
            || haystack.contains(&format!("missing manifest wiring for {lowered}"))
    })
}

fn greenfield_manifest_blocked_summary(repo_root: &Path, contract: &Contract) -> Option<String> {
    if !is_greenfield_manifest_no_progress(contract, &contract.entry_points) {
        return None;
    }

    let mut missing_surfaces = Vec::new();
    for path in &contract.entry_points {
        if !repo_root.join(path).exists() {
            missing_surfaces.push(path.clone());
        }
    }
    for scope in &contract.allowed_scope {
        if contract.entry_points.iter().any(|entry| entry == scope) || is_file_like_scope(scope) {
            continue;
        }
        let scope_path = repo_root.join(scope);
        let is_missing_or_empty = !scope_path.exists()
            || fs::read_dir(&scope_path)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(true);
        if is_missing_or_empty {
            missing_surfaces.push(format!("{scope}/"));
        }
    }
    if missing_surfaces.is_empty() {
        return None;
    }

    let check = contract
        .target_checks
        .first()
        .cloned()
        .or_else(|| contract.integrity_checks.first().cloned())
        .unwrap_or_else(|| "the required checks".to_string());
    Some(format!(
        "PUNK_EXECUTION_BLOCKED: missing manifest wiring in allowed scope; repo root has no {}, so there is no scaffold entry point to implement or verify with {}",
        missing_surfaces.join(", "),
        check
    ))
}

fn no_progress_only_in_controller_scaffold(
    no_progress_paths: &[String],
    created_scaffold_paths: &[String],
) -> bool {
    !no_progress_paths.is_empty()
        && no_progress_paths
            .iter()
            .all(|path| created_scaffold_paths.iter().any(|created| created == path))
}

fn merged_contract_checks(contract: &Contract) -> Vec<String> {
    let mut checks = contract.target_checks.clone();
    for check in &contract.integrity_checks {
        if !checks.iter().any(|existing| existing == check) {
            checks.push(check.clone());
        }
    }
    checks
}

fn run_command_with_timeout(command: &mut Command, timeout: Duration) -> Result<TimedOutput> {
    #[cfg(unix)]
    command.process_group(0);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let child_pid = child.id();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("child stdout pipe unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("child stderr pipe unavailable"))?;
    let progress = Arc::new(ProgressTracker::new());
    let stdout_live = Arc::new(Mutex::new(Vec::new()));
    let stderr_live = Arc::new(Mutex::new(Vec::new()));
    let stdout_handle =
        spawn_stream_capture(stdout, progress.clone(), true, Some(stdout_live.clone()));
    let stderr_handle = spawn_stream_capture(stderr, progress, false, Some(stderr_live.clone()));
    let start = Instant::now();
    let (timed_out, orphaned) = loop {
        if child.try_wait()?.is_some() {
            if !wait_for_stream_completion(
                &stdout_handle,
                &stderr_handle,
                codex_executor_orphan_grace_timeout(),
            ) {
                terminate_process_tree(&mut child, child_pid);
                break (false, true);
            }
            break (false, false);
        }
        if start.elapsed() >= timeout {
            terminate_process_tree(&mut child, child_pid);
            break (true, false);
        }
        thread::sleep(Duration::from_millis(200));
    };
    let status = child.wait()?;
    let stdout = collect_stream_capture_output(
        stdout_handle,
        &stdout_live,
        codex_executor_orphan_grace_timeout(),
    )?;
    let stderr = collect_stream_capture_output(
        stderr_handle,
        &stderr_live,
        codex_executor_orphan_grace_timeout(),
    )?;
    Ok(TimedOutput {
        output: std::process::Output {
            status,
            stdout,
            stderr,
        },
        timed_out,
        stalled: false,
        orphaned,
        no_progress_paths: Vec::new(),
        scaffold_only_paths: Vec::new(),
        post_check_zero_progress_paths: Vec::new(),
    })
}

fn run_patch_lane_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    executor_pid_path: PathBuf,
) -> Result<PatchLaneTimedOutput> {
    #[cfg(unix)]
    command.process_group(0);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let child_pid = child.id();
    write_executor_pid(&executor_pid_path, child_pid)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("child stdout pipe unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("child stderr pipe unavailable"))?;
    let progress = Arc::new(ProgressTracker::new());
    let stdout_live = Arc::new(Mutex::new(Vec::new()));
    let stderr_live = Arc::new(Mutex::new(Vec::new()));
    let stdout_handle = spawn_stream_tee_with_live_capture(
        stdout,
        stdout_path.clone(),
        progress.clone(),
        true,
        Some(stdout_live.clone()),
    );
    let stderr_handle = spawn_stream_tee_with_live_capture(
        stderr,
        stderr_path.clone(),
        progress,
        false,
        Some(stderr_live.clone()),
    );
    let start = Instant::now();
    let (timed_out, orphaned, response) = loop {
        if let Some(response) = detect_patch_lane_response(&stdout_live, &stderr_live) {
            terminate_process_tree(&mut child, child_pid);
            break (false, false, Some(response));
        }
        if child.try_wait()?.is_some() {
            if !wait_for_stream_completion(
                &stdout_handle,
                &stderr_handle,
                codex_executor_orphan_grace_timeout(),
            ) {
                terminate_process_tree(&mut child, child_pid);
                break (false, true, None);
            }
            break (false, false, None);
        }
        if start.elapsed() >= timeout {
            let response = detect_patch_lane_response(&stdout_live, &stderr_live);
            terminate_process_tree(&mut child, child_pid);
            break (response.is_none(), false, response);
        }
        thread::sleep(Duration::from_millis(200));
    };
    let status = child.wait()?;
    let stdout = collect_stream_tee_output(
        stdout_handle,
        &stdout_path,
        codex_executor_orphan_grace_timeout(),
    )?;
    let stderr = collect_stream_tee_output(
        stderr_handle,
        &stderr_path,
        codex_executor_orphan_grace_timeout(),
    )?;
    Ok(PatchLaneTimedOutput {
        output: std::process::Output {
            status,
            stdout,
            stderr,
        },
        timed_out,
        orphaned,
        response,
    })
}

fn run_plan_lane_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    executor_pid_path: PathBuf,
) -> Result<PlanLaneTimedOutput> {
    #[cfg(unix)]
    command.process_group(0);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let child_pid = child.id();
    write_executor_pid(&executor_pid_path, child_pid)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("child stdout pipe unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("child stderr pipe unavailable"))?;
    let progress = Arc::new(ProgressTracker::new());
    let stdout_live = Arc::new(Mutex::new(Vec::new()));
    let stderr_live = Arc::new(Mutex::new(Vec::new()));
    let stdout_handle = spawn_stream_tee_with_live_capture(
        stdout,
        stdout_path.clone(),
        progress.clone(),
        true,
        Some(stdout_live.clone()),
    );
    let stderr_handle = spawn_stream_tee_with_live_capture(
        stderr,
        stderr_path.clone(),
        progress,
        false,
        Some(stderr_live.clone()),
    );
    let start = Instant::now();
    let (timed_out, orphaned, response) = loop {
        if let Some(response) = detect_plan_prepass_response(&stdout_live, &stderr_live) {
            terminate_process_tree(&mut child, child_pid);
            break (false, false, Some(response));
        }
        if child.try_wait()?.is_some() {
            if !wait_for_stream_completion(
                &stdout_handle,
                &stderr_handle,
                codex_executor_orphan_grace_timeout(),
            ) {
                terminate_process_tree(&mut child, child_pid);
                break (false, true, None);
            }
            break (false, false, None);
        }
        if start.elapsed() >= timeout {
            let response = detect_plan_prepass_response(&stdout_live, &stderr_live);
            terminate_process_tree(&mut child, child_pid);
            break (response.is_none(), false, response);
        }
        thread::sleep(Duration::from_millis(200));
    };
    let status = child.wait()?;
    let stdout = collect_stream_tee_output(
        stdout_handle,
        &stdout_path,
        codex_executor_orphan_grace_timeout(),
    )?;
    let stderr = collect_stream_tee_output(
        stderr_handle,
        &stderr_path,
        codex_executor_orphan_grace_timeout(),
    )?;
    Ok(PlanLaneTimedOutput {
        output: std::process::Output {
            status,
            stdout,
            stderr,
        },
        timed_out,
        orphaned,
        response,
    })
}

fn run_command_with_timeout_and_tee(
    command: &mut Command,
    timeout: Duration,
    stall_timeout: Duration,
    no_progress_timeout: Duration,
    scaffold_progress_timeout: Duration,
    orphan_grace_timeout: Duration,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    executor_pid_path: PathBuf,
    scaffold_probe: Option<(&std::path::Path, &Contract)>,
    no_progress_probe: Option<(&std::path::Path, &[EntryPointSnapshot])>,
) -> Result<TimedOutput> {
    #[cfg(unix)]
    command.process_group(0);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let child_pid = child.id();
    write_executor_pid(&executor_pid_path, child_pid)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("child stdout pipe unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("child stderr pipe unavailable"))?;
    let progress = Arc::new(ProgressTracker::new());
    let stdout_handle = spawn_stream_tee(stdout, stdout_path.clone(), progress.clone(), true);
    let stderr_handle = spawn_stream_tee(stderr, stderr_path.clone(), progress.clone(), false);
    let start = Instant::now();
    let (
        timed_out,
        stalled,
        orphaned,
        no_progress_paths,
        scaffold_only_paths,
        post_check_zero_progress_paths,
    ) = loop {
        if child.try_wait()?.is_some() {
            if !wait_for_stream_completion(&stdout_handle, &stderr_handle, orphan_grace_timeout) {
                terminate_process_tree(&mut child, child_pid);
                break (false, false, true, Vec::new(), Vec::new(), Vec::new());
            }
            break (false, false, false, Vec::new(), Vec::new(), Vec::new());
        }
        if start.elapsed() >= timeout {
            terminate_process_tree(&mut child, child_pid);
            break (true, false, false, Vec::new(), Vec::new(), Vec::new());
        }
        if let Some((repo_root, snapshots)) = no_progress_probe {
            if start.elapsed() >= no_progress_timeout
                && !logs_indicate_successful_check_output(&stdout_path, &stderr_path)
                && !logs_indicate_compile_or_check_reason(&stdout_path, &stderr_path)
            {
                let unchanged_paths = unchanged_entry_point_paths(repo_root, snapshots)?;
                if !unchanged_paths.is_empty() && unchanged_paths.len() == snapshots.len() {
                    terminate_process_tree(&mut child, child_pid);
                    break (false, false, false, unchanged_paths, Vec::new(), Vec::new());
                }
            } else if progress.stalled_for(no_progress_timeout)
                && !logs_indicate_successful_check_output(&stdout_path, &stderr_path)
                && !logs_indicate_compile_or_check_reason(&stdout_path, &stderr_path)
            {
                let unchanged_paths = unchanged_entry_point_paths(repo_root, snapshots)?;
                if !unchanged_paths.is_empty() && unchanged_paths.len() == snapshots.len() {
                    terminate_process_tree(&mut child, child_pid);
                    break (false, false, false, unchanged_paths, Vec::new(), Vec::new());
                }
            }
        }
        if let Some((repo_root, contract)) = scaffold_probe {
            if progress.stalled_for(scaffold_progress_timeout)
                && logs_indicate_successful_check_output(&stdout_path, &stderr_path)
            {
                let scaffold_only_paths = scaffold_only_entry_points(repo_root, contract)?;
                if !scaffold_only_paths.is_empty() {
                    terminate_process_tree(&mut child, child_pid);
                    break (
                        false,
                        false,
                        false,
                        Vec::new(),
                        scaffold_only_paths,
                        Vec::new(),
                    );
                }
            }
        }
        if let Some((repo_root, snapshots)) = no_progress_probe {
            if progress.stalled_for(scaffold_progress_timeout)
                && logs_indicate_successful_check_output(&stdout_path, &stderr_path)
            {
                let unchanged_paths = unchanged_entry_point_paths(repo_root, snapshots)?;
                if !unchanged_paths.is_empty() && unchanged_paths.len() == snapshots.len() {
                    terminate_process_tree(&mut child, child_pid);
                    break (false, false, false, Vec::new(), Vec::new(), unchanged_paths);
                }
            }
        }
        if let Some((repo_root, snapshots)) = no_progress_probe {
            if progress.stalled_for(scaffold_progress_timeout)
                && logs_indicate_post_check_zero_progress_tail(&stdout_path, &stderr_path)
            {
                let unchanged_paths = unchanged_entry_point_paths(repo_root, snapshots)?;
                if !unchanged_paths.is_empty() && unchanged_paths.len() == snapshots.len() {
                    terminate_process_tree(&mut child, child_pid);
                    break (false, false, false, Vec::new(), Vec::new(), unchanged_paths);
                }
            }
        }
        if progress.stalled_for(stall_timeout) {
            terminate_process_tree(&mut child, child_pid);
            break (false, true, false, Vec::new(), Vec::new(), Vec::new());
        }
        thread::sleep(Duration::from_millis(200));
    };
    let status = child.wait()?;
    let stdout = collect_stream_tee_output(
        stdout_handle,
        &stdout_path,
        codex_executor_orphan_grace_timeout(),
    )?;
    let stderr = collect_stream_tee_output(
        stderr_handle,
        &stderr_path,
        codex_executor_orphan_grace_timeout(),
    )?;
    Ok(TimedOutput {
        output: std::process::Output {
            status,
            stdout,
            stderr,
        },
        timed_out,
        stalled,
        orphaned,
        no_progress_paths,
        scaffold_only_paths,
        post_check_zero_progress_paths,
    })
}

fn detect_patch_lane_response(
    stdout: &Arc<Mutex<Vec<u8>>>,
    stderr: &Arc<Mutex<Vec<u8>>>,
) -> Option<PatchLaneResponse> {
    let stdout = snapshot_live_output(stdout)?;
    let stderr = snapshot_live_output(stderr)?;
    load_patch_lane_response(&stdout, &stderr).ok()
}

fn detect_plan_prepass_response(
    stdout: &Arc<Mutex<Vec<u8>>>,
    stderr: &Arc<Mutex<Vec<u8>>>,
) -> Option<PlanPrepassResponse> {
    let stdout = snapshot_live_output(stdout)?;
    let stderr = snapshot_live_output(stderr)?;
    load_plan_prepass_response(&stdout, &stderr).ok()
}

fn snapshot_live_output(buffer: &Arc<Mutex<Vec<u8>>>) -> Option<String> {
    let bytes = buffer.lock().ok()?.clone();
    Some(String::from_utf8_lossy(&bytes).to_string())
}

fn spawn_stream_tee_with_live_capture<R>(
    mut reader: R,
    path: PathBuf,
    progress: Arc<ProgressTracker>,
    is_stdout: bool,
    live_capture: Option<Arc<Mutex<Vec<u8>>>>,
) -> thread::JoinHandle<Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut file = fs::File::create(path)?;
        let mut captured = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])?;
            file.flush()?;
            captured.extend_from_slice(&buffer[..read]);
            if let Some(live_capture) = live_capture.as_ref() {
                if let Ok(mut live) = live_capture.lock() {
                    live.extend_from_slice(&buffer[..read]);
                }
            }
            progress.record(read as u64, is_stdout);
        }
        Ok(captured)
    })
}

fn spawn_stream_tee<R>(
    reader: R,
    path: PathBuf,
    progress: Arc<ProgressTracker>,
    is_stdout: bool,
) -> thread::JoinHandle<Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    spawn_stream_tee_with_live_capture(reader, path, progress, is_stdout, None)
}

fn spawn_stream_capture<R>(
    mut reader: R,
    progress: Arc<ProgressTracker>,
    is_stdout: bool,
    live_capture: Option<Arc<Mutex<Vec<u8>>>>,
) -> thread::JoinHandle<Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut captured = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            captured.extend_from_slice(&buffer[..read]);
            if let Some(live_capture) = live_capture.as_ref() {
                if let Ok(mut live) = live_capture.lock() {
                    live.extend_from_slice(&buffer[..read]);
                }
            }
            progress.record(read as u64, is_stdout);
        }
        Ok(captured)
    })
}

fn join_stream_tee(handle: thread::JoinHandle<Result<Vec<u8>>>) -> Result<Vec<u8>> {
    handle
        .join()
        .map_err(|_| anyhow!("stream tee thread panicked"))?
}

fn collect_stream_tee_output(
    handle: thread::JoinHandle<Result<Vec<u8>>>,
    path: &std::path::Path,
    grace_timeout: Duration,
) -> Result<Vec<u8>> {
    let start = Instant::now();
    loop {
        if handle.is_finished() {
            return join_stream_tee(handle);
        }
        if start.elapsed() >= grace_timeout {
            return match fs::read(path) {
                Ok(bytes) => Ok(bytes),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
                Err(err) => Err(err.into()),
            };
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn collect_stream_capture_output(
    handle: thread::JoinHandle<Result<Vec<u8>>>,
    live_capture: &Arc<Mutex<Vec<u8>>>,
    grace_timeout: Duration,
) -> Result<Vec<u8>> {
    let start = Instant::now();
    loop {
        if handle.is_finished() {
            return join_stream_tee(handle);
        }
        if start.elapsed() >= grace_timeout {
            return Ok(live_capture
                .lock()
                .map(|bytes| bytes.clone())
                .unwrap_or_default());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn last_non_empty_line(text: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn timeout_summary(timeout: Duration, stdout: &str, stderr: &str) -> String {
    format!(
        "codex command timed out after {}s{}",
        timeout.as_secs(),
        last_non_empty_line(stderr)
            .or_else(|| last_non_empty_line(stdout))
            .map(|line| format!(": {line}"))
            .unwrap_or_default()
    )
}

fn stall_summary(timeout: Duration, stdout: &str, stderr: &str) -> String {
    format!(
        "codex command stalled after {}s without output progress{}",
        timeout.as_secs(),
        last_non_empty_line(stderr)
            .or_else(|| last_non_empty_line(stdout))
            .map(|line| format!(": {line}"))
            .unwrap_or_default()
    )
}

fn orphan_summary(stdout: &str, stderr: &str) -> String {
    format!(
        "codex command detached or orphaned nested processes{}",
        last_non_empty_line(stderr)
            .or_else(|| last_non_empty_line(stdout))
            .map(|line| format!(": {line}"))
            .unwrap_or_default()
    )
}

fn logs_indicate_successful_check_output(
    stdout_path: &std::path::Path,
    stderr_path: &std::path::Path,
) -> bool {
    let stdout = read_tail_text(stdout_path, 1024).unwrap_or_default();
    let stderr = read_tail_text(stderr_path, 1024).unwrap_or_default();
    output_indicates_successful_check_output(&stdout, &stderr)
}

fn logs_indicate_compile_or_check_reason(
    stdout_path: &std::path::Path,
    stderr_path: &std::path::Path,
) -> bool {
    let stdout = read_tail_text(stdout_path, 1024).unwrap_or_default();
    let stderr = read_tail_text(stderr_path, 1024).unwrap_or_default();
    let combined = format!("{stdout}\n{stderr}");
    combined.contains("Compiling ")
        || combined.contains("Running unittests")
        || combined.contains("Doc-tests ")
        || combined.contains("/bin/zsh -lc 'cargo ")
        || combined.contains("/bin/bash -lc 'cargo ")
        || combined.contains("error[")
        || combined.contains("error:")
        || blocked_execution_line(&stdout, &stderr).is_some()
        || successful_execution_line(&stdout, &stderr).is_some()
}

fn logs_indicate_post_check_zero_progress_tail(
    stdout_path: &std::path::Path,
    stderr_path: &std::path::Path,
) -> bool {
    let stdout = read_tail_text(stdout_path, 1024).unwrap_or_default();
    let stderr = read_tail_text(stderr_path, 1024).unwrap_or_default();
    output_indicates_post_check_zero_progress_tail(&stdout, &stderr)
}

fn output_indicates_successful_check_output(stdout: &str, stderr: &str) -> bool {
    let combined = format!("{stdout}\n{stderr}");
    combined.contains("test result: ok.")
        || combined.contains("Finished `test` profile")
        || combined.contains("Finished `dev` profile")
        || (combined.contains("succeeded in ")
            && (combined.contains("cargo test")
                || combined.contains("cargo check")
                || combined.contains("cargo build")
                || combined.contains("Running unittests")
                || combined.contains("0 tests, 0 benchmarks")
                || combined.contains("0 passed; 0 failed;")))
}

fn output_indicates_post_check_zero_progress_tail(stdout: &str, stderr: &str) -> bool {
    let combined = format!("{stdout}\n{stderr}");
    combined.contains("0 tests, 0 benchmarks")
        || combined.contains("0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out")
        || combined.contains("0 passed; 0 failed;")
}

fn read_tail_text(path: &std::path::Path, max_bytes: usize) -> Result<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    let bytes = fs::read(path).with_context(|| format!("read log tail {}", path.display()))?;
    let start = bytes.len().saturating_sub(max_bytes);
    Ok(String::from_utf8_lossy(&bytes[start..]).to_string())
}

fn scaffold_only_summary(paths: &[String], stdout: &str, stderr: &str) -> String {
    format!(
        "no implementation progress beyond scaffold in {}{}",
        paths.join(", "),
        last_non_empty_line(stderr)
            .or_else(|| last_non_empty_line(stdout))
            .map(|line| format!(": {line}"))
            .unwrap_or_default()
    )
}

fn no_progress_after_dispatch_summary(paths: &[String], stdout: &str, stderr: &str) -> String {
    format!(
        "no implementation progress after bounded context dispatch in {}{}",
        paths.join(", "),
        last_non_empty_line(stderr)
            .or_else(|| last_non_empty_line(stdout))
            .map(|line| format!(": {line}"))
            .unwrap_or_default()
    )
}

fn post_check_zero_progress_summary(paths: &[String], stdout: &str, stderr: &str) -> String {
    format!(
        "no implementation progress after post-check stall in {}{}",
        paths.join(", "),
        last_non_empty_line(stderr)
            .or_else(|| last_non_empty_line(stdout))
            .map(|line| format!(": {line}"))
            .unwrap_or_default()
    )
}

fn classify_scaffold_only_result(paths: &[String], stdout: &str, stderr: &str) -> (bool, String) {
    if let Some(blocked) = blocked_execution_line(stdout, stderr) {
        return (false, blocked);
    }
    (false, scaffold_only_summary(paths, stdout, stderr))
}

fn classify_no_progress_after_dispatch_result(
    paths: &[String],
    stdout: &str,
    stderr: &str,
) -> (bool, String) {
    if let Some(blocked) = blocked_execution_line(stdout, stderr) {
        return (false, blocked);
    }
    if logs_indicate_missing_manifest_wiring(stdout, stderr, paths) {
        return (
            false,
            format!(
                "PUNK_EXECUTION_BLOCKED: missing manifest wiring for {}",
                paths.join(", ")
            ),
        );
    }
    (
        false,
        no_progress_after_dispatch_summary(paths, stdout, stderr),
    )
}

fn classify_post_check_zero_progress_result(
    paths: &[String],
    stdout: &str,
    stderr: &str,
) -> (bool, String) {
    if let Some(blocked) = blocked_execution_line(stdout, stderr) {
        return (false, blocked);
    }
    (
        false,
        post_check_zero_progress_summary(paths, stdout, stderr),
    )
}

fn reclassify_stalled_post_check_zero_progress(
    repo_root: &std::path::Path,
    snapshots: &[EntryPointSnapshot],
    mut timed_output: TimedOutput,
) -> Result<TimedOutput> {
    if !timed_output.stalled
        || !timed_output.no_progress_paths.is_empty()
        || !timed_output.scaffold_only_paths.is_empty()
        || !timed_output.post_check_zero_progress_paths.is_empty()
        || snapshots.is_empty()
    {
        return Ok(timed_output);
    }

    let stdout = String::from_utf8_lossy(&timed_output.output.stdout);
    let stderr = String::from_utf8_lossy(&timed_output.output.stderr);
    let unchanged_paths = unchanged_entry_point_paths(repo_root, snapshots)?;
    if unchanged_paths.is_empty() || unchanged_paths.len() != snapshots.len() {
        return Ok(timed_output);
    }

    timed_output.stalled = false;
    if output_indicates_successful_check_output(&stdout, &stderr)
        || output_indicates_post_check_zero_progress_tail(&stdout, &stderr)
    {
        timed_output.post_check_zero_progress_paths = unchanged_paths;
    } else {
        timed_output.no_progress_paths = unchanged_paths;
    }
    Ok(timed_output)
}

fn blocked_execution_template() -> &'static str {
    "PUNK_EXECUTION_BLOCKED: <reason>"
}

fn successful_execution_template() -> &'static str {
    "PUNK_EXECUTION_COMPLETE: <summary>"
}

fn blocked_execution_line(stdout: &str, stderr: &str) -> Option<String> {
    [stdout, stderr].into_iter().find_map(|stream| {
        stream.lines().find_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with(BLOCKED_EXECUTION_SENTINEL) {
                Some(trimmed.to_string())
            } else {
                None
            }
        })
    })
}

fn successful_execution_line(stdout: &str, stderr: &str) -> Option<String> {
    [stdout, stderr].into_iter().find_map(|stream| {
        stream.lines().find_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with(SUCCESSFUL_EXECUTION_SENTINEL) {
                Some(trimmed.to_string())
            } else {
                None
            }
        })
    })
}

fn classify_execution_result(exit_success: bool, stdout: &str, stderr: &str) -> (bool, String) {
    let blocked_summary = blocked_execution_line(stdout, stderr);
    let success_summary = successful_execution_line(stdout, stderr);
    let summary = blocked_summary
        .clone()
        .or_else(|| success_summary.clone())
        .or_else(|| last_non_empty_line(stdout))
        .or_else(|| last_non_empty_line(stderr))
        .unwrap_or_else(|| {
            if exit_success {
                "Codex run completed".to_string()
            } else {
                "Codex run failed".to_string()
            }
        });
    (
        exit_success && blocked_summary.is_none() && success_summary.is_some(),
        summary,
    )
}

fn classify_timeout_result(stdout: &str, stderr: &str, timeout: Duration) -> (bool, String) {
    if let Some(blocked) = blocked_execution_line(stdout, stderr) {
        return (false, blocked);
    }
    if let Some(success) = successful_execution_line(stdout, stderr) {
        return (true, success);
    }
    (false, timeout_summary(timeout, stdout, stderr))
}

fn classify_stall_result(stdout: &str, stderr: &str, stall_timeout: Duration) -> (bool, String) {
    if let Some(blocked) = blocked_execution_line(stdout, stderr) {
        return (false, blocked);
    }
    if let Some(success) = successful_execution_line(stdout, stderr) {
        return (true, success);
    }
    (false, stall_summary(stall_timeout, stdout, stderr))
}

fn classify_orphan_result(stdout: &str, stderr: &str) -> (bool, String) {
    if let Some(blocked) = blocked_execution_line(stdout, stderr) {
        return (false, blocked);
    }
    (false, orphan_summary(stdout, stderr))
}

impl ProgressTracker {
    fn new() -> Self {
        Self {
            stdout_bytes: AtomicU64::new(0),
            stderr_bytes: AtomicU64::new(0),
            last_progress: Mutex::new(Instant::now()),
        }
    }

    fn record(&self, bytes: u64, is_stdout: bool) {
        if bytes == 0 {
            return;
        }
        if is_stdout {
            self.stdout_bytes.fetch_add(bytes, Ordering::Relaxed);
        } else {
            self.stderr_bytes.fetch_add(bytes, Ordering::Relaxed);
        }
        if let Ok(mut last_progress) = self.last_progress.lock() {
            *last_progress = Instant::now();
        }
    }

    fn stalled_for(&self, stall_timeout: Duration) -> bool {
        if stall_timeout.is_zero() {
            return false;
        }
        self.last_progress
            .lock()
            .map(|last_progress| last_progress.elapsed() >= stall_timeout)
            .unwrap_or(false)
    }
}

fn wait_for_stream_completion(
    stdout_handle: &thread::JoinHandle<Result<Vec<u8>>>,
    stderr_handle: &thread::JoinHandle<Result<Vec<u8>>>,
    grace_timeout: Duration,
) -> bool {
    let start = Instant::now();
    loop {
        if stdout_handle.is_finished() && stderr_handle.is_finished() {
            return true;
        }
        if start.elapsed() >= grace_timeout {
            return false;
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn terminate_process_tree(child: &mut std::process::Child, child_pid: u32) {
    #[cfg(unix)]
    {
        let process_group = format!("-{}", child_pid);
        let _ = Command::new("kill")
            .args(["-TERM", process_group.as_str()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        thread::sleep(Duration::from_millis(150));
        let _ = Command::new("kill")
            .args(["-KILL", process_group.as_str()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

fn write_executor_pid(path: &std::path::Path, child_pid: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "child_pid": child_pid,
            "process_group_id": child_pid,
        }))?,
    )?;
    Ok(())
}

fn capture_entry_point_snapshots(
    repo_root: &std::path::Path,
    contract: &Contract,
    extra_paths: &[String],
) -> Result<Vec<EntryPointSnapshot>> {
    let mut snapshots = Vec::new();
    let mut paths = contract.entry_points.clone();
    extend_unique_paths(&mut paths, extra_paths);
    let mut remaining_scope_files = 64usize;
    for scope in &contract.allowed_scope {
        if is_file_like_scope(scope) {
            continue;
        }
        if remaining_scope_files == 0 {
            break;
        }
        let discovered =
            collect_existing_allowed_scope_files(repo_root, scope, &mut remaining_scope_files)?;
        extend_unique_paths(&mut paths, &discovered);
    }
    for entry_point in &paths {
        if !is_file_like_scope(entry_point) {
            continue;
        }
        if !path_is_in_allowed_scope(entry_point, &contract.allowed_scope) {
            continue;
        }
        let file_path = repo_root.join(entry_point);
        snapshots.push(EntryPointSnapshot {
            path: entry_point.clone(),
            content: if file_path.exists() {
                Some(
                    fs::read_to_string(&file_path)
                        .with_context(|| format!("read entry point snapshot {entry_point}"))?,
                )
            } else {
                None
            },
        });
    }
    Ok(snapshots)
}

fn collect_existing_allowed_scope_files(
    repo_root: &std::path::Path,
    scope: &str,
    remaining: &mut usize,
) -> Result<Vec<String>> {
    let mut collected = Vec::new();
    let root = repo_root.join(scope);
    if !root.exists() || *remaining == 0 {
        return Ok(collected);
    }
    collect_existing_allowed_scope_files_recursive(repo_root, &root, remaining, &mut collected)?;
    collected.sort();
    Ok(collected)
}

fn collect_existing_allowed_scope_files_recursive(
    repo_root: &std::path::Path,
    current: &std::path::Path,
    remaining: &mut usize,
    collected: &mut Vec<String>,
) -> Result<()> {
    if *remaining == 0 {
        return Ok(());
    }
    let metadata = match fs::metadata(current) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(()),
    };
    if metadata.is_file() {
        if let Ok(relative) = current.strip_prefix(repo_root) {
            collected.push(relative.to_string_lossy().replace('\\', "/"));
            *remaining = remaining.saturating_sub(1);
        }
        return Ok(());
    }
    if !metadata.is_dir() {
        return Ok(());
    }

    let mut entries =
        fs::read_dir(current)?.collect::<std::result::Result<Vec<_>, std::io::Error>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        if *remaining == 0 {
            break;
        }
        collect_existing_allowed_scope_files_recursive(
            repo_root,
            &entry.path(),
            remaining,
            collected,
        )?;
    }
    Ok(())
}

fn unchanged_entry_point_paths(
    repo_root: &std::path::Path,
    snapshots: &[EntryPointSnapshot],
) -> Result<Vec<String>> {
    let mut unchanged = Vec::new();
    for snapshot in snapshots {
        let file_path = repo_root.join(&snapshot.path);
        let current =
            if file_path.exists() {
                Some(fs::read_to_string(&file_path).with_context(|| {
                    format!("read entry point progress probe {}", snapshot.path)
                })?)
            } else {
                None
            };
        if current == snapshot.content {
            unchanged.push(snapshot.path.clone());
        }
    }
    Ok(unchanged)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApplyPatchUpdate {
    path: String,
    hunks: Vec<ApplyPatchHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApplyPatchHunk {
    header: Option<String>,
    lines: Vec<String>,
    eof: bool,
}

fn load_patch_lane_response(stdout: &str, stderr: &str) -> Result<PatchLaneResponse> {
    if let Some(blocked) = blocked_execution_line(stdout, stderr) {
        return Ok(PatchLaneResponse::Blocked(blocked));
    }

    extract_apply_patch_envelope(stdout)
        .or_else(|| extract_apply_patch_envelope(stderr))
        .or_else(|| extract_apply_patch_envelope(&format!("{stdout}\n{stderr}")))
        .map(PatchLaneResponse::Patch)
        .ok_or_else(|| {
            anyhow!("patch lane returned no complete patch artifact or blocked sentinel")
        })
}

fn load_plan_prepass_response(stdout: &str, stderr: &str) -> Result<PlanPrepassResponse> {
    if let Some(blocked) = blocked_execution_line(stdout, stderr) {
        return Ok(PlanPrepassResponse::Blocked(blocked));
    }

    let payload = extract_plan_envelope(stdout)
        .or_else(|| extract_plan_envelope(stderr))
        .or_else(|| extract_plan_envelope(&format!("{stdout}\n{stderr}")))
        .ok_or_else(|| anyhow!("patch prepass returned no complete plan artifact"))?;
    parse_plan_prepass_response(&payload)
}

fn extract_plan_envelope(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut begin = None;
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if begin.is_none() {
            if trimmed == "PUNK_PLAN_BEGIN" {
                begin = Some(idx);
            }
            continue;
        }
        if trimmed == "PUNK_PLAN_END" {
            let body = lines[begin? + 1..idx].join("\n");
            let body = body.trim().to_string();
            if body.is_empty() {
                return None;
            }
            return Some(body);
        }
    }
    None
}

fn parse_plan_prepass_response(payload: &str) -> Result<PlanPrepassResponse> {
    let mut summary = None;
    let mut targets = Vec::new();
    let mut current_path = None;
    let mut current_symbol = None;
    let mut current_insert = None;
    let mut current_sketch = None;

    for raw_line in payload.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("SUMMARY: ") {
            summary = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("TARGET: ") {
            if let (Some(path), Some(symbol), Some(insertion_point), Some(execution_sketch)) = (
                current_path.take(),
                current_symbol.take(),
                current_insert.take(),
                current_sketch.take(),
            ) {
                targets.push(PlanPrepassTarget {
                    path,
                    symbol,
                    insertion_point,
                    execution_sketch,
                });
            }
            current_path = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("SYMBOL: ") {
            current_symbol = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("INSERT: ") {
            current_insert = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("SKETCH: ") {
            current_sketch = Some(rest.trim().to_string());
            continue;
        }
        return Err(anyhow!("unexpected plan prepass line `{line}`"));
    }

    if let (Some(path), Some(symbol), Some(insertion_point), Some(execution_sketch)) = (
        current_path.take(),
        current_symbol.take(),
        current_insert.take(),
        current_sketch.take(),
    ) {
        targets.push(PlanPrepassTarget {
            path,
            symbol,
            insertion_point,
            execution_sketch,
        });
    }

    let summary = summary.ok_or_else(|| anyhow!("plan prepass missing SUMMARY line"))?;
    if targets.is_empty() {
        return Err(anyhow!("plan prepass declared no targets"));
    }
    if targets.len() > 3 {
        return Err(anyhow!("plan prepass declared too many targets"));
    }
    Ok(PlanPrepassResponse::Plan { summary, targets })
}

fn extract_apply_patch_envelope(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let (start, end) = find_apply_patch_boundaries(&lines)?;
    let patch = lines[start..=end].join("\n");
    let patch = normalize_patch_text(&patch);
    if patch.trim().is_empty() {
        None
    } else {
        Some(patch)
    }
}

fn find_apply_patch_boundaries(lines: &[&str]) -> Option<(usize, usize)> {
    let mut begin = None;
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if begin.is_none() {
            if trimmed == "*** Begin Patch" {
                begin = Some(idx);
            }
            continue;
        }
        if trimmed == "*** End Patch" {
            return Some((begin.expect("begin set above"), idx));
        }
    }
    None
}

fn normalize_patch_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut normalized = trimmed.to_string();
    if !normalized.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

fn validate_patch_scope(patch: &str, allowed_scope: &[String]) -> Result<Vec<ApplyPatchUpdate>> {
    let updates = parse_apply_patch_updates(patch)?;
    if updates.is_empty() {
        return Err(anyhow!("patch did not declare any modified file paths"));
    }
    for update in &updates {
        if !path_is_in_allowed_scope(&update.path, allowed_scope) {
            return Err(anyhow!("patch touched out-of-scope path {}", update.path));
        }
    }
    Ok(updates)
}

fn validate_plan_prepass_scope(
    response: &PlanPrepassResponse,
    allowed_scope: &[String],
) -> Result<()> {
    let PlanPrepassResponse::Plan { targets, .. } = response else {
        return Ok(());
    };
    for target in targets {
        if !path_is_in_allowed_scope(&target.path, allowed_scope) {
            return Err(anyhow!(
                "plan prepass touched out-of-scope path {}",
                target.path
            ));
        }
        if target.symbol.trim().is_empty() || target.insertion_point.trim().is_empty() {
            return Err(anyhow!(
                "plan prepass requires non-empty symbol and insertion point"
            ));
        }
    }
    Ok(())
}

fn parse_apply_patch_updates(patch: &str) -> Result<Vec<ApplyPatchUpdate>> {
    let patch = normalize_patch_text(patch);
    let lines: Vec<&str> = patch.lines().collect();
    let (begin, end) = find_apply_patch_boundaries(&lines)
        .ok_or_else(|| anyhow!("patch envelope is incomplete"))?;
    if begin != 0 || end + 1 != lines.len() {
        return Err(anyhow!(
            "patch lane output must contain only one apply_patch envelope"
        ));
    }

    let mut updates = Vec::new();
    let mut i = begin + 1;
    while i < end {
        let line = lines[i].trim_end();
        if let Some(path) = line.strip_prefix("*** Update File: ") {
            if Path::new(path).is_absolute() {
                return Err(anyhow!("patch path must be repo-relative: {path}"));
            }
            i += 1;
            if i < end && lines[i].trim_end().starts_with("*** Move to: ") {
                return Err(anyhow!(
                    "patch/apply lane only accepts existing-file updates"
                ));
            }
            let mut hunks = Vec::new();
            while i < end {
                let line = lines[i].trim_end();
                if line.starts_with("*** Update File: ")
                    || line.starts_with("*** Add File: ")
                    || line.starts_with("*** Delete File: ")
                {
                    break;
                }
                if !line.starts_with("@@") {
                    return Err(anyhow!("expected hunk header for {path}, got `{line}`"));
                }
                let header = line
                    .strip_prefix("@@")
                    .map(str::trim)
                    .filter(|header| !header.is_empty())
                    .map(|header| header.to_string());
                i += 1;
                let mut hunk_lines = Vec::new();
                let mut eof = false;
                while i < end {
                    let line = lines[i];
                    let trimmed = line.trim_end();
                    if trimmed.starts_with("@@")
                        || trimmed.starts_with("*** Update File: ")
                        || trimmed.starts_with("*** Add File: ")
                        || trimmed.starts_with("*** Delete File: ")
                    {
                        break;
                    }
                    if trimmed == "*** End of File" {
                        eof = true;
                        i += 1;
                        break;
                    }
                    match line.chars().next() {
                        Some(' ') | Some('+') | Some('-') => hunk_lines.push(line.to_string()),
                        _ => return Err(anyhow!("invalid hunk line for {}: `{}`", path, line)),
                    }
                    i += 1;
                }
                if hunk_lines.is_empty() {
                    return Err(anyhow!("empty hunk body for {path}"));
                }
                hunks.push(ApplyPatchHunk {
                    header,
                    lines: hunk_lines,
                    eof,
                });
            }
            if hunks.is_empty() {
                return Err(anyhow!("update file section for {path} had no hunks"));
            }
            updates.push(ApplyPatchUpdate {
                path: path.to_string(),
                hunks,
            });
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            return Err(anyhow!(
                "patch/apply lane only accepts existing-file updates, got add file {path}"
            ));
        }
        if let Some(path) = line.strip_prefix("*** Delete File: ") {
            return Err(anyhow!(
                "patch/apply lane only accepts existing-file updates, got delete file {path}"
            ));
        }
        return Err(anyhow!("unexpected patch line `{line}`"));
    }

    Ok(updates)
}

fn apply_patch_in_repo(repo_root: &Path, updates: &[ApplyPatchUpdate]) -> Result<()> {
    for update in updates {
        apply_update_in_repo(repo_root, update)?;
    }
    Ok(())
}

fn apply_update_in_repo(repo_root: &Path, update: &ApplyPatchUpdate) -> Result<()> {
    let file_path = repo_root.join(&update.path);
    if !file_path.exists() {
        return Err(anyhow!("patch target does not exist: {}", update.path));
    }
    let original = fs::read_to_string(&file_path)
        .with_context(|| format!("read patch target {}", file_path.display()))?;
    let had_trailing_newline = original.ends_with('\n');
    let mut lines: Vec<String> = original.lines().map(|line| line.to_string()).collect();
    let mut cursor = 0usize;
    for hunk in &update.hunks {
        apply_hunk_to_lines(&mut lines, hunk, &mut cursor)
            .with_context(|| format!("apply hunk in {}", update.path))?;
    }
    let mut new_content = lines.join("\n");
    if had_trailing_newline || !new_content.is_empty() {
        new_content.push('\n');
    }
    fs::write(&file_path, new_content)
        .with_context(|| format!("write patch target {}", file_path.display()))?;
    Ok(())
}

fn apply_hunk_to_lines(
    file_lines: &mut Vec<String>,
    hunk: &ApplyPatchHunk,
    cursor: &mut usize,
) -> Result<()> {
    let old_lines = apply_patch_old_lines(&hunk.lines);
    let new_lines = apply_patch_new_lines(&hunk.lines);
    let position = locate_hunk_position(
        file_lines,
        &old_lines,
        *cursor,
        hunk.eof,
        hunk.header.as_deref(),
    );
    if let Some(position) = position {
        let end = position + old_lines.len();
        file_lines.splice(position..end, new_lines);
        *cursor = position + apply_patch_new_line_count(&hunk.lines);
        return Ok(());
    }

    if is_addition_only_hunk(&hunk.lines) {
        if let Some(positions) =
            locate_ordered_subsequence_positions(file_lines, &old_lines, *cursor)
        {
            let insert_at = positions
                .last()
                .copied()
                .unwrap_or(*cursor)
                .saturating_add(1);
            let added_lines = apply_patch_added_lines(&hunk.lines);
            file_lines.splice(insert_at..insert_at, added_lines.clone());
            *cursor = insert_at + added_lines.len();
            return Ok(());
        }
    }

    Err(anyhow!("unable to locate hunk context"))
}

fn apply_patch_old_lines(hunk_lines: &[String]) -> Vec<String> {
    hunk_lines
        .iter()
        .filter_map(|line| match line.chars().next() {
            Some(' ') | Some('-') => Some(line[1..].to_string()),
            _ => None,
        })
        .collect()
}

fn apply_patch_new_lines(hunk_lines: &[String]) -> Vec<String> {
    hunk_lines
        .iter()
        .filter_map(|line| match line.chars().next() {
            Some(' ') | Some('+') => Some(line[1..].to_string()),
            _ => None,
        })
        .collect()
}

fn apply_patch_new_line_count(hunk_lines: &[String]) -> usize {
    hunk_lines
        .iter()
        .filter(|line| matches!(line.chars().next(), Some(' ') | Some('+')))
        .count()
}

fn apply_patch_added_lines(hunk_lines: &[String]) -> Vec<String> {
    hunk_lines
        .iter()
        .filter_map(|line| match line.chars().next() {
            Some('+') => Some(line[1..].to_string()),
            _ => None,
        })
        .collect()
}

fn is_addition_only_hunk(hunk_lines: &[String]) -> bool {
    let has_add = hunk_lines
        .iter()
        .any(|line| matches!(line.chars().next(), Some('+')));
    let has_remove = hunk_lines
        .iter()
        .any(|line| matches!(line.chars().next(), Some('-')));
    has_add && !has_remove
}

fn locate_hunk_position(
    file_lines: &[String],
    old_lines: &[String],
    start: usize,
    eof: bool,
    _header: Option<&str>,
) -> Option<usize> {
    if old_lines.is_empty() {
        return Some(if eof {
            file_lines.len()
        } else {
            start.min(file_lines.len())
        });
    }
    seek_sequence(file_lines, old_lines, start, eof)
}

fn seek_sequence(haystack: &[String], needle: &[String], start: usize, eof: bool) -> Option<usize> {
    if needle.is_empty() {
        return Some(start.min(haystack.len()));
    }
    if needle.len() > haystack.len() {
        return None;
    }
    let exact = matching_sequence_positions(haystack, needle, |left, right| left == right);
    if let Some(position) =
        choose_sequence_position(&exact, needle.len(), start, eof, haystack.len())
    {
        return Some(position);
    }
    let trimmed_end = matching_sequence_positions(haystack, needle, |left, right| {
        left.trim_end() == right.trim_end()
    });
    if let Some(position) =
        choose_sequence_position(&trimmed_end, needle.len(), start, eof, haystack.len())
    {
        return Some(position);
    }
    let trimmed =
        matching_sequence_positions(haystack, needle, |left, right| left.trim() == right.trim());
    choose_sequence_position(&trimmed, needle.len(), start, eof, haystack.len())
}

fn matching_sequence_positions<F>(haystack: &[String], needle: &[String], matcher: F) -> Vec<usize>
where
    F: Fn(&str, &str) -> bool,
{
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    let mut positions = Vec::new();
    for start in 0..=haystack.len() - needle.len() {
        if needle
            .iter()
            .enumerate()
            .all(|(offset, line)| matcher(&haystack[start + offset], line))
        {
            positions.push(start);
        }
    }
    positions
}

fn choose_sequence_position(
    positions: &[usize],
    needle_len: usize,
    start: usize,
    eof: bool,
    haystack_len: usize,
) -> Option<usize> {
    if positions.is_empty() {
        return None;
    }
    if eof {
        if let Some(position) = positions
            .iter()
            .copied()
            .find(|position| position + needle_len == haystack_len)
        {
            return Some(position);
        }
    }
    if let Some(position) = positions
        .iter()
        .copied()
        .find(|position| *position >= start)
    {
        return Some(position);
    }
    if positions.len() == 1 {
        return positions.first().copied();
    }
    None
}

fn locate_ordered_subsequence_positions(
    haystack: &[String],
    needle: &[String],
    start: usize,
) -> Option<Vec<usize>> {
    if needle.is_empty() {
        return Some(Vec::new());
    }
    let first = needle.first()?.trim();
    let mut starts = haystack
        .iter()
        .enumerate()
        .skip(start)
        .filter_map(|(idx, line)| (line.trim() == first).then_some(idx))
        .collect::<Vec<_>>();
    if starts.is_empty() && start > 0 {
        starts = haystack
            .iter()
            .enumerate()
            .filter_map(|(idx, line)| (line.trim() == first).then_some(idx))
            .collect();
    }
    for candidate_start in starts {
        let mut positions = vec![candidate_start];
        let mut cursor = candidate_start + 1;
        let mut matched = true;
        for expected in needle.iter().skip(1) {
            let expected = expected.trim();
            if let Some((idx, _)) = haystack
                .iter()
                .enumerate()
                .skip(cursor)
                .find(|(_, line)| line.trim() == expected)
            {
                positions.push(idx);
                cursor = idx + 1;
            } else {
                matched = false;
                break;
            }
        }
        if matched {
            if let (Some(first_idx), Some(last_idx)) = (positions.first(), positions.last()) {
                if last_idx.saturating_sub(*first_idx) <= needle.len() + 8 {
                    return Some(positions);
                }
            }
        }
    }
    None
}

fn collect_contract_checks(contract: &Contract) -> Vec<String> {
    let mut checks = Vec::new();
    for check in contract
        .target_checks
        .iter()
        .chain(contract.integrity_checks.iter())
    {
        let trimmed = check.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !checks.iter().any(|existing| existing == trimmed) {
            checks.push(trimmed.to_string());
        }
    }
    checks
}

fn run_contract_checks(
    repo_root: &Path,
    contract: &Contract,
    checks: &[String],
    stdout_path: &Path,
    stderr_path: &Path,
) -> std::result::Result<Vec<String>, String> {
    let git_guard = GitGuardEnv::install().map_err(|err| err.to_string())?;
    let mut checks_run = Vec::new();
    for check in checks {
        append_log_text(stdout_path, &format!("\n[punk check] {check}\n"))
            .map_err(|err| err.to_string())?;
        let mut command = Command::new("/bin/zsh");
        command
            .arg("-lc")
            .arg(check)
            .current_dir(resolve_check_workdir(repo_root, contract, check));
        if let Some(git_guard) = git_guard.as_ref() {
            git_guard.apply(&mut command);
        }
        let output = run_command_with_timeout(&mut command, codex_executor_timeout())
            .map_err(|err| format!("failed to run check `{check}`: {err}"))?;
        append_log_bytes(stdout_path, &output.output.stdout).map_err(|err| err.to_string())?;
        append_log_bytes(stderr_path, &output.output.stderr).map_err(|err| err.to_string())?;
        if output.timed_out {
            return Err(format!(
                "patch/apply lane check timed out: {check}{}",
                last_non_empty_line(&String::from_utf8_lossy(&output.output.stderr))
                    .or_else(|| last_non_empty_line(&String::from_utf8_lossy(
                        &output.output.stdout
                    )))
                    .map(|line| format!(": {line}"))
                    .unwrap_or_default()
            ));
        }
        if !output.output.status.success() {
            let stdout = String::from_utf8_lossy(&output.output.stdout);
            let stderr = String::from_utf8_lossy(&output.output.stderr);
            return Err(format!(
                "patch/apply lane check failed: {check}{}",
                last_non_empty_line(&stderr)
                    .or_else(|| last_non_empty_line(&stdout))
                    .map(|line| format!(": {line}"))
                    .unwrap_or_default()
            ));
        }
        checks_run.push(check.clone());
    }
    Ok(checks_run)
}

fn resolve_check_workdir(repo_root: &Path, contract: &Contract, check: &str) -> PathBuf {
    let trimmed = check.trim();
    if !trimmed.starts_with("cargo ")
        || trimmed.contains("cd ")
        || trimmed.contains("&&")
        || trimmed.contains(';')
    {
        return repo_root.to_path_buf();
    }
    infer_scoped_cargo_root(repo_root, contract).unwrap_or_else(|| repo_root.to_path_buf())
}

fn infer_scoped_cargo_root(repo_root: &Path, contract: &Contract) -> Option<PathBuf> {
    let mut roots = Vec::new();
    for rel_path in contract
        .entry_points
        .iter()
        .chain(contract.allowed_scope.iter())
    {
        let Some(root) = outermost_cargo_root_for_scope_path(repo_root, rel_path) else {
            continue;
        };
        if !roots.iter().any(|existing: &PathBuf| existing == &root) {
            roots.push(root);
        }
    }
    if roots.len() == 1 {
        roots.into_iter().next()
    } else {
        None
    }
}

fn outermost_cargo_root_for_scope_path(repo_root: &Path, rel_path: &str) -> Option<PathBuf> {
    let joined = repo_root.join(rel_path);
    let mut cursor = if joined.is_dir() {
        joined.as_path()
    } else {
        joined.parent()?
    };
    let mut candidate = None;
    loop {
        if cursor == repo_root && candidate.is_some() {
            break;
        }
        if cursor.join("Cargo.toml").exists() {
            candidate = Some(cursor.to_path_buf());
        }
        if cursor == repo_root {
            break;
        }
        cursor = cursor.parent()?;
        if !cursor.starts_with(repo_root) {
            break;
        }
    }
    candidate
}

fn append_log_text(path: &Path, text: &str) -> Result<()> {
    append_log_bytes(path, text.as_bytes())
}

fn append_log_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open log {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("append log {}", path.display()))?;
    file.flush()
        .with_context(|| format!("flush log {}", path.display()))?;
    Ok(())
}

fn path_is_in_allowed_scope(path: &str, allowed_scope: &[String]) -> bool {
    allowed_scope.iter().any(|scope| {
        path == scope
            || path
                .strip_prefix(scope)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

fn is_file_like_scope(path: &str) -> bool {
    std::path::Path::new(path).extension().is_some()
        || matches!(
            path,
            "Cargo.toml" | "Cargo.lock" | "README.md" | "go.mod" | "go.sum" | "pyproject.toml"
        )
}

fn find_binary_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate = dir.join(format!("{name}.exe"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn sh_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub struct FailingExecutor;

impl Executor for FailingExecutor {
    fn name(&self) -> &'static str {
        "failing"
    }
    fn execute_contract(&self, _input: ExecuteInput) -> Result<ExecuteOutput> {
        Err(anyhow!("forced failure"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use punk_domain::RepoScanSummary;
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::time::Instant;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn build_exec_prompt_mentions_scope_when_present() {
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["public fn x".into()],
            behavior_requirements: vec!["do x".into()],
            allowed_scope: vec!["src".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        let prompt = build_exec_prompt(&contract, None, &[]);
        assert!(prompt.contains("Allowed scope: src"));
        assert!(prompt.contains("Start by inspecting only the listed entry points"));
        assert!(prompt.contains("Do not perform broad repo-wide search."));
        assert!(prompt.contains(blocked_execution_template()));
        assert!(prompt.contains(successful_execution_template()));
    }

    #[test]
    fn drafter_attempt_timeouts_reserve_retry_budget() {
        let (primary, retry) = drafter_attempt_timeouts(Duration::from_secs(30), true);
        assert_eq!(primary, Duration::from_secs(20));
        assert_eq!(retry, Some(Duration::from_secs(10)));

        let (primary, retry) = drafter_attempt_timeouts(Duration::from_secs(1), true);
        assert_eq!(primary, Duration::from_secs(1));
        assert_eq!(retry, None);
    }

    #[test]
    fn codex_drafter_timeout_is_total_budget_across_retry_attempts() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-drafter-timeout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).unwrap();
        let fake_codex = fake_bin.join("codex");
        fs::write(
            &fake_codex,
            "#!/usr/bin/env python3\nimport time\ntime.sleep(5)\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&fake_codex).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&fake_codex, perms).unwrap();
        }

        let old_path = std::env::var_os("PATH");
        let old_timeout = std::env::var_os("PUNK_CODEX_DRAFTER_TIMEOUT_SECS");
        std::env::set_var(
            "PATH",
            std::env::join_paths(
                [fake_bin.clone()]
                    .into_iter()
                    .chain(std::env::split_paths(&old_path.clone().unwrap_or_default())),
            )
            .unwrap(),
        );
        std::env::set_var("PUNK_CODEX_DRAFTER_TIMEOUT_SECS", "3");

        let drafter = CodexCliContractDrafter::default();
        let input = DraftInput {
            repo_root: root.display().to_string(),
            prompt: "bounded prompt".into(),
            scan: RepoScanSummary {
                project_kind: "generic".into(),
                manifests: vec![],
                package_manager: None,
                available_scripts: BTreeMap::new(),
                candidate_entry_points: vec![],
                candidate_scope_paths: vec![],
                candidate_file_scope_paths: vec![],
                candidate_directory_scope_paths: vec![],
                candidate_target_checks: vec!["true".into()],
                candidate_integrity_checks: vec!["true".into()],
                notes: vec![],
            },
        };

        let start = Instant::now();
        let err = drafter.draft(input).unwrap_err().to_string();
        let elapsed = start.elapsed();

        if let Some(path) = old_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        if let Some(timeout) = old_timeout {
            std::env::set_var("PUNK_CODEX_DRAFTER_TIMEOUT_SECS", timeout);
        } else {
            std::env::remove_var("PUNK_CODEX_DRAFTER_TIMEOUT_SECS");
        }

        assert!(err.contains("timed out after 3s"), "{err}");
        assert!(
            elapsed < Duration::from_secs(5),
            "drafter elapsed too long: {elapsed:?}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_drafter_skips_retry_after_silent_timeout() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-drafter-silent-timeout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).unwrap();
        let fake_codex = fake_bin.join("codex");
        let count_path = root.join("count.txt");
        fs::write(
            &fake_codex,
            format!(
                "#!/usr/bin/env python3\nimport pathlib, time\ncount_path = pathlib.Path({count_path:?})\ncount = int(count_path.read_text()) if count_path.exists() else 0\ncount_path.write_text(str(count + 1))\ntime.sleep(5)\n"
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&fake_codex).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&fake_codex, perms).unwrap();
        }

        let old_path = std::env::var_os("PATH");
        let old_timeout = std::env::var_os("PUNK_CODEX_DRAFTER_TIMEOUT_SECS");
        std::env::set_var(
            "PATH",
            std::env::join_paths(
                [fake_bin.clone()]
                    .into_iter()
                    .chain(std::env::split_paths(&old_path.clone().unwrap_or_default())),
            )
            .unwrap(),
        );
        std::env::set_var("PUNK_CODEX_DRAFTER_TIMEOUT_SECS", "6");

        let drafter = CodexCliContractDrafter::default();
        let input = DraftInput {
            repo_root: root.display().to_string(),
            prompt: "bounded prompt".into(),
            scan: RepoScanSummary {
                project_kind: "generic".into(),
                manifests: vec![],
                package_manager: None,
                available_scripts: BTreeMap::new(),
                candidate_entry_points: vec![],
                candidate_scope_paths: vec![],
                candidate_file_scope_paths: vec![],
                candidate_directory_scope_paths: vec![],
                candidate_target_checks: vec!["true".into()],
                candidate_integrity_checks: vec!["true".into()],
                notes: vec![],
            },
        };

        let err = drafter.draft(input).unwrap_err().to_string();

        if let Some(path) = old_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        if let Some(timeout) = old_timeout {
            std::env::set_var("PUNK_CODEX_DRAFTER_TIMEOUT_SECS", timeout);
        } else {
            std::env::remove_var("PUNK_CODEX_DRAFTER_TIMEOUT_SECS");
        }

        assert!(err.contains("timed out after 6s"), "{err}");
        assert_eq!(fs::read_to_string(&count_path).unwrap(), "1");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_drafter_skips_retry_after_mcp_only_timeout_noise() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-drafter-mcp-timeout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).unwrap();
        let fake_codex = fake_bin.join("codex");
        let count_path = root.join("count.txt");
        fs::write(
            &fake_codex,
            format!(
                "#!/usr/bin/env python3\nimport pathlib, sys, time\ncount_path = pathlib.Path({count_path:?})\ncount = int(count_path.read_text()) if count_path.exists() else 0\ncount_path.write_text(str(count + 1))\nfor _ in range(6):\n    sys.stderr.write('mcp: engram/mem_search (completed)\\\\n')\n    sys.stderr.flush()\n    time.sleep(0.5)\n"
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&fake_codex).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&fake_codex, perms).unwrap();
        }

        let old_path = std::env::var_os("PATH");
        let old_timeout = std::env::var_os("PUNK_CODEX_DRAFTER_TIMEOUT_SECS");
        std::env::set_var(
            "PATH",
            std::env::join_paths(
                [fake_bin.clone()]
                    .into_iter()
                    .chain(std::env::split_paths(&old_path.clone().unwrap_or_default())),
            )
            .unwrap(),
        );
        std::env::set_var("PUNK_CODEX_DRAFTER_TIMEOUT_SECS", "6");

        let drafter = CodexCliContractDrafter::default();
        let input = DraftInput {
            repo_root: root.display().to_string(),
            prompt: "bounded prompt".into(),
            scan: RepoScanSummary {
                project_kind: "generic".into(),
                manifests: vec![],
                package_manager: None,
                available_scripts: BTreeMap::new(),
                candidate_entry_points: vec![],
                candidate_scope_paths: vec![],
                candidate_file_scope_paths: vec![],
                candidate_directory_scope_paths: vec![],
                candidate_target_checks: vec!["true".into()],
                candidate_integrity_checks: vec!["true".into()],
                notes: vec![],
            },
        };

        let err = drafter.draft(input).unwrap_err().to_string();

        if let Some(path) = old_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        if let Some(timeout) = old_timeout {
            std::env::set_var("PUNK_CODEX_DRAFTER_TIMEOUT_SECS", timeout);
        } else {
            std::env::remove_var("PUNK_CODEX_DRAFTER_TIMEOUT_SECS");
        }

        assert!(err.contains("timed out after 6s"), "{err}");
        assert_eq!(fs::read_to_string(&count_path).unwrap(), "1");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_drafter_retries_after_partial_draft_json_timeout() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-drafter-partial-json-timeout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).unwrap();
        let fake_codex = fake_bin.join("codex");
        let count_path = root.join("count.txt");
        fs::write(
            &fake_codex,
            format!(
                "#!/usr/bin/env python3\nimport pathlib, sys, time\ncount_path = pathlib.Path({count_path:?})\ncount = int(count_path.read_text()) if count_path.exists() else 0\ncount_path.write_text(str(count + 1))\nsys.stderr.write('{{\\\"allowed_scope\\\":[')\nsys.stderr.flush()\ntime.sleep(5)\n"
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&fake_codex).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&fake_codex, perms).unwrap();
        }

        let old_path = std::env::var_os("PATH");
        let old_timeout = std::env::var_os("PUNK_CODEX_DRAFTER_TIMEOUT_SECS");
        std::env::set_var(
            "PATH",
            std::env::join_paths(
                [fake_bin.clone()]
                    .into_iter()
                    .chain(std::env::split_paths(&old_path.clone().unwrap_or_default())),
            )
            .unwrap(),
        );
        std::env::set_var("PUNK_CODEX_DRAFTER_TIMEOUT_SECS", "6");

        let drafter = CodexCliContractDrafter::default();
        let input = DraftInput {
            repo_root: root.display().to_string(),
            prompt: "bounded prompt".into(),
            scan: RepoScanSummary {
                project_kind: "generic".into(),
                manifests: vec![],
                package_manager: None,
                available_scripts: BTreeMap::new(),
                candidate_entry_points: vec![],
                candidate_scope_paths: vec![],
                candidate_file_scope_paths: vec![],
                candidate_directory_scope_paths: vec![],
                candidate_target_checks: vec!["true".into()],
                candidate_integrity_checks: vec!["true".into()],
                notes: vec![],
            },
        };

        let _err = drafter.draft(input).unwrap_err().to_string();

        if let Some(path) = old_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        if let Some(timeout) = old_timeout {
            std::env::set_var("PUNK_CODEX_DRAFTER_TIMEOUT_SECS", timeout);
        } else {
            std::env::remove_var("PUNK_CODEX_DRAFTER_TIMEOUT_SECS");
        }

        assert_eq!(fs::read_to_string(&count_path).unwrap(), "2");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_drafter_passes_prompt_after_separator() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-drafter-separator-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).unwrap();
        let fake_codex = fake_bin.join("codex");
        let args_path = root.join("args.json");
        fs::write(
            &fake_codex,
            format!(
                "#!/usr/bin/env python3\nimport json, sys\nargs = sys.argv[1:]\nwith open({args_path:?}, 'w') as fh:\n    json.dump(args, fh)\nout_path = None\nfor idx, arg in enumerate(args):\n    if arg == '-o' and idx + 1 < len(args):\n        out_path = args[idx + 1]\n        break\npayload = {{'title': 'draft', 'summary': 'draft', 'entry_points': ['src/lib.rs'], 'import_paths': [], 'expected_interfaces': ['pub fn demo'], 'behavior_requirements': ['do demo'], 'allowed_scope': ['src/lib.rs'], 'target_checks': ['true'], 'integrity_checks': ['true'], 'risk_level': 'low'}}\nif out_path:\n    with open(out_path, 'w') as fh:\n        json.dump(payload, fh)\nelse:\n    print(json.dumps(payload))\n"
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&fake_codex).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&fake_codex, perms).unwrap();
        }

        let old_path = std::env::var_os("PATH");
        std::env::set_var(
            "PATH",
            std::env::join_paths(
                [fake_bin.clone()]
                    .into_iter()
                    .chain(std::env::split_paths(&old_path.clone().unwrap_or_default())),
            )
            .unwrap(),
        );

        let drafter = CodexCliContractDrafter::default();
        let schema_path = root.join("schema.json");
        let output_path = root.join("output.json");
        fs::write(&schema_path, "{}").unwrap();
        let prompt = "--dangerous prompt";
        let proposal = match drafter.run_json_prompt_once(
            prompt,
            root.to_str().unwrap(),
            &schema_path,
            &output_path,
            Duration::from_secs(5),
        ) {
            Ok(proposal) => proposal,
            Err(_) => panic!("fake codex should succeed"),
        };

        if let Some(path) = old_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        let args: Vec<String> = serde_json::from_slice(&fs::read(&args_path).unwrap()).unwrap();
        let separator_index = args
            .iter()
            .position(|arg| arg == "--")
            .expect("codex args should contain separator");
        assert_eq!(
            args.get(separator_index + 1).map(String::as_str),
            Some(prompt)
        );
        assert_eq!(proposal.title, "draft");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn greenfield_manifest_contracts_use_shorter_executor_timeout() {
        let contract = Contract {
            id: "ct_greenfield".into(),
            feature_id: "feat_greenfield".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "scaffold Rust workspace".into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec![],
            expected_interfaces: vec!["initial Rust scaffold".into()],
            behavior_requirements: vec!["scaffold Rust workspace".into()],
            allowed_scope: vec!["Cargo.toml".into(), "crates".into(), "tests".into()],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert_eq!(
            effective_codex_executor_timeout(&contract),
            Duration::from_secs(20)
        );
    }

    #[test]
    fn build_patch_apply_prompt_requests_plain_patch_text() {
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["public fn x".into()],
            behavior_requirements: vec!["do x".into()],
            allowed_scope: vec!["src/lib.rs".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        let prompt = build_patch_apply_prompt(&contract, &ContextPack::default());
        assert!(prompt.contains("single apply_patch-style patch"));
        assert!(prompt.contains("do not output JSON"));
        assert!(prompt.contains("do not run `rg`, `grep`, `sed`, `cat`, `find`, `ls`, or any other shell command for orientation"));
        assert!(prompt
            .contains("do not use python or any shell/interpreter command to rediscover them"));
        assert!(prompt.contains(blocked_execution_template()));
        assert!(!prompt.contains("match the provided schema exactly"));
    }

    #[test]
    fn bounded_medium_risk_prompt_requires_fail_closed_out_of_scope_blocking() {
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["crates/punk-council/src/scoring.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["scoreboard".into()],
            behavior_requirements: vec!["add deterministic scoring".into()],
            allowed_scope: vec!["crates/punk-council/src/scoring.rs".into()],
            target_checks: vec!["cargo test -p punk-council".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        let prompt = build_exec_prompt(&contract, None, &[]);
        assert!(prompt.contains("Do not read any file outside allowed scope or entry points."));
        assert!(prompt.contains("Prioritize production code paths first."));
        assert!(prompt.contains("treat everything below that boundary as off-limits by default"));
        assert!(prompt.contains("Avoid reading `#[cfg(test)]` modules or long test sections"));
        assert!(prompt.contains(
            "Do not create temporary type-introspection tests, debug-print probes, or `--nocapture` discovery loops"
        ));
        assert!(prompt.contains(
            "Do not invent, invoke, or follow any alternate framework or meta-workflow such as signum phases"
        ));
        assert!(prompt.contains(
            "Do not use git checkout, git restore, git reset, git clean, git switch, or similar VCS restore/reset commands"
        ));
        assert!(prompt.contains(
            "PUNK_EXECUTION_BLOCKED: need out-of-scope file <repo-relative-path> because <reason>"
        ));
        assert!(prompt.contains(
            "If a required Rust type shape still cannot be derived safely from the in-scope source files"
        ));
        assert!(prompt.contains("Do not invent, invoke, or follow any alternate framework or meta-workflow such as signum phases"));
    }

    #[test]
    fn non_bounded_prompt_keeps_soft_scope_guidance() {
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![
                "src/lib.rs".into(),
                "src/main.rs".into(),
                "tests/smoke.rs".into(),
                "README.md".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["iface".into()],
            behavior_requirements: vec!["do x".into()],
            allowed_scope: vec![
                "src".into(),
                "tests".into(),
                "README.md".into(),
                "Cargo.toml".into(),
            ],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        let prompt = build_exec_prompt(&contract, None, &[]);
        assert!(prompt.contains("Do not read unrelated files outside allowed scope unless a concrete compile or verification blocker makes that strictly necessary."));
        assert!(prompt.contains("The approved cut contract already defines the execution workflow"));
        assert!(!prompt.contains(
            "PUNK_EXECUTION_BLOCKED: need out-of-scope file <repo-relative-path> because <reason>"
        ));
    }

    #[test]
    fn broader_contracts_still_forbid_signum_meta_workflows() {
        let contract = Contract {
            id: "ct_events".into(),
            feature_id: "feat_events".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![
                "crates/punk-council/src/events.rs".into(),
                "crates/punk-council/src/proposal.rs".into(),
                "crates/punk-council/src/review.rs".into(),
                "crates/punk-council/src/synthesis.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["event emission".into()],
            behavior_requirements: vec!["emit council phase events".into()],
            allowed_scope: vec![
                "crates/punk-council/src/events.rs".into(),
                "crates/punk-council/src/proposal.rs".into(),
                "crates/punk-council/src/review.rs".into(),
                "crates/punk-council/src/synthesis.rs".into(),
                "crates/punk-council/src/lib.rs".into(),
            ],
            target_checks: vec!["cargo test -p punk-council".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        let prompt = build_exec_prompt(&contract, None, &[]);
        assert!(prompt.contains("Do not invent, invoke, or follow any alternate framework or meta-workflow such as signum phases"));
        assert!(prompt.contains("The approved cut contract already defines the execution workflow"));
        assert!(prompt.contains("Do not read any file outside allowed scope or entry points."));
        assert!(prompt.contains("Avoid reading `#[cfg(test)]` modules or long test sections"));
        assert!(prompt
            .contains("Do not use git checkout, git restore, git reset, git clean, git switch"));
    }

    #[test]
    fn bounded_prompt_includes_authoritative_context_pack() {
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["crates/punk-council/src/scoring.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["score_reviews".into()],
            behavior_requirements: vec!["add deterministic scoring".into()],
            allowed_scope: vec!["crates/punk-council/src/scoring.rs".into()],
            target_checks: vec!["cargo test -p punk-council".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        let context_pack = ContextPack {
            files: vec![context_pack::ContextFileExcerpt {
                path: "crates/punk-council/src/scoring.rs".into(),
                start_line: 1,
                end_line: 3,
                truncated_at_test_boundary: true,
                content: "pub fn score_reviews() {}".into(),
            }],
            missing_paths: vec![],
            recipe_seed: None,
            patch_seed: None,
            plan_seed: None,
        };
        let prompt = build_exec_prompt(
            &contract,
            Some(&context_pack),
            &["crates/punk-council/src/scoring.rs".into()],
        );
        assert!(prompt.contains("Authoritative bounded context pack:"));
        assert!(prompt.contains(
            "Use this controller-built bounded context as the authoritative initial implementation context."
        ));
        assert!(prompt.contains(
            "entry-point files may be pre-masked to production-only content above the first test boundary"
        ));
        assert!(prompt.contains(
            "If the context pack lists missing entry-point files at baseline, treat those paths as approved new files and create them directly inside allowed scope instead of probing for them."
        ));
        assert!(prompt.contains(
            "The following entry-point files were materialized for this run and must remain present"
        ));
        assert!(prompt.contains("pub fn score_reviews() {}"));
    }

    #[test]
    fn bounded_prompt_tells_executor_to_follow_recipe_seed_directly() {
        let contract = Contract {
            id: "ct_synth".into(),
            feature_id: "feat_synth".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![
                "crates/punk-council/src/synthesis.rs".into(),
                "crates/punk-council/src/lib.rs".into(),
                "crates/punk-council/src/storage.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["CouncilSynthesis".into()],
            behavior_requirements: vec!["persist synthesis.json and final record.json".into()],
            allowed_scope: vec![
                "crates/punk-council/src/synthesis.rs".into(),
                "crates/punk-council/src/lib.rs".into(),
                "crates/punk-council/src/storage.rs".into(),
            ],
            target_checks: vec!["cargo test -p punk-council".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        let context_pack = ContextPack {
            files: vec![],
            missing_paths: vec![],
            recipe_seed: Some(context_pack::ContextRecipeSeed {
                title: "seed".into(),
                summary: "multi-file".into(),
                files: vec![context_pack::ContextRecipeFileSeed {
                    path: "crates/punk-council/src/lib.rs".into(),
                    role: "wiring".into(),
                    edit_targets: vec!["add service method".into()],
                }],
            }),
            patch_seed: Some(context_pack::ContextPatchSeed {
                title: "patch".into(),
                summary: "apply patch".into(),
                files: vec![context_pack::ContextPatchSeedFile {
                    path: "crates/punk-council/src/storage.rs".into(),
                    purpose: "persistence".into(),
                    snippet: "pub fn persist_synthesis() {}".into(),
                }],
            }),
            plan_seed: None,
        };
        let prompt = build_exec_prompt(&contract, Some(&context_pack), &[]);
        assert!(prompt.contains(
            "If the context pack includes a controller-owned patch seed, apply those snippets in place first"
        ));
        assert!(prompt.contains(
            "If the context pack includes a controller-owned recipe seed, follow it directly"
        ));
        assert!(prompt
            .contains("Do not use git checkout, git restore, git reset, git clean, git switch"));
        assert!(prompt.contains("Controller-owned patch seed:"));
        assert!(prompt.contains("pub fn persist_synthesis() {}"));
        assert!(prompt.contains("Controller-owned recipe seed:"));
        assert!(prompt.contains("add service method"));
    }

    #[test]
    fn git_guard_wrapper_blocks_destructive_restore_commands() {
        let guard = GitGuardEnv::install()
            .unwrap()
            .expect("git guard should install");
        let mut command = Command::new("python3");
        command.arg("-c").arg(
            "import subprocess, sys; cp = subprocess.run(['/bin/zsh', '-lc', 'git checkout -- src/lib.rs'], capture_output=True, text=True); sys.stdout.write(cp.stdout); sys.stderr.write(cp.stderr); sys.exit(cp.returncode)",
        );
        guard.apply(&mut command);

        let output = command.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}\n{stderr}");
        assert!(!output.status.success());
        assert!(
            combined.contains("PUNK_EXECUTION_BLOCKED: forbidden vcs restore/reset command"),
            "combined output: {combined:?}"
        );
        assert!(
            combined.contains("git checkout -- src/lib.rs"),
            "combined output: {combined:?}"
        );
    }

    #[test]
    fn orientation_guard_blocks_shell_orientation_commands() {
        let guard = OrientationGuardEnv::install().expect("orientation guard should install");
        let python = find_binary_in_path("python3").expect("python3 should exist");
        let mut command = Command::new(python);
        command.arg("-c").arg(
            "import subprocess, sys; cp = subprocess.run(['/bin/zsh', '-lc', 'rg -n \"foo\" src'], capture_output=True, text=True); sys.stdout.write(cp.stdout); sys.stderr.write(cp.stderr); sys.exit(cp.returncode)",
        );
        guard.apply(&mut command);

        let output = command.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}\n{stderr}");
        assert!(!output.status.success());
        assert!(
            combined.contains(
                "PUNK_EXECUTION_BLOCKED: shell orientation forbidden in patch/apply lane: rg"
            ),
            "combined output: {combined:?}"
        );
    }

    #[test]
    fn low_risk_bounded_execution_uses_low_reasoning() {
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["public fn x".into()],
            behavior_requirements: vec!["do x".into()],
            allowed_scope: vec!["src/lib.rs".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        assert_eq!(codex_executor_reasoning_effort(&contract), Some("low"));
    }

    #[test]
    fn self_referential_reliability_slice_requires_manual_mode() {
        let contract = Contract {
            id: "ct_retry".into(),
            feature_id: "feat_retry".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source:
                "Strengthen patch-seed retry for no-progress existing-file self-hosting slices"
                    .into(),
            entry_points: vec![
                "crates/punk-adapters/src/context_pack.rs".into(),
                "crates/punk-adapters/src/lib.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["controller-owned retry path".into()],
            behavior_requirements: vec![
                "preserve no implementation progress after bounded context dispatch".into(),
            ],
            allowed_scope: vec![
                "crates/punk-adapters/src/context_pack.rs".into(),
                "crates/punk-adapters/src/lib.rs".into(),
            ],
            target_checks: vec!["cargo test -p punk-adapters".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert!(is_self_referential_reliability_slice(&contract));
        assert_eq!(
            manual_mode_block_summary(&contract).as_deref(),
            Some(
                "PUNK_EXECUTION_BLOCKED: self-referential reliability slice requires manual bounded implementation"
            )
        );
    }

    #[test]
    fn helper_feature_without_reliability_signal_is_not_forced_into_manual_mode() {
        let contract = Contract {
            id: "ct_vcs".into(),
            feature_id: "feat_vcs".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "Add JSON output to punk vcs status".into(),
            entry_points: vec![
                "crates/punk-vcs/src/lib.rs".into(),
                "crates/punk-cli/src/main.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["punk vcs status --json emits structured JSON".into()],
            behavior_requirements: vec!["preserve human output".into()],
            allowed_scope: vec![
                "crates/punk-vcs/src/lib.rs".into(),
                "crates/punk-cli/src/main.rs".into(),
            ],
            target_checks: vec!["cargo test -p punk-vcs".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert!(!is_self_referential_reliability_slice(&contract));
        assert!(manual_mode_block_summary(&contract).is_none());
    }

    #[test]
    fn execute_contract_short_circuits_manual_mode_slices() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-manual-mode-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let contract = Contract {
            id: "ct_post_check".into(),
            feature_id: "feat_post_check".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source:
                "Classify and fail fast on post-check zero-progress stall in bounded self-hosting runs"
                    .into(),
            entry_points: vec!["crates/punk-adapters/src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["preserve current no-progress detection".into()],
            behavior_requirements: vec!["post-check stall reliability slice".into()],
            allowed_scope: vec!["crates/punk-adapters/src/lib.rs".into()],
            target_checks: vec!["cargo test -p punk-adapters".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let executor = CodexCliExecutor::default();
        let output = executor
            .execute_contract(ExecuteInput {
                repo_root: root.clone(),
                contract,
                stdout_path: root.join("stdout.log"),
                stderr_path: root.join("stderr.log"),
                executor_pid_path: root.join("executor.json"),
            })
            .unwrap();

        assert!(!output.success);
        assert_eq!(
            output.summary,
            "PUNK_EXECUTION_BLOCKED: self-referential reliability slice requires manual bounded implementation"
        );
        assert_eq!(output.duration_ms, 0);
        assert!(!root.join("stdout.log").exists());
        assert!(!root.join("stderr.log").exists());
        assert!(!root.join("executor.json").exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn execution_lane_uses_patch_apply_for_existing_file_glue_slice() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-patch-lane-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("punk/punk-orch/src")).unwrap();
        fs::create_dir_all(root.join("punk/punk-run/src")).unwrap();
        fs::write(
            root.join("punk/punk-orch/src/ratchet.rs"),
            "pub struct X;\n",
        )
        .unwrap();
        fs::write(root.join("punk/punk-run/src/main.rs"), "fn main() {}\n").unwrap();

        let contract = Contract {
            id: "ct_skill_bridge".into(),
            feature_id: "feat_skill_bridge".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "Bridge skill eval summaries into the nested punk ratchet surface"
                .into(),
            entry_points: vec![
                "punk/punk-orch/src/ratchet.rs".into(),
                "punk/punk-run/src/main.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["skill-eval ratchet bridge".into()],
            behavior_requirements: vec![
                "add skill eval summary aggregation to ratchet output".into()
            ],
            allowed_scope: vec![
                "punk/punk-orch/src/ratchet.rs".into(),
                "punk/punk-run/src/main.rs".into(),
            ],
            target_checks: vec!["cargo test -p punk-orch".into()],
            integrity_checks: vec!["cargo test -p punk-run".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert_eq!(
            execution_lane_for_contract(&root, &contract),
            ExecutionLane::PatchApply
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn validate_patch_scope_rejects_out_of_scope_paths() {
        let err = validate_patch_scope(
            "*** Begin Patch\n*** Update File: src/lib.rs\n@@ fn old\n-pub fn old() {}\n+pub fn new() {}\n*** End Patch\n",
            &[String::from("src/main.rs")],
        )
        .unwrap_err();
        assert!(err.to_string().contains("out-of-scope path src/lib.rs"));
    }

    #[test]
    fn load_patch_lane_response_extracts_fenced_patch() {
        let stdout = "```text\n*** Begin Patch\n*** Update File: src/lib.rs\n@@ fn old\n-pub fn old() {}\n+pub fn new() {}\n*** End Patch\n```\n";
        let response = load_patch_lane_response(stdout, "").unwrap();
        assert_eq!(
            response,
            PatchLaneResponse::Patch(
                "*** Begin Patch\n*** Update File: src/lib.rs\n@@ fn old\n-pub fn old() {}\n+pub fn new() {}\n*** End Patch\n".to_string()
            )
        );
    }

    #[test]
    fn load_patch_lane_response_prefers_blocked_sentinel() {
        let response = load_patch_lane_response(
            "",
            "noise\nPUNK_EXECUTION_BLOCKED: waiting on missing interface details\n",
        )
        .unwrap();
        assert_eq!(
            response,
            PatchLaneResponse::Blocked(
                "PUNK_EXECUTION_BLOCKED: waiting on missing interface details".to_string()
            )
        );
    }

    #[test]
    fn load_plan_prepass_response_extracts_compact_plan() {
        let stdout = "PUNK_PLAN_BEGIN\nSUMMARY: wire skill eval summary into status output\nTARGET: punk/punk-orch/src/lib.rs\nSYMBOL: StatusSnapshot\nINSERT: near existing eval/benchmark fields\nSKETCH: add optional skill eval window fields and populate them from eval summary helpers\nTARGET: punk/punk-run/src/main.rs\nSYMBOL: cmd_status\nINSERT: after benchmark window printing\nSKETCH: print one concise skill eval window line using stored summary data\nPUNK_PLAN_END\n";
        let response = load_plan_prepass_response(stdout, "").unwrap();
        assert!(matches!(
            response,
            PlanPrepassResponse::Plan { targets, .. } if targets.len() == 2
        ));
    }

    #[test]
    fn validate_plan_prepass_scope_rejects_out_of_scope_paths() {
        let response = PlanPrepassResponse::Plan {
            summary: "x".into(),
            targets: vec![PlanPrepassTarget {
                path: "src/lib.rs".into(),
                symbol: "fn old".into(),
                insertion_point: "after old".into(),
                execution_sketch: "edit old".into(),
            }],
        };
        let err =
            validate_plan_prepass_scope(&response, &[String::from("src/main.rs")]).unwrap_err();
        assert!(err.to_string().contains("out-of-scope path src/lib.rs"));
    }

    #[test]
    fn apply_patch_in_repo_applies_update_patch() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-apply-patch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        let mut init = Command::new("git");
        init.arg("init").current_dir(&root);
        assert!(init.status().unwrap().success());
        fs::write(root.join("src/lib.rs"), "pub fn old() {}\n").unwrap();

        let patch = "*** Begin Patch\n*** Update File: src/lib.rs\n@@ fn old\n-pub fn old() {}\n+pub fn new() {}\n*** End Patch\n";
        let updates = validate_patch_scope(patch, &[String::from("src/lib.rs")]).unwrap();
        apply_patch_in_repo(&root, &updates).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "pub fn new() {}\n"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_patch_in_repo_accepts_addition_hunk_with_gapped_context() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-apply-patch-gap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        let mut init = Command::new("git");
        init.arg("init").current_dir(&root);
        assert!(init.status().unwrap().success());
        fs::write(
            root.join("src/lib.rs"),
            "pub fn summarize() {\n    let skills = 1;\n    let weakest = 2;\n    println!(\"done\");\n}\n",
        )
        .unwrap();

        let patch = "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n     let skills = 1;\n     println!(\"done\");\n }\n+\n+pub fn format_summary() -> &'static str {\n+    \"ok\"\n+}\n*** End Patch\n";
        let updates = validate_patch_scope(patch, &[String::from("src/lib.rs")]).unwrap();
        apply_patch_in_repo(&root, &updates).unwrap();
        let updated = fs::read_to_string(root.join("src/lib.rs")).unwrap();
        assert!(updated.contains("pub fn format_summary()"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn validate_patch_scope_rejects_add_file_sections() {
        let err = validate_patch_scope(
            "*** Begin Patch\n*** Add File: src/new.rs\n+pub fn new() {}\n*** End Patch\n",
            &[String::from("src")],
        )
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("only accepts existing-file updates"));
    }

    #[test]
    fn run_patch_lane_command_with_timeout_detects_complete_patch_before_exit() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-patch-stream-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        let executor_pid = root.join("executor.json");

        let mut command = Command::new("/bin/zsh");
        command.arg("-lc").arg(
            "printf '*** Begin Patch\n*** Update File: src/lib.rs\n@@ fn old\n-pub fn old() {}\n+pub fn new() {}\n*** End Patch\n'; sleep 5",
        );

        let output = run_patch_lane_command_with_timeout(
            &mut command,
            Duration::from_secs(1),
            stdout_path,
            stderr_path,
            executor_pid,
        )
        .unwrap();

        assert!(!output.timed_out);
        assert!(!output.orphaned);
        assert!(matches!(output.response, Some(PatchLaneResponse::Patch(_))));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn needs_patch_plan_prepass_for_uncertain_status_summary_slice() {
        let contract = Contract {
            id: "ct_status".into(),
            feature_id: "feat_status".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "Add a skill eval summary line to nested punk status output and reuse existing eval summary helpers.".into(),
            entry_points: vec![
                "punk/punk-orch/src/lib.rs".into(),
                "punk/punk-run/src/main.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["status snapshot remains backward-compatible".into()],
            behavior_requirements: vec![
                "derive window data from stored skill eval summaries".into(),
                "print concise human-readable output similar to existing window reporting".into(),
            ],
            allowed_scope: vec![
                "punk/punk-orch/src/lib.rs".into(),
                "punk/punk-run/src/main.rs".into(),
            ],
            target_checks: vec!["cargo test -p punk-run".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        assert!(needs_patch_plan_prepass(&contract, &ContextPack::default()));
    }

    #[test]
    fn infer_scoped_cargo_root_prefers_nested_workspace_root() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-check-root-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("punk/punk-run/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers=[\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("punk/Cargo.toml"),
            "[workspace]\nmembers=[\"punk-run\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("punk/punk-run/Cargo.toml"),
            "[package]\nname=\"punk-run\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        fs::write(root.join("punk/punk-run/src/main.rs"), "fn main() {}\n").unwrap();

        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["punk/punk-run/src/main.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["main".into()],
            behavior_requirements: vec!["do x".into()],
            allowed_scope: vec!["punk/punk-run/src/main.rs".into()],
            target_checks: vec!["cargo test -p punk-run".into()],
            integrity_checks: vec!["cargo build -p punk-run".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert_eq!(
            infer_scoped_cargo_root(&root, &contract),
            Some(root.join("punk"))
        );
        assert_eq!(
            resolve_check_workdir(&root, &contract, "cargo test -p punk-run"),
            root.join("punk")
        );
        assert_eq!(
            resolve_check_workdir(&root, &contract, "cd punk && cargo test -p punk-run"),
            root
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn seek_sequence_prefers_exact_match_after_start_offset() {
        let haystack = vec![
            "fn alpha() {".to_string(),
            "    one();".to_string(),
            "}".to_string(),
            "fn gamma() {".to_string(),
            "    three();".to_string(),
            "}".to_string(),
        ];
        let needle = vec![
            "fn gamma() {".to_string(),
            "    three();".to_string(),
            "}".to_string(),
        ];
        assert_eq!(seek_sequence(&haystack, &needle, 3, false), Some(3));
    }

    #[test]
    fn non_bounded_or_high_risk_execution_does_not_override_reasoning() {
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["public fn x".into()],
            behavior_requirements: vec!["do x".into()],
            allowed_scope: vec!["src/lib.rs".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "high".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        assert_eq!(codex_executor_reasoning_effort(&contract), None);
    }

    #[test]
    fn bounded_medium_risk_execution_uses_medium_reasoning() {
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["public fn x".into()],
            behavior_requirements: vec!["do x".into()],
            allowed_scope: vec!["src/lib.rs".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        assert_eq!(codex_executor_reasoning_effort(&contract), Some("medium"));
    }

    #[test]
    fn broader_explicit_file_scope_uses_bounded_reasoning() {
        let contract = Contract {
            id: "ct_events".into(),
            feature_id: "feat_events".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![
                "crates/punk-council/src/events.rs".into(),
                "crates/punk-council/src/proposal.rs".into(),
                "crates/punk-council/src/review.rs".into(),
                "crates/punk-council/src/synthesis.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["event emission".into()],
            behavior_requirements: vec!["emit council phase events".into()],
            allowed_scope: vec![
                "crates/punk-council/src/events.rs".into(),
                "crates/punk-council/src/proposal.rs".into(),
                "crates/punk-council/src/review.rs".into(),
                "crates/punk-council/src/synthesis.rs".into(),
                "crates/punk-council/src/lib.rs".into(),
            ],
            target_checks: vec!["cargo test -p punk-council".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        assert!(is_bounded_execution_task(&contract));
        assert_eq!(codex_executor_reasoning_effort(&contract), Some("low"));
    }

    #[test]
    fn directory_scope_does_not_count_as_bounded_execution() {
        let contract = Contract {
            id: "ct_dir".into(),
            feature_id: "feat_dir".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["src/lib.rs".into(), "src/main.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["iface".into()],
            behavior_requirements: vec!["do x".into()],
            allowed_scope: vec!["src".into(), "Cargo.toml".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        assert!(!is_bounded_execution_task(&contract));
    }

    #[test]
    fn build_draft_prompt_embeds_scan() {
        let input = DraftInput {
            repo_root: "/tmp/demo".into(),
            prompt: "add auth".into(),
            scan: RepoScanSummary {
                project_kind: "rust".into(),
                manifests: vec!["Cargo.toml".into()],
                package_manager: None,
                available_scripts: BTreeMap::new(),
                candidate_entry_points: vec!["src/lib.rs".into()],
                candidate_scope_paths: vec!["src".into()],
                candidate_file_scope_paths: vec!["src/lib.rs".into()],
                candidate_directory_scope_paths: vec!["src".into()],
                candidate_target_checks: vec!["cargo test".into()],
                candidate_integrity_checks: vec!["cargo test".into()],
                notes: vec![],
            },
        };
        let prompt = build_draft_prompt(&input);
        assert!(prompt.contains("cargo test"));
        assert!(prompt.contains("add auth"));
    }

    #[test]
    fn compact_draft_prompt_is_shorter_than_primary_prompt() {
        let input = DraftInput {
            repo_root: "/tmp/demo".into(),
            prompt: "add auth".into(),
            scan: RepoScanSummary {
                project_kind: "rust".into(),
                manifests: vec!["Cargo.toml".into()],
                package_manager: None,
                available_scripts: BTreeMap::new(),
                candidate_entry_points: vec!["src/lib.rs".into()],
                candidate_scope_paths: vec!["src".into()],
                candidate_file_scope_paths: vec!["src/lib.rs".into()],
                candidate_directory_scope_paths: vec!["src".into()],
                candidate_target_checks: vec![
                    "cargo build -p punk-cli".into(),
                    "cargo test -p punk-adapters".into(),
                ],
                candidate_integrity_checks: vec!["cargo test --workspace".into()],
                notes: vec!["extra note".into()],
            },
        };
        let primary = build_draft_prompt(&input);
        let compact = build_compact_draft_prompt(&input);
        assert!(compact.len() < primary.len());
        assert!(compact.contains("add auth"));
    }

    #[test]
    fn compact_refine_prompt_is_shorter_than_primary_prompt() {
        let input = RefineInput {
            repo_root: "/tmp/demo".into(),
            prompt: "narrow the scope".into(),
            guidance: "keep exact checks".into(),
            current: DraftProposal {
                title: "Title".into(),
                summary: "Summary".into(),
                entry_points: vec!["src/lib.rs".into()],
                import_paths: vec!["src/lib.rs".into()],
                expected_interfaces: vec!["iface".into()],
                behavior_requirements: vec!["req".into()],
                allowed_scope: vec!["src".into()],
                target_checks: vec!["cargo test -p punk-adapters".into()],
                integrity_checks: vec!["cargo test --workspace".into()],
                risk_level: "low".into(),
            },
            scan: RepoScanSummary {
                project_kind: "rust".into(),
                manifests: vec!["Cargo.toml".into()],
                package_manager: None,
                available_scripts: BTreeMap::new(),
                candidate_entry_points: vec!["src/lib.rs".into()],
                candidate_scope_paths: vec!["src".into()],
                candidate_file_scope_paths: vec!["src/lib.rs".into()],
                candidate_directory_scope_paths: vec!["src".into()],
                candidate_target_checks: vec![
                    "cargo build -p punk-cli".into(),
                    "cargo test -p punk-adapters".into(),
                ],
                candidate_integrity_checks: vec!["cargo test --workspace".into()],
                notes: vec!["extra note".into()],
            },
        };
        let primary = build_refine_prompt(&input);
        let compact = build_compact_refine_prompt(&input);
        assert!(compact.len() < primary.len());
        assert!(compact.contains("narrow the scope"));
        assert!(compact.contains("keep exact checks"));
    }

    #[test]
    fn run_command_with_timeout_returns_output_on_success() {
        let mut command = Command::new("/bin/sh");
        command.arg("-lc").arg("printf ok");
        let output = run_command_with_timeout(&mut command, Duration::from_secs(1)).unwrap();
        assert!(!output.timed_out);
        assert!(!output.stalled);
        assert!(!output.orphaned);
        assert_eq!(String::from_utf8_lossy(&output.output.stdout), "ok");
    }

    #[test]
    fn run_command_with_timeout_marks_timeout_and_preserves_output() {
        let mut command = Command::new("/bin/sh");
        command.arg("-lc").arg("printf before-timeout; sleep 2");
        let output = run_command_with_timeout(&mut command, Duration::from_secs(1)).unwrap();
        assert!(output.timed_out);
        assert!(!output.stalled);
        assert!(!output.orphaned);
        assert_eq!(
            String::from_utf8_lossy(&output.output.stdout),
            "before-timeout"
        );
    }

    #[test]
    fn run_command_with_timeout_returns_without_waiting_for_detached_pipe_leak() {
        let mut command = Command::new("/bin/sh");
        command.arg("-lc").arg(
            "python3 - <<'PY'\nimport subprocess, sys\nsubprocess.Popen(['/bin/sh', '-lc', 'sleep 5'], stdout=sys.stdout, stderr=sys.stderr, close_fds=False, start_new_session=True)\nsys.stdout.write('parent-done')\nsys.stdout.flush()\nPY",
        );

        let start = Instant::now();
        let output = run_command_with_timeout(&mut command, Duration::from_secs(5)).unwrap();

        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(!output.timed_out);
        assert!(!output.stalled);
        assert!(output.orphaned);
        assert_eq!(
            String::from_utf8_lossy(&output.output.stdout),
            "parent-done"
        );
    }

    #[test]
    fn timeout_summary_prefers_last_non_empty_line() {
        let summary = timeout_summary(Duration::from_secs(3), "first\nlast stdout\n", "");
        assert!(summary.contains("timed out after 3s"));
        assert!(summary.contains("last stdout"));
    }

    #[test]
    fn run_command_with_timeout_and_tee_streams_to_files() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-tee-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        let mut command = Command::new("/bin/sh");
        command
            .arg("-lc")
            .arg("printf out; printf err >&2; sleep 1");

        let output = run_command_with_timeout_and_tee(
            &mut command,
            Duration::from_secs(2),
            Duration::from_secs(2),
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_secs(2),
            stdout_path.clone(),
            stderr_path.clone(),
            root.join("executor.json"),
            None,
            None,
        )
        .unwrap();

        assert!(!output.timed_out);
        assert!(!output.stalled);
        assert!(!output.orphaned);
        assert!(output.no_progress_paths.is_empty());
        assert!(output.scaffold_only_paths.is_empty());
        assert!(output.post_check_zero_progress_paths.is_empty());
        assert_eq!(String::from_utf8_lossy(&output.output.stdout), "out");
        assert_eq!(String::from_utf8_lossy(&output.output.stderr), "err");
        assert_eq!(fs::read_to_string(stdout_path).unwrap(), "out");
        assert_eq!(fs::read_to_string(stderr_path).unwrap(), "err");
        assert!(root.join("executor.json").exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_command_with_timeout_and_tee_preserves_partial_output_on_timeout() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-tee-timeout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        let mut command = Command::new("/bin/sh");
        command
            .arg("-lc")
            .arg("printf before-timeout; printf err-before-timeout >&2; sleep 2");

        let output = run_command_with_timeout_and_tee(
            &mut command,
            Duration::from_secs(1),
            Duration::from_secs(5),
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_secs(1),
            stdout_path.clone(),
            stderr_path.clone(),
            root.join("executor.json"),
            None,
            None,
        )
        .unwrap();

        assert!(output.timed_out);
        assert!(!output.stalled);
        assert!(!output.orphaned);
        assert!(output.no_progress_paths.is_empty());
        assert!(output.scaffold_only_paths.is_empty());
        assert!(output.post_check_zero_progress_paths.is_empty());
        assert_eq!(
            String::from_utf8_lossy(&output.output.stdout),
            "before-timeout"
        );
        assert_eq!(
            String::from_utf8_lossy(&output.output.stderr),
            "err-before-timeout"
        );
        assert_eq!(fs::read_to_string(stdout_path).unwrap(), "before-timeout");
        assert_eq!(
            fs::read_to_string(stderr_path).unwrap(),
            "err-before-timeout"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_command_with_timeout_and_tee_marks_stall_without_output_progress() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-tee-stall-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        let mut command = Command::new("/bin/sh");
        command.arg("-lc").arg("printf start; sleep 2");

        let output = run_command_with_timeout_and_tee(
            &mut command,
            Duration::from_secs(5),
            Duration::from_millis(700),
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_millis(700),
            stdout_path.clone(),
            stderr_path.clone(),
            root.join("executor.json"),
            None,
            None,
        )
        .unwrap();

        assert!(!output.timed_out);
        assert!(output.stalled);
        assert!(!output.orphaned);
        assert!(output.no_progress_paths.is_empty());
        assert!(output.scaffold_only_paths.is_empty());
        assert!(output.post_check_zero_progress_paths.is_empty());
        assert_eq!(String::from_utf8_lossy(&output.output.stdout), "start");
        assert_eq!(fs::read_to_string(stdout_path).unwrap(), "start");
        assert!(stall_summary(
            Duration::from_millis(700),
            &String::from_utf8_lossy(&output.output.stdout),
            &String::from_utf8_lossy(&output.output.stderr),
        )
        .contains("stalled"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_command_with_timeout_and_tee_marks_orphaned_process_tree() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-tee-orphan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        let mut command = Command::new("/bin/sh");
        command
            .arg("-lc")
            .arg(
                "python3 - <<'PY'\nimport subprocess, sys\nsubprocess.Popen(['/bin/sh', '-lc', 'sleep 5'], stdout=sys.stdout, stderr=sys.stderr, close_fds=False)\nsys.stdout.write('parent-done')\nsys.stdout.flush()\nPY",
            );

        let output = run_command_with_timeout_and_tee(
            &mut command,
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_millis(400),
            stdout_path.clone(),
            stderr_path.clone(),
            root.join("executor.json"),
            None,
            None,
        )
        .unwrap();

        assert!(!output.timed_out);
        assert!(!output.stalled);
        assert!(output.orphaned);
        assert!(output.no_progress_paths.is_empty());
        assert!(output.scaffold_only_paths.is_empty());
        assert!(output.post_check_zero_progress_paths.is_empty());
        assert_eq!(
            String::from_utf8_lossy(&output.output.stdout),
            "parent-done"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_command_with_timeout_and_tee_returns_without_waiting_for_detached_pipe_leak() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-tee-detached-orphan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        let mut command = Command::new("/bin/sh");
        command.arg("-lc").arg(
            "python3 - <<'PY'\nimport subprocess, sys\nsubprocess.Popen(['/bin/sh', '-lc', 'sleep 5'], stdout=sys.stdout, stderr=sys.stderr, close_fds=False, start_new_session=True)\nsys.stdout.write('parent-done')\nsys.stdout.flush()\nPY",
        );

        let start = Instant::now();
        let output = run_command_with_timeout_and_tee(
            &mut command,
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_millis(400),
            stdout_path.clone(),
            stderr_path.clone(),
            root.join("executor.json"),
            None,
            None,
        )
        .unwrap();

        assert!(start.elapsed() < Duration::from_secs(4));
        assert!(!output.timed_out);
        assert!(!output.stalled);
        assert!(output.orphaned);
        assert_eq!(
            String::from_utf8_lossy(&output.output.stdout),
            "parent-done"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_command_with_timeout_and_tee_detects_scaffold_only_post_check_state() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-tee-scaffold-only-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let contract = Contract {
            id: "ct_synth".into(),
            feature_id: "feat_synth".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["crates/punk-council/src/synthesis.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["CouncilSynthesis helper".into()],
            behavior_requirements: vec!["persist synthesis".into()],
            allowed_scope: vec!["crates/punk-council/src/synthesis.rs".into()],
            target_checks: vec!["cargo test -p punk-council".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        materialize_missing_entry_points(&root, &contract).unwrap();

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        let mut command = Command::new("/bin/sh");
        command
            .arg("-lc")
            .arg("printf 'test result: ok. 0 passed; 0 failed;\\n'; sleep 2");

        let output = run_command_with_timeout_and_tee(
            &mut command,
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(1),
            Duration::from_millis(400),
            Duration::from_secs(1),
            stdout_path,
            stderr_path,
            root.join("executor.json"),
            Some((&root, &contract)),
            None,
        )
        .unwrap();

        assert!(!output.timed_out);
        assert!(!output.stalled);
        assert!(!output.orphaned);
        assert!(output.no_progress_paths.is_empty());
        assert!(output.post_check_zero_progress_paths.is_empty());
        assert_eq!(
            output.scaffold_only_paths,
            vec!["crates/punk-council/src/synthesis.rs".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_command_with_timeout_and_tee_detects_no_progress_after_bounded_dispatch() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-tee-no-progress-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();

        let file_path = root.join("src/lib.rs");
        fs::write(&file_path, "pub fn unchanged() {}\n").unwrap();
        let snapshots = vec![EntryPointSnapshot {
            path: "src/lib.rs".into(),
            content: Some("pub fn unchanged() {}\n".into()),
        }];

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        let mut command = Command::new("/bin/sh");
        command
            .arg("-lc")
            .arg("printf 'bounded context excerpt\\n'; sleep 2");

        let output = run_command_with_timeout_and_tee(
            &mut command,
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_millis(400),
            Duration::from_secs(1),
            Duration::from_secs(1),
            stdout_path,
            stderr_path,
            root.join("executor.json"),
            None,
            Some((&root, snapshots.as_slice())),
        )
        .unwrap();

        assert!(!output.timed_out);
        assert!(!output.stalled);
        assert!(!output.orphaned);
        assert!(output.scaffold_only_paths.is_empty());
        assert!(output.post_check_zero_progress_paths.is_empty());
        assert_eq!(output.no_progress_paths, vec!["src/lib.rs".to_string()]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_command_with_timeout_and_tee_detects_no_progress_despite_periodic_output_noise() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-tee-no-progress-noise-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();

        let file_path = root.join("src/lib.rs");
        fs::write(&file_path, "pub fn unchanged() {}\n").unwrap();
        let snapshots = vec![EntryPointSnapshot {
            path: "src/lib.rs".into(),
            content: Some("pub fn unchanged() {}\n".into()),
        }];

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        let mut command = Command::new("/bin/sh");
        command.arg("-lc").arg(
            "python3 - <<'PY'\nimport sys, time\nfor _ in range(20):\n    sys.stdout.write('mcp: engram/mem_search (completed)\\n')\n    sys.stdout.flush()\n    time.sleep(0.15)\nPY",
        );

        let output = run_command_with_timeout_and_tee(
            &mut command,
            Duration::from_secs(10),
            Duration::from_secs(10),
            Duration::from_millis(600),
            Duration::from_secs(2),
            Duration::from_secs(1),
            stdout_path,
            stderr_path,
            root.join("executor.json"),
            None,
            Some((&root, snapshots.as_slice())),
        )
        .unwrap();

        assert!(!output.timed_out);
        assert!(!output.stalled);
        assert!(!output.orphaned);
        assert!(output.scaffold_only_paths.is_empty());
        assert!(output.post_check_zero_progress_paths.is_empty());
        assert_eq!(output.no_progress_paths, vec!["src/lib.rs".to_string()]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_command_with_timeout_and_tee_detects_no_progress_for_missing_file_entry_point() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-tee-no-progress-missing-file-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let snapshots = vec![EntryPointSnapshot {
            path: "Cargo.toml".into(),
            content: None,
        }];

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        let mut command = Command::new("/bin/sh");
        command.arg("-lc").arg(
            "python3 - <<'PY'\nimport sys, time\nprint('MISSING Cargo.toml')\nsys.stdout.flush()\nfor _ in range(20):\n    sys.stderr.write('mcp: engram/mem_search (completed)\\n')\n    sys.stderr.flush()\n    time.sleep(0.15)\nPY",
        );

        let output = run_command_with_timeout_and_tee(
            &mut command,
            Duration::from_secs(10),
            Duration::from_secs(10),
            Duration::from_millis(600),
            Duration::from_secs(2),
            Duration::from_secs(1),
            stdout_path,
            stderr_path,
            root.join("executor.json"),
            None,
            Some((&root, snapshots.as_slice())),
        )
        .unwrap();

        assert!(!output.timed_out);
        assert!(!output.stalled);
        assert!(!output.orphaned);
        assert!(output.scaffold_only_paths.is_empty());
        assert!(output.post_check_zero_progress_paths.is_empty());
        assert_eq!(output.no_progress_paths, vec!["Cargo.toml".to_string()]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_command_with_timeout_and_tee_detects_post_check_zero_progress_stall() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-tee-post-check-stall-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();

        let file_path = root.join("src/lib.rs");
        fs::write(&file_path, "pub fn unchanged() {}\n").unwrap();
        let snapshots = vec![EntryPointSnapshot {
            path: "src/lib.rs".into(),
            content: Some("pub fn unchanged() {}\n".into()),
        }];

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        let mut command = Command::new("/bin/sh");
        command
            .arg("-lc")
            .arg("printf 'Running unittests\\n0 tests, 0 benchmarks\\n'; sleep 2");

        let output = run_command_with_timeout_and_tee(
            &mut command,
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(1),
            Duration::from_millis(400),
            Duration::from_secs(1),
            stdout_path,
            stderr_path,
            root.join("executor.json"),
            None,
            Some((&root, snapshots.as_slice())),
        )
        .unwrap();

        assert!(!output.timed_out);
        assert!(!output.stalled);
        assert!(!output.orphaned);
        assert!(output.no_progress_paths.is_empty());
        assert!(output.scaffold_only_paths.is_empty());
        assert_eq!(
            output.post_check_zero_progress_paths,
            vec!["src/lib.rs".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compile_or_check_reason_detects_launched_cargo_commands() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-compile-reason-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        fs::write(&stdout_path, "").unwrap();
        fs::write(
            &stderr_path,
            "exec\n/bin/zsh -lc 'cargo build -p punk-cli && cargo test -p punk-council && cargo test --workspace'\n",
        )
        .unwrap();

        assert!(logs_indicate_compile_or_check_reason(
            &stdout_path,
            &stderr_path
        ));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compile_or_check_reason_ignores_prompt_declared_target_checks() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-compile-reason-prompt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        fs::write(&stdout_path, "").unwrap();
        fs::write(
            &stderr_path,
            "user\nTarget checks to satisfy: cargo test --workspace\nIntegrity checks to keep passing: cargo test --workspace\n",
        )
        .unwrap();

        assert!(!logs_indicate_compile_or_check_reason(
            &stdout_path,
            &stderr_path
        ));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compile_or_check_reason_ignores_prompt_declared_sentinels() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-compile-reason-sentinel-prompt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        fs::write(&stdout_path, "").unwrap();
        fs::write(
            &stderr_path,
            "user\nIf you are blocked emit exactly one single-line sentinel in the form `PUNK_EXECUTION_BLOCKED: <reason>` and stop. When implementation is complete emit exactly one single-line sentinel in the form `PUNK_EXECUTION_COMPLETE: <summary>`.\n",
        )
        .unwrap();

        assert!(!logs_indicate_compile_or_check_reason(
            &stdout_path,
            &stderr_path
        ));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn blocked_execution_line_detects_sentinel_in_output() {
        let blocked = blocked_execution_line("line 1\nPUNK_EXECUTION_BLOCKED: missing scope\n", "");
        assert_eq!(
            blocked.as_deref(),
            Some("PUNK_EXECUTION_BLOCKED: missing scope")
        );

        let blocked = blocked_execution_line(
            "",
            "warning\nPUNK_EXECUTION_BLOCKED: missing manifest wiring\n",
        );
        assert_eq!(
            blocked.as_deref(),
            Some("PUNK_EXECUTION_BLOCKED: missing manifest wiring")
        );
    }

    #[test]
    fn successful_execution_line_detects_completion_sentinel() {
        let success =
            successful_execution_line("line 1\nPUNK_EXECUTION_COMPLETE: checks passed\n", "");
        assert_eq!(
            success.as_deref(),
            Some("PUNK_EXECUTION_COMPLETE: checks passed")
        );
    }

    #[test]
    fn blocked_execution_sentinel_forces_non_success() {
        let (success, summary) = classify_execution_result(
            true,
            "PUNK_EXECUTION_BLOCKED: missing scope expansion\n",
            "",
        );
        assert!(!success);
        assert_eq!(summary, "PUNK_EXECUTION_BLOCKED: missing scope expansion");
    }

    #[test]
    fn completion_sentinel_is_required_for_success() {
        let (success, summary) = classify_execution_result(true, "done without sentinel\n", "");
        assert!(!success);
        assert_eq!(summary, "done without sentinel");
    }

    #[test]
    fn completion_sentinel_marks_success() {
        let (success, summary) =
            classify_execution_result(true, "PUNK_EXECUTION_COMPLETE: checks passed\n", "");
        assert!(success);
        assert_eq!(summary, "PUNK_EXECUTION_COMPLETE: checks passed");
    }

    #[test]
    fn timeout_classification_accepts_completion_sentinel() {
        let (success, summary) = classify_timeout_result(
            "PUNK_EXECUTION_COMPLETE: checks passed\n",
            "",
            Duration::from_secs(3),
        );
        assert!(success);
        assert_eq!(summary, "PUNK_EXECUTION_COMPLETE: checks passed");
    }

    #[test]
    fn stall_classification_accepts_completion_sentinel() {
        let (success, summary) = classify_stall_result(
            "PUNK_EXECUTION_COMPLETE: checks passed\n",
            "",
            Duration::from_secs(3),
        );
        assert!(success);
        assert_eq!(summary, "PUNK_EXECUTION_COMPLETE: checks passed");
    }

    #[test]
    fn successful_check_output_detects_shell_success_tail_after_cargo_tests() {
        assert!(output_indicates_successful_check_output(
            "",
            "exec\n/bin/zsh -lc 'cargo test -p pubpunk-cli'\nsucceeded in 827ms:\n",
        ));
    }

    #[test]
    fn reclassify_stalled_post_check_zero_progress_recovers_successful_check_tail() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-reclassify-post-check-stall-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/pubpunk-core/src")).unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            "pub fn validate() {}\n",
        )
        .unwrap();

        let snapshots = vec![EntryPointSnapshot {
            path: "crates/pubpunk-core/src/lib.rs".into(),
            content: Some("pub fn validate() {}\n".into()),
        }];

        let output = Command::new("/bin/sh")
            .arg("-lc")
            .arg(
                "printf \"exec\\n/bin/zsh -lc 'cargo test -p pubpunk-core'\\nsucceeded in 827ms:\\n\" 1>&2; exit 1",
            )
            .output()
            .unwrap();
        let timed_output = TimedOutput {
            output,
            timed_out: false,
            stalled: true,
            orphaned: false,
            no_progress_paths: Vec::new(),
            scaffold_only_paths: Vec::new(),
            post_check_zero_progress_paths: Vec::new(),
        };

        let reclassified =
            reclassify_stalled_post_check_zero_progress(&root, &snapshots, timed_output).unwrap();

        assert!(!reclassified.stalled);
        assert_eq!(
            reclassified.post_check_zero_progress_paths,
            vec!["crates/pubpunk-core/src/lib.rs".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reclassify_stalled_post_check_zero_progress_recovers_inspection_only_stall() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-reclassify-inspection-stall-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("tests/README.md"),
            "# Controller-owned bootstrap placeholder\n",
        )
        .unwrap();

        let snapshots = vec![EntryPointSnapshot {
            path: "tests/README.md".into(),
            content: Some("# Controller-owned bootstrap placeholder\n".into()),
        }];

        let output = Command::new("/bin/sh")
            .arg("-lc")
            .arg(
                "printf \"mcp: engram/mem_search (completed)\\nexec\\n/bin/zsh -lc \\\"find tests -maxdepth 3 -type f | sort\\\" in /tmp\\n succeeded in 0ms:\\n# Controller-owned bootstrap placeholder\\n\" 1>&2; exit 1",
            )
            .output()
            .unwrap();
        let timed_output = TimedOutput {
            output,
            timed_out: false,
            stalled: true,
            orphaned: false,
            no_progress_paths: Vec::new(),
            scaffold_only_paths: Vec::new(),
            post_check_zero_progress_paths: Vec::new(),
        };

        let reclassified =
            reclassify_stalled_post_check_zero_progress(&root, &snapshots, timed_output).unwrap();

        assert!(!reclassified.stalled);
        assert_eq!(
            reclassified.no_progress_paths,
            vec!["tests/README.md".to_string()]
        );
        assert!(reclassified.post_check_zero_progress_paths.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scaffold_only_classification_reports_precise_failure() {
        let (success, summary) = classify_scaffold_only_result(
            &[String::from("crates/punk-council/src/synthesis.rs")],
            "",
            "test result: ok. 0 passed; 0 failed;\n",
        );
        assert!(!success);
        assert!(summary.contains("no implementation progress beyond scaffold"));
        assert!(summary.contains("crates/punk-council/src/synthesis.rs"));
    }

    #[test]
    fn no_progress_classification_reports_precise_failure() {
        let (success, summary) = classify_no_progress_after_dispatch_result(
            &[String::from("crates/punk-council/src/synthesis.rs")],
            "",
            "",
        );
        assert!(!success);
        assert!(summary.contains("no implementation progress after bounded context dispatch"));
        assert!(summary.contains("crates/punk-council/src/synthesis.rs"));
    }

    #[test]
    fn no_progress_classification_blocks_missing_manifest_wiring() {
        let (success, summary) = classify_no_progress_after_dispatch_result(
            &[String::from("Cargo.toml")],
            "Cargo.toml -> MISSING\n",
            "",
        );
        assert!(!success);
        assert_eq!(
            summary,
            "PUNK_EXECUTION_BLOCKED: missing manifest wiring for Cargo.toml"
        );
    }

    #[test]
    fn manifest_probe_no_progress_skips_retry() {
        let contract = Contract {
            id: "ct_manifest".into(),
            feature_id: "feat_manifest".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "scaffold Rust workspace".into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec![],
            expected_interfaces: vec!["workspace scaffold".into()],
            behavior_requirements: vec!["bootstrap project".into()],
            allowed_scope: vec!["Cargo.toml".into(), "crates".into(), "tests".into()],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert!(!should_retry_after_no_progress(
            &contract,
            &[String::from("Cargo.toml")],
            "Cargo.toml -> MISSING\n",
            "",
        ));
    }

    #[test]
    fn greenfield_manifest_blocked_summary_reports_missing_scope_surfaces() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-greenfield-blocked-summary-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let contract = Contract {
            id: "ct_manifest".into(),
            feature_id: "feat_manifest".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "scaffold Rust workspace".into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec![],
            expected_interfaces: vec!["workspace scaffold".into()],
            behavior_requirements: vec!["bootstrap project".into()],
            allowed_scope: vec!["Cargo.toml".into(), "crates".into(), "tests".into()],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let summary = greenfield_manifest_blocked_summary(&root, &contract).unwrap();
        assert!(
            summary.contains("PUNK_EXECUTION_BLOCKED: missing manifest wiring in allowed scope")
        );
        assert!(summary.contains("Cargo.toml"));
        assert!(summary.contains("crates/"));
        assert!(summary.contains("tests/"));
        assert!(summary.contains("cargo test --workspace"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn materialize_rust_workspace_bootstrap_scaffold_creates_compile_ready_workspace() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-greenfield-bootstrap-materialize-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let contract = Contract {
            id: "ct_manifest".into(),
            feature_id: "feat_manifest".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "bootstrap initial Rust workspace for pubpunk".into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec!["crates/pubpunk-cli".into(), "crates/pubpunk-core".into()],
            expected_interfaces: vec!["initial Rust scaffold".into()],
            behavior_requirements: vec!["bootstrap project".into()],
            allowed_scope: vec![
                "Cargo.toml".into(),
                "crates/pubpunk-cli".into(),
                "crates/pubpunk-core".into(),
                "tests".into(),
            ],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let created = materialize_rust_workspace_bootstrap_scaffold(&root, &contract).unwrap();
        assert!(created.iter().any(|path| path == "Cargo.toml"));
        assert!(created
            .iter()
            .any(|path| path == "crates/pubpunk-cli/Cargo.toml"));
        assert!(created
            .iter()
            .any(|path| path == "crates/pubpunk-cli/src/main.rs"));
        assert!(created
            .iter()
            .any(|path| path == "crates/pubpunk-core/Cargo.toml"));
        assert!(created
            .iter()
            .any(|path| path == "crates/pubpunk-core/src/lib.rs"));
        assert!(created.iter().any(|path| path == "tests/README.md"));

        let status = Command::new("cargo")
            .args(["test", "--workspace"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(status.success());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn materialize_rust_workspace_bootstrap_scaffold_infers_generic_crate_dirs_from_cli_goal() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-greenfield-bootstrap-infer-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let contract = Contract {
            id: "ct_manifest".into(),
            feature_id: "feat_manifest".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "scaffold Rust workspace and implement pubpunk init + validate".into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec!["crates".into()],
            expected_interfaces: vec![
                "A Rust workspace rooted at `Cargo.toml`.".into(),
                "A `pubpunk` CLI exposing `init` and `validate` subcommands.".into(),
            ],
            behavior_requirements: vec![
                "Scaffold a minimal Rust workspace only.".into(),
                "Implement `pubpunk init` and `pubpunk validate` with conservative bounded behavior."
                    .into(),
            ],
            allowed_scope: vec!["Cargo.toml".into(), "crates".into(), "tests".into()],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let created = materialize_rust_workspace_bootstrap_scaffold(&root, &contract).unwrap();
        assert!(created.iter().any(|path| path == "Cargo.toml"));
        assert!(created
            .iter()
            .any(|path| path == "crates/pubpunk-cli/Cargo.toml"));
        assert!(created
            .iter()
            .any(|path| path == "crates/pubpunk-cli/src/main.rs"));
        assert!(created
            .iter()
            .any(|path| path == "crates/pubpunk-core/Cargo.toml"));
        assert!(created
            .iter()
            .any(|path| path == "crates/pubpunk-core/src/lib.rs"));
        assert!(created.iter().any(|path| path == "tests/README.md"));

        let status = Command::new("cargo")
            .args(["test", "--workspace"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(status.success());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn capture_entry_point_snapshots_includes_created_bootstrap_files() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-greenfield-bootstrap-snapshots-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let contract = Contract {
            id: "ct_manifest".into(),
            feature_id: "feat_manifest".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "bootstrap initial Rust workspace for pubpunk".into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec!["crates/pubpunk-cli".into(), "crates/pubpunk-core".into()],
            expected_interfaces: vec!["initial Rust scaffold".into()],
            behavior_requirements: vec!["bootstrap project".into()],
            allowed_scope: vec![
                "Cargo.toml".into(),
                "crates/pubpunk-cli".into(),
                "crates/pubpunk-core".into(),
                "tests".into(),
            ],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let created = materialize_rust_workspace_bootstrap_scaffold(&root, &contract).unwrap();
        let snapshots = capture_entry_point_snapshots(&root, &contract, &created).unwrap();
        let paths = snapshots
            .iter()
            .map(|snapshot| snapshot.path.as_str())
            .collect::<Vec<_>>();
        assert!(paths.contains(&"Cargo.toml"));
        assert!(paths.contains(&"crates/pubpunk-cli/Cargo.toml"));
        assert!(paths.contains(&"crates/pubpunk-cli/src/main.rs"));
        assert!(paths.contains(&"crates/pubpunk-core/Cargo.toml"));
        assert!(paths.contains(&"crates/pubpunk-core/src/lib.rs"));
        assert!(paths.contains(&"tests/README.md"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn capture_entry_point_snapshots_includes_existing_files_under_allowed_scope_dirs() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-snapshot-allowed-scope-files-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/pubpunk-cli/src")).unwrap();
        fs::create_dir_all(root.join("crates/pubpunk-core/src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/Cargo.toml"),
            "[package]\nname = \"pubpunk-cli\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/Cargo.toml"),
            "[package]\nname = \"pubpunk-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            "pub fn init() {}\n",
        )
        .unwrap();
        fs::write(root.join("tests/README.md"), "# tests\n").unwrap();

        let contract = Contract {
            id: "ct_manifest".into(),
            feature_id: "feat_manifest".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "implement pubpunk init".into(),
            entry_points: vec![
                "crates/pubpunk-cli/Cargo.toml".into(),
                "crates/pubpunk-core/Cargo.toml".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["bounded implementation slice".into()],
            behavior_requirements: vec!["implement pubpunk init".into()],
            allowed_scope: vec![
                "crates/pubpunk-cli".into(),
                "crates/pubpunk-core".into(),
                "tests".into(),
            ],
            target_checks: vec!["cargo test -p pubpunk-cli".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let snapshots = capture_entry_point_snapshots(&root, &contract, &[]).unwrap();
        let paths = snapshots
            .iter()
            .map(|snapshot| snapshot.path.as_str())
            .collect::<Vec<_>>();
        assert!(paths.contains(&"crates/pubpunk-cli/Cargo.toml"));
        assert!(paths.contains(&"crates/pubpunk-cli/src/main.rs"));
        assert!(paths.contains(&"crates/pubpunk-core/Cargo.toml"));
        assert!(paths.contains(&"crates/pubpunk-core/src/lib.rs"));
        assert!(paths.contains(&"tests/README.md"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_command_with_timeout_and_tee_detects_successful_check_stall_without_code_progress() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-tee-success-check-stall-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/pubpunk-cli/src")).unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/Cargo.toml"),
            "[package]\nname = \"pubpunk-cli\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();

        let snapshots = vec![
            EntryPointSnapshot {
                path: "crates/pubpunk-cli/Cargo.toml".into(),
                content: Some("[package]\nname = \"pubpunk-cli\"\nversion = \"0.1.0\"\n".into()),
            },
            EntryPointSnapshot {
                path: "crates/pubpunk-cli/src/main.rs".into(),
                content: Some("fn main() {}\n".into()),
            },
        ];

        let stdout_path = root.join("stdout.log");
        let stderr_path = root.join("stderr.log");
        let mut command = Command::new("/bin/sh");
        command
            .arg("-lc")
            .arg("printf 'Finished `test` profile\\n'; sleep 2");

        let output = run_command_with_timeout_and_tee(
            &mut command,
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(1),
            Duration::from_millis(400),
            Duration::from_secs(1),
            stdout_path,
            stderr_path,
            root.join("executor.json"),
            None,
            Some((&root, snapshots.as_slice())),
        )
        .unwrap();

        assert!(!output.timed_out);
        assert!(!output.stalled);
        assert!(!output.orphaned);
        assert!(output.no_progress_paths.is_empty());
        assert!(output.scaffold_only_paths.is_empty());
        assert_eq!(
            output.post_check_zero_progress_paths,
            vec![
                "crates/pubpunk-cli/Cargo.toml".to_string(),
                "crates/pubpunk-cli/src/main.rs".to_string()
            ]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn no_progress_only_in_controller_scaffold_matches_created_paths() {
        assert!(no_progress_only_in_controller_scaffold(
            &[
                "Cargo.toml".to_string(),
                "crates/pubpunk-cli/Cargo.toml".to_string(),
            ],
            &[
                "Cargo.toml".to_string(),
                "crates/pubpunk-cli/Cargo.toml".to_string(),
                "crates/pubpunk-cli/src/main.rs".to_string(),
            ],
        ));
        assert!(!no_progress_only_in_controller_scaffold(
            &["Cargo.toml".to_string(), "README.md".to_string()],
            &["Cargo.toml".to_string()],
        ));
    }

    #[test]
    fn directory_scoped_bounded_contracts_capture_progress_snapshots() {
        let contract = Contract {
            id: "ct_init".into(),
            feature_id: "feat_init".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "implement pubpunk init".into(),
            entry_points: vec![
                "crates/pubpunk-cli/Cargo.toml".into(),
                "crates/pubpunk-core/Cargo.toml".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["bounded implementation slice".into()],
            behavior_requirements: vec!["implement pubpunk init".into()],
            allowed_scope: vec![
                "crates/pubpunk-cli".into(),
                "crates/pubpunk-core".into(),
                "tests".into(),
            ],
            target_checks: vec!["cargo test -p pubpunk-cli".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert!(should_capture_progress_snapshots(&contract, &[]));
    }

    #[test]
    fn merged_contract_checks_dedupes_target_and_integrity_lists() {
        let contract = Contract {
            id: "ct_manifest".into(),
            feature_id: "feat_manifest".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "bootstrap".into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec![],
            expected_interfaces: vec![],
            behavior_requirements: vec![],
            allowed_scope: vec!["Cargo.toml".into()],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec![
                "cargo test --workspace".into(),
                "cargo test -p pubpunk-core".into(),
            ],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert_eq!(
            merged_contract_checks(&contract),
            vec![
                "cargo test --workspace".to_string(),
                "cargo test -p pubpunk-core".to_string(),
            ]
        );
    }

    #[test]
    fn post_check_zero_progress_classification_reports_precise_failure() {
        let (success, summary) = classify_post_check_zero_progress_result(
            &[String::from("crates/punk-vcs/src/lib.rs")],
            "",
            "0 tests, 0 benchmarks\n",
        );
        assert!(!success);
        assert!(summary.contains("no implementation progress after post-check stall"));
        assert!(summary.contains("crates/punk-vcs/src/lib.rs"));
    }

    #[test]
    fn orphan_classification_is_failure_even_without_timeout() {
        let (success, summary) = classify_orphan_result("done\n", "");
        assert!(!success);
        assert!(summary.contains("orphaned"));
    }
}
