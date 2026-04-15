//! Thin upstream-facing adapters for drafting and bounded execution.
//!
//! Boundary policy:
//! - adapters normalize provider/runtime IO into local artifact shapes
//! - adapters may add provider-specific preflight, guard rails, and failure classification
//! - adapters must not own scope policy, gate semantics, proof semantics, or a parallel
//!   universal agent runtime
//! - when an upstream-native capability becomes clearly better, the adapter layer should
//!   become thinner rather than growing new local machinery around it

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
use punk_domain::{Contract, DraftInput, DraftProposal, FrozenCapabilityResolution, RefineInput};
const BLOCKED_EXECUTION_SENTINEL: &str = "PUNK_EXECUTION_BLOCKED:";
const SUCCESSFUL_EXECUTION_SENTINEL: &str = "PUNK_EXECUTION_COMPLETE:";

pub struct ExecuteInput {
    pub repo_root: PathBuf,
    pub contract: Contract,
    pub capability_resolution: Option<FrozenCapabilityResolution>,
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
    stalled: bool,
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
    /// Stable provider-agnostic port for bounded execution.
    ///
    /// Implementations may wrap vendor CLIs, SDKs, or managed runtimes, but they must return
    /// local execution facts in `ExecuteOutput` rather than leaking provider-specific semantics
    /// into the core runtime.
    fn name(&self) -> &'static str;
    fn execute_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput>;
}

pub trait ContractDrafter {
    /// Stable provider-agnostic port for contract drafting and refinement.
    ///
    /// Implementations may use provider-native structured output, tools, or reasoning controls,
    /// but they must normalize results into `DraftProposal` and must not become the source of
    /// truth for local policy, scope law, or acceptance semantics.
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
        let effective_contract = effective_execution_contract(&input.repo_root, &input.contract)?;
        let effective_input = ExecuteInput {
            repo_root: input.repo_root.clone(),
            contract: effective_contract,
            capability_resolution: input.capability_resolution.clone(),
            stdout_path: input.stdout_path.clone(),
            stderr_path: input.stderr_path.clone(),
            executor_pid_path: input.executor_pid_path.clone(),
        };
        let start = Instant::now();

        if let Some(output) =
            maybe_execute_controller_pubpunk_cleanup_recipe(&effective_input, start)?
        {
            return Ok(output);
        }

        if let Some(output) = maybe_execute_controller_pubpunk_validate_file_parseability_recipe(
            &effective_input,
            start,
        )? {
            return Ok(output);
        }

        if let Some(output) =
            maybe_execute_controller_pubpunk_validate_parseability_recipe(&effective_input, start)?
        {
            return Ok(output);
        }

        if let Some(output) =
            maybe_execute_controller_pubpunk_validate_recipe(&effective_input, start)?
        {
            return Ok(output);
        }

        if let Some(output) = maybe_execute_controller_pubpunk_init_recipe(&effective_input, start)?
        {
            return Ok(output);
        }

        match execution_lane_for_contract(&effective_input.repo_root, &effective_input.contract) {
            ExecutionLane::Manual => {
                let summary = manual_mode_block_summary(&effective_input.contract)
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
                return self.execute_patch_apply_contract(effective_input);
            }
            ExecutionLane::Exec => {}
        }
        let start = Instant::now();
        let executor_timeout = effective_codex_executor_timeout(&effective_input.contract);
        restore_stale_entry_point_masks(&effective_input.repo_root)?;
        let created_entry_points = if is_fail_closed_scope_task(&effective_input.contract) {
            materialize_missing_entry_points(&effective_input.repo_root, &effective_input.contract)?
        } else {
            Vec::new()
        };
        let created_bootstrap_scaffold =
            materialize_controller_bootstrap_scaffold(&effective_input)?;
        let bootstrap_scaffold_paths = controller_bootstrap_scaffold_paths(&effective_input);
        let mut created_scaffold_paths = created_entry_points.clone();
        extend_unique_paths(&mut created_scaffold_paths, &created_bootstrap_scaffold);
        let mut attempt =
            self.run_execution_attempt(&effective_input, &created_scaffold_paths, false)?;
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
            && is_fail_closed_scope_task(&effective_input.contract)
        {
            let stdout = String::from_utf8_lossy(&attempt.timed_output.output.stdout);
            let stderr = String::from_utf8_lossy(&attempt.timed_output.output.stderr);
            if should_retry_after_no_progress(
                &effective_input.contract,
                &attempt.timed_output.no_progress_paths,
                &stdout,
                &stderr,
            ) {
                attempt =
                    self.run_execution_attempt(&effective_input, &created_scaffold_paths, true)?;
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
        fs::write(&effective_input.stdout_path, &timed_output.output.stdout)?;
        fs::write(&effective_input.stderr_path, &timed_output.output.stderr)?;
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
        let materialized_entry_points = if is_fail_closed_scope_task(&input.contract) {
            materialize_missing_entry_points(&input.repo_root, &input.contract)?
        } else {
            Vec::new()
        };
        let mut patch_progress_probe_paths = materialized_entry_points.clone();
        extend_unique_paths(
            &mut patch_progress_probe_paths,
            &controller_bootstrap_scaffold_paths(&input),
        );
        let patch_entry_point_snapshots =
            if should_capture_progress_snapshots(&input.contract, &patch_progress_probe_paths) {
                capture_entry_point_snapshots(
                    &input.repo_root,
                    &input.contract,
                    &patch_progress_probe_paths,
                )?
            } else {
                Vec::new()
            };
        let finalize_output = |output| {
            finalize_patch_lane_output(&input.repo_root, &patch_entry_point_snapshots, output)
        };
        let mut retry_feedback = None;
        let mut slow_patch_retry_used = false;
        let mut patched_worktree = false;
        let max_attempts = patch_apply_max_attempts(&input.contract);

        for attempt_index in 0..max_attempts {
            let retry_mode = attempt_index > 0;
            let mut context_pack = build_context_pack(&input.repo_root, &input.contract)?;
            if retry_mode {
                ensure_retry_patch_seed(&input.repo_root, &input.contract, &mut context_pack);
            }
            let has_patch_seed = context_pack.patch_seed.is_some();
            let controller_plan_seed = if has_patch_seed {
                ContextPlanSeed {
                    title: String::new(),
                    summary: String::new(),
                    targets: Vec::new(),
                }
            } else {
                let mut seed = derive_plan_seed(&input.contract, &context_pack);
                hydrate_plan_seed_excerpts(&context_pack, &mut seed);
                if !seed.targets.is_empty() {
                    context_pack.plan_seed = Some(seed.clone());
                }
                seed
            };
            let mut excerpt_guard = EntryPointExcerptGuard::apply(&input.repo_root, &context_pack)?;
            if !has_patch_seed && needs_patch_plan_prepass(&input.contract, &context_pack) {
                match self.run_patch_plan_prepass(&input, &context_pack) {
                    Err(err) => {
                        if let Some(guard) = excerpt_guard.as_mut() {
                            guard.restore()?;
                        }
                        append_log_text(
                            &input.stderr_path,
                            &format!("\n[punk patch/prepass] failed: {err}\n"),
                        )?;
                        let output = maybe_rollback_failed_patch_lane_output(
                            &input.repo_root,
                            &patch_entry_point_snapshots,
                            patched_worktree,
                            ExecuteOutput {
                                success: false,
                                summary: format!("patch prepass failed: {err}"),
                                checks_run: Vec::new(),
                                cost_usd: None,
                                duration_ms: start.elapsed().as_millis() as u64,
                            },
                        )?;
                        return Ok(output).and_then(finalize_output);
                    }
                    Ok(PlanPrepassResponse::Blocked(reason)) => {
                        if let Some(guard) = excerpt_guard.as_mut() {
                            guard.restore()?;
                        }
                        append_log_text(
                            &input.stderr_path,
                            &format!("\n[punk patch/prepass] blocked: {reason}\n"),
                        )?;
                        let output = maybe_rollback_failed_patch_lane_output(
                            &input.repo_root,
                            &patch_entry_point_snapshots,
                            patched_worktree,
                            ExecuteOutput {
                                success: false,
                                summary: reason,
                                checks_run: Vec::new(),
                                cost_usd: None,
                                duration_ms: start.elapsed().as_millis() as u64,
                            },
                        )?;
                        return Ok(output).and_then(finalize_output);
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
            let prompt = build_patch_apply_prompt(
                &input.contract,
                &context_pack,
                attempt_index,
                max_attempts,
                retry_feedback.as_deref(),
            );

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
                codex_executor_stall_timeout(),
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
                    let _ = restore_entry_point_snapshots(
                        &input.repo_root,
                        &patch_entry_point_snapshots,
                    );
                    let _ = restore_damaged_entry_point_snapshots(
                        &input.repo_root,
                        &patch_entry_point_snapshots,
                    );
                    return Err(err);
                }
            };
            let stdout = String::from_utf8_lossy(&timed_output.output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&timed_output.output.stderr).to_string();
            let response = timed_output
                .response
                .clone()
                .or_else(|| load_patch_lane_response(&stdout, &stderr).ok());
            if (timed_output.timed_out || timed_output.stalled) && response.is_none() {
                let summary = if timed_output.stalled {
                    stall_summary(codex_executor_stall_timeout(), &stdout, &stderr)
                } else {
                    timeout_summary(codex_patch_lane_timeout(), &stdout, &stderr)
                };
                if !slow_patch_retry_used {
                    slow_patch_retry_used = true;
                    retry_feedback = Some(patch_apply_retry_feedback(
                        &summary,
                        &input.repo_root,
                        &input.contract.allowed_scope,
                        &input.stdout_path,
                        &input.stderr_path,
                    )?);
                    append_log_text(
                        &input.stderr_path,
                        &format!(
                            "\n[punk patch/apply] retrying after {} with bounded repair context (pass {} of {})\n",
                            if timed_output.stalled {
                                "no-output stall"
                            } else {
                                "timeout"
                            },
                            attempt_index + 2,
                            max_attempts
                        ),
                    )?;
                    continue;
                }
                if !patch_entry_point_snapshots.is_empty() {
                    let unchanged_paths = unchanged_entry_point_paths(
                        &input.repo_root,
                        &patch_entry_point_snapshots,
                    )?;
                    if !unchanged_paths.is_empty()
                        && unchanged_paths.len() == patch_entry_point_snapshots.len()
                    {
                        let output = maybe_rollback_failed_patch_lane_output(
                            &input.repo_root,
                            &patch_entry_point_snapshots,
                            patched_worktree,
                            ExecuteOutput {
                                success: false,
                                summary: no_progress_after_dispatch_summary(
                                    &unchanged_paths,
                                    &stdout,
                                    &stderr,
                                ),
                                checks_run: Vec::new(),
                                cost_usd: None,
                                duration_ms: start.elapsed().as_millis() as u64,
                            },
                        )?;
                        return Ok(output).and_then(finalize_output);
                    }
                }
                let output = maybe_rollback_failed_patch_lane_output(
                    &input.repo_root,
                    &patch_entry_point_snapshots,
                    patched_worktree,
                    ExecuteOutput {
                        success: false,
                        summary,
                        checks_run: Vec::new(),
                        cost_usd: None,
                        duration_ms: start.elapsed().as_millis() as u64,
                    },
                )?;
                return Ok(output).and_then(finalize_output);
            }
            if timed_output.orphaned {
                let output = maybe_rollback_failed_patch_lane_output(
                    &input.repo_root,
                    &patch_entry_point_snapshots,
                    patched_worktree,
                    ExecuteOutput {
                        success: false,
                        summary: orphan_summary(&stdout, &stderr),
                        checks_run: Vec::new(),
                        cost_usd: None,
                        duration_ms: start.elapsed().as_millis() as u64,
                    },
                )?;
                return Ok(output).and_then(finalize_output);
            }
            if !timed_output.output.status.success() && response.is_none() {
                let (success, summary) = classify_execution_result(false, &stdout, &stderr);
                let output = maybe_rollback_failed_patch_lane_output(
                    &input.repo_root,
                    &patch_entry_point_snapshots,
                    patched_worktree,
                    ExecuteOutput {
                        success,
                        summary,
                        checks_run: Vec::new(),
                        cost_usd: None,
                        duration_ms: start.elapsed().as_millis() as u64,
                    },
                )?;
                return Ok(output).and_then(finalize_output);
            }

            let response = match response {
                Some(response) => response,
                None => {
                    let err = anyhow!("patch lane returned no complete patch artifact");
                    append_log_text(
                        &input.stderr_path,
                        &format!("\n[punk patch/apply] failed to parse patch output: {err}\n"),
                    )?;
                    let output = maybe_rollback_failed_patch_lane_output(
                        &input.repo_root,
                        &patch_entry_point_snapshots,
                        patched_worktree,
                        ExecuteOutput {
                            success: false,
                            summary: format!("patch/apply lane returned invalid patch text: {err}"),
                            checks_run: Vec::new(),
                            cost_usd: None,
                            duration_ms: start.elapsed().as_millis() as u64,
                        },
                    )?;
                    return Ok(output).and_then(finalize_output);
                }
            };
            let patch = match response {
                PatchLaneResponse::Blocked(reason) => {
                    append_log_text(
                        &input.stderr_path,
                        &format!("\n[punk patch/apply] blocked: {reason}\n"),
                    )?;
                    let output = maybe_rollback_failed_patch_lane_output(
                        &input.repo_root,
                        &patch_entry_point_snapshots,
                        patched_worktree,
                        ExecuteOutput {
                            success: false,
                            summary: reason,
                            checks_run: Vec::new(),
                            cost_usd: None,
                            duration_ms: start.elapsed().as_millis() as u64,
                        },
                    )?;
                    return Ok(output).and_then(finalize_output);
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
                    let output = maybe_rollback_failed_patch_lane_output(
                        &input.repo_root,
                        &patch_entry_point_snapshots,
                        patched_worktree,
                        ExecuteOutput {
                            success: false,
                            summary: format!("patch/apply lane rejected patch: {err}"),
                            checks_run: Vec::new(),
                            cost_usd: None,
                            duration_ms: start.elapsed().as_millis() as u64,
                        },
                    )?;
                    return Ok(output).and_then(finalize_output);
                }
            };
            let patch_paths = updates
                .iter()
                .map(|update| update.path.clone())
                .collect::<Vec<_>>();

            if let Err(err) = apply_patch_in_repo(&input.repo_root, &updates) {
                let detail = err.to_string();
                append_log_text(
                    &input.stderr_path,
                    &format!("\n[punk patch/apply] failed to apply patch: {detail}\n"),
                )?;
                let output = maybe_rollback_failed_patch_lane_output(
                    &input.repo_root,
                    &patch_entry_point_snapshots,
                    patched_worktree,
                    ExecuteOutput {
                        success: false,
                        summary: if detail.starts_with(BLOCKED_EXECUTION_SENTINEL) {
                            detail
                        } else {
                            format!("patch/apply lane failed to apply patch: {detail}")
                        },
                        checks_run: Vec::new(),
                        cost_usd: None,
                        duration_ms: start.elapsed().as_millis() as u64,
                    },
                )?;
                return Ok(output).and_then(finalize_output);
            }
            patched_worktree = true;
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
                    if should_retry_patch_apply_after_check_failure(
                        &input.contract,
                        &summary,
                        attempt_index,
                        max_attempts,
                    ) {
                        retry_feedback = Some(patch_apply_retry_feedback(
                            &summary,
                            &input.repo_root,
                            &input.contract.allowed_scope,
                            &input.stdout_path,
                            &input.stderr_path,
                        )?);
                        append_log_text(
                            &input.stderr_path,
                            &format!(
                                "\n[punk patch/apply] retrying with bounded repair context (pass {} of {})\n",
                                attempt_index + 2,
                                max_attempts
                            ),
                        )?;
                        continue;
                    }
                    let output = maybe_rollback_failed_patch_lane_output(
                        &input.repo_root,
                        &patch_entry_point_snapshots,
                        patched_worktree,
                        ExecuteOutput {
                            success: false,
                            summary,
                            checks_run: Vec::new(),
                            cost_usd: None,
                            duration_ms: start.elapsed().as_millis() as u64,
                        },
                    )?;
                    return Ok(output).and_then(finalize_output);
                }
            };

            return Ok(ExecuteOutput {
                success: true,
                summary: format!(
                    "{SUCCESSFUL_EXECUTION_SENTINEL} patch/apply lane succeeded after applying patch for {}",
                    patch_paths.join(", ")
                ),
                checks_run,
                cost_usd: None,
                duration_ms: start.elapsed().as_millis() as u64,
            })
            .and_then(finalize_output);
        }

        let output = maybe_rollback_failed_patch_lane_output(
            &input.repo_root,
            &patch_entry_point_snapshots,
            patched_worktree,
            ExecuteOutput {
                success: false,
                summary: format!(
                    "patch/apply lane exhausted repair attempts after {max_attempts} passes"
                ),
                checks_run: Vec::new(),
                cost_usd: None,
                duration_ms: start.elapsed().as_millis() as u64,
            },
        )?;
        Ok(output).and_then(finalize_output)
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
            &controller_bootstrap_scaffold_paths(input),
        );
        let entry_point_snapshots = if should_capture_progress_snapshots(
            &input.contract,
            progress_probe_paths.as_slice(),
        ) {
            capture_entry_point_snapshots(&input.repo_root, &input.contract, &progress_probe_paths)?
        } else {
            Vec::new()
        };

        let mut visible_allowed_files = entry_point_snapshots
            .iter()
            .map(|snapshot| snapshot.path.clone())
            .collect::<Vec<_>>();
        visible_allowed_files.dedup();

        let prompt = build_exec_prompt_with_mode_and_visible_files(
            &input.contract,
            context_pack.as_ref(),
            created_scaffold_paths,
            retry_mode,
            visible_allowed_files.as_slice(),
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
                let _ = restore_controller_bootstrap_scaffold(input, created_scaffold_paths);
                return Err(err);
            }
        };

        let restored_paths = restore_missing_materialized_entry_points(
            &input.repo_root,
            &input.contract,
            created_scaffold_paths,
        )?;
        let restored_bootstrap_paths =
            restore_controller_bootstrap_scaffold(input, created_scaffold_paths)?;
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
    build_exec_prompt_with_mode_and_visible_files(
        contract,
        context_pack,
        created_scaffold_paths,
        false,
        &[],
    )
}

fn build_exec_prompt_with_mode_and_visible_files(
    contract: &Contract,
    context_pack: Option<&ContextPack>,
    created_scaffold_paths: &[String],
    retry_mode: bool,
    visible_allowed_files: &[String],
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
    let visible_allowed_files_section = if visible_allowed_files.is_empty() {
        String::new()
    } else {
        format!(
            "Current allowed files available for direct edit: {}.\nFor this directory-scoped slice, treat these listed files as the initial bounded edit set before doing any more orientation.\n",
            visible_allowed_files.join(", ")
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
            "Implement the approved contract in the current repo. Contract id: {}. Original goal: {}. Treat the original goal as authoritative if the behavior requirements below are abbreviated. Behavior requirements: {}. Stay narrowly scoped to the contract and do not perform broad repo-wide search unless a concrete compile or verification blocker requires it. {}{} {} {} {} If you are blocked by scope, missing manifest wiring, or a similar execution blocker, do not ask the operator a question. Instead emit exactly one single-line sentinel in the form `{}` and stop without claiming success. When implementation is complete and all required checks are done, emit exactly one single-line sentinel in the form `{}`. If scope is unclear but not blocked, make the smallest safe change and explain what remains unspecified.",
            contract.id,
            contract.prompt_source,
            contract.behavior_requirements.join("; "),
            format!("{created_entry_points_section}{visible_allowed_files_section}{context_pack_section}"),
            retry_section,
            scope_rule,
            meta_workflow_rule,
            vcs_restore_rule,
            blocked_execution_template(),
            successful_execution_template()
        );
    }
    format!(
        "Implement the approved contract in the current repo.\nContract id: {}\nOriginal goal: {}\nTreat the original goal as authoritative if the behavior requirements below are abbreviated.\nBehavior requirements: {}\nAllowed scope: {}\nEntry points: {}\nExpected interfaces: {}\nTarget checks to satisfy: {}\nIntegrity checks to keep passing: {}\n{}{}\nStart by inspecting only the listed entry points and other files inside allowed scope.\nDo not perform broad repo-wide search.\n{}\n{}\n{}\nIf you are blocked by scope, missing manifest wiring, or a similar execution blocker, do not ask the operator a question. Instead emit exactly one single-line sentinel in the form `{}` and stop without claiming success.\nWhen implementation is complete and all required checks are done, emit exactly one single-line sentinel in the form `{}`.\nOnly modify files inside allowed scope.",
        contract.id,
        contract.prompt_source,
        contract.behavior_requirements.join("; "),
        contract.allowed_scope.join(", "),
        contract.entry_points.join(", "),
        contract.expected_interfaces.join("; "),
        contract.target_checks.join("; "),
        contract.integrity_checks.join("; "),
        format!(
            "{created_entry_points_section}{visible_allowed_files_section}{context_pack_section}"
        ),
        retry_section,
        scope_rule,
        meta_workflow_rule,
        vcs_restore_rule,
        blocked_execution_template(),
        successful_execution_template(),
    )
}

fn build_patch_apply_prompt(
    contract: &Contract,
    context_pack: &ContextPack,
    attempt_index: usize,
    max_attempts: usize,
    retry_feedback: Option<&str>,
) -> String {
    let context_pack_section = format_patch_context_pack(context_pack);
    let plan_rule = if context_pack.plan_seed.is_some() {
        "- if the controller-owned plan prepass is present, treat its target files, symbols, and insertion points as authoritative and produce the patch directly against that plan\n"
    } else {
        ""
    };
    let retry_rule = if attempt_index > 0 {
        let repair_pass = attempt_index + 1;
        format!(
            "- this is patch/apply repair pass {repair_pass} of {max_attempts} after a failed prior pass; repair the current in-scope files in place instead of restarting from scratch\n\
- repair only the concrete issue described in the feedback below; keep the patch surgical and do not re-add modules, tests, or helpers that already exist in the current file state\n\
- if the feedback reports duplicate definitions or duplicate test modules, remove or merge the duplicate instead of adding another copy\n\
- if the feedback includes current file snippets around failing lines, treat those snippets as the authoritative current state over any earlier baseline excerpt\n"
        )
    } else {
        String::new()
    };
    let retry_feedback_section = retry_feedback
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("Latest repair feedback:\n```text\n{}\n```\n", value.trim()))
        .unwrap_or_default();
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
        retry_feedback_section,
        plan_rule,
        retry_rule,
        blocked = blocked_execution_template(),
    )
}

fn patch_apply_max_attempts(contract: &Contract) -> usize {
    1 + merged_contract_checks(contract).len().min(2)
}

fn should_retry_patch_apply_after_check_failure(
    contract: &Contract,
    summary: &str,
    attempt_index: usize,
    max_attempts: usize,
) -> bool {
    is_fail_closed_scope_task(contract)
        && attempt_index + 1 < max_attempts
        && summary
            .trim_start()
            .starts_with("patch/apply lane check failed:")
}

fn patch_apply_retry_feedback(
    summary: &str,
    repo_root: &Path,
    allowed_scope: &[String],
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<String> {
    let stdout = fs::read_to_string(stdout_path).unwrap_or_default();
    let stderr = fs::read_to_string(stderr_path).unwrap_or_default();
    let stdout_tail = log_tail(&stdout, 60, 4000);
    let stderr_tail = log_tail(&stderr, 80, 6000);
    let mut sections = vec![format!("Summary: {}", summary.trim())];
    let repair_directives = patch_apply_repair_directives(summary, &stderr);
    if !repair_directives.is_empty() {
        sections.push(format!(
            "Repair directives:\n{}",
            repair_directives
                .into_iter()
                .map(|directive| format!("- {directive}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    let source_snippets =
        patch_apply_retry_source_snippets(repo_root, allowed_scope, &stderr).unwrap_or_default();
    if !source_snippets.is_empty() {
        sections.push(format!(
            "Current source near reported failures:\n{}",
            source_snippets
        ));
    }
    if !stdout_tail.is_empty() {
        sections.push(format!("Recent stdout:\n{}", stdout_tail));
    }
    if !stderr_tail.is_empty() {
        sections.push(format!("Recent stderr:\n{}", stderr_tail));
    }
    Ok(sections.join("\n\n"))
}

fn log_tail(text: &str, max_lines: usize, max_chars: usize) -> String {
    let mut lines = text.lines().rev().take(max_lines).collect::<Vec<_>>();
    lines.reverse();
    let joined = lines.join("\n");
    if joined.chars().count() <= max_chars {
        return joined;
    }
    let tail = joined
        .chars()
        .rev()
        .take(max_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("...{}", tail)
}

fn patch_apply_repair_directives(summary: &str, stderr: &str) -> Vec<String> {
    let mut directives =
        vec!["Repair only the reported issue in the current in-scope files.".into()];
    let lower = format!("{summary}\n{stderr}").to_ascii_lowercase();
    if (lower.contains("timed out") && lower.contains("before emitting a patch"))
        || (lower.contains("timed out") && lower.contains("apply_patch"))
    {
        directives.push(
            "Emit the smallest valid apply_patch response immediately; do not spend this pass rereading context or reformulating the task."
                .into(),
        );
    }
    if lower.contains("defined multiple times") || lower.contains("error[e0428]") {
        directives.push(
            "Keep exactly one definition for the reported item; remove or merge the duplicate instead of adding another copy."
                .into(),
        );
    }
    if (lower.contains("mod tests") || lower.contains("name `tests`"))
        && (lower.contains("defined multiple times") || lower.contains("error[e0428]"))
    {
        directives.push(
            "Keep exactly one `mod tests` block per file and edit the existing block in place."
                .into(),
        );
    }
    if lower.contains("not bound in all patterns")
        || lower.contains("possibly-uninitialized")
        || lower.contains("error[e0408]")
        || lower.contains("error[e0381]")
    {
        directives.push(
            "Fix the reported match arm or binding in place; do not rewrite unrelated code paths."
                .into(),
        );
    }
    directives
}

fn patch_apply_retry_source_snippets(
    repo_root: &Path,
    allowed_scope: &[String],
    stderr: &str,
) -> Result<String> {
    let mut seen = Vec::new();
    let mut snippets = Vec::new();
    for line in stderr.lines() {
        let Some((raw_path, line_number)) = parse_rust_diagnostic_location(line) else {
            continue;
        };
        let Some(path) = normalize_retry_feedback_path(repo_root, raw_path) else {
            continue;
        };
        if !path_is_in_allowed_scope(&path, allowed_scope) {
            continue;
        }
        if seen
            .iter()
            .any(|(seen_path, seen_line)| seen_path == &path && *seen_line == line_number)
        {
            continue;
        }
        seen.push((path.clone(), line_number));
        let snippet = read_source_snippet_around_line(repo_root, &path, line_number, 6)?;
        if !snippet.is_empty() {
            snippets.push(format!("{path} around line {line_number}\n{snippet}"));
        }
        if snippets.len() >= 4 {
            break;
        }
    }
    Ok(snippets.join("\n\n"))
}

fn parse_rust_diagnostic_location(line: &str) -> Option<(&str, usize)> {
    let rest = line.trim().strip_prefix("--> ")?;
    let mut parts = rest.splitn(3, ':');
    let path = parts.next()?.trim();
    let line_number = parts.next()?.trim().parse().ok()?;
    Some((path, line_number))
}

fn normalize_retry_feedback_path(repo_root: &Path, raw_path: &str) -> Option<String> {
    if !Path::new(raw_path).is_absolute() {
        return Some(raw_path.replace('\\', "/"));
    }
    Path::new(raw_path)
        .strip_prefix(repo_root)
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}

fn read_source_snippet_around_line(
    repo_root: &Path,
    rel_path: &str,
    line_number: usize,
    radius: usize,
) -> Result<String> {
    let source = match fs::read_to_string(repo_root.join(rel_path)) {
        Ok(source) => source,
        Err(_) => return Ok(String::new()),
    };
    let lines = source.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return Ok(String::new());
    }
    let start = line_number.saturating_sub(radius).max(1);
    let end = (line_number + radius).min(lines.len());
    let snippet = (start..=end)
        .map(|current| format!("{:>4} | {}", current, lines[current - 1]))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("```rust\n{}\n```", snippet))
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

fn materialize_controller_bootstrap_scaffold(input: &ExecuteInput) -> Result<Vec<String>> {
    let Some(files) = controller_bootstrap_scaffold_templates(input) else {
        return Ok(Vec::new());
    };
    let mut created = Vec::new();
    for (path, contents) in files {
        if !path_is_in_allowed_scope(&path, &input.contract.allowed_scope) {
            continue;
        }
        let file_path = input.repo_root.join(&path);
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

fn restore_controller_bootstrap_scaffold(
    input: &ExecuteInput,
    created_paths: &[String],
) -> Result<Vec<String>> {
    let Some(files) = controller_bootstrap_scaffold_templates(input) else {
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
        let file_path = input.repo_root.join(path);
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

fn controller_bootstrap_scaffold_kind(input: &ExecuteInput) -> Option<&str> {
    input
        .capability_resolution
        .as_ref()
        .and_then(|resolution| resolution.controller_scaffold_kind.as_deref())
}

fn controller_bootstrap_scaffold_templates(input: &ExecuteInput) -> Option<Vec<(String, String)>> {
    match controller_bootstrap_scaffold_kind(input) {
        Some("rust-cargo") => rust_workspace_bootstrap_templates(&input.contract),
        Some("go-mod") => go_module_bootstrap_templates(&input.contract),
        Some("python-pyproject-pytest") => python_package_bootstrap_templates(&input.contract),
        Some(_) => None,
        None => legacy_controller_bootstrap_scaffold_templates(&input.contract),
    }
}

fn legacy_controller_bootstrap_scaffold_templates(
    contract: &Contract,
) -> Option<Vec<(String, String)>> {
    rust_workspace_bootstrap_templates(contract)
        .or_else(|| go_module_bootstrap_templates(contract))
        .or_else(|| python_package_bootstrap_templates(contract))
}

#[allow(dead_code)]
fn materialize_rust_workspace_bootstrap_scaffold(
    repo_root: &Path,
    contract: &Contract,
) -> Result<Vec<String>> {
    materialize_controller_bootstrap_scaffold(&ExecuteInput {
        repo_root: repo_root.to_path_buf(),
        contract: contract.clone(),
        capability_resolution: None,
        stdout_path: PathBuf::new(),
        stderr_path: PathBuf::new(),
        executor_pid_path: PathBuf::new(),
    })
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

fn go_module_bootstrap_templates(contract: &Contract) -> Option<Vec<(String, String)>> {
    if contract.entry_points != vec!["go.mod".to_string()] {
        return None;
    }
    if !contract_has_check(contract, "go test ./...") {
        return None;
    }
    if !contract.allowed_scope.iter().any(|scope| scope == "cmd") {
        return None;
    }
    let slug = infer_rust_bootstrap_app_slug(contract).unwrap_or_else(|| "app".to_string());
    let module_slug = normalize_go_module_slug(&slug);
    let command_dir = contract
        .allowed_scope
        .iter()
        .find(|scope| scope.starts_with("cmd/") && !scope.contains('.'))
        .cloned()
        .unwrap_or_else(|| format!("cmd/{slug}"));
    let mut files = vec![
        (
            "go.mod".to_string(),
            render_go_module_manifest(&module_slug),
        ),
        (
            format!("{command_dir}/main.go"),
            "package main\n\nfunc main() {}\n".to_string(),
        ),
    ];
    if contract
        .allowed_scope
        .iter()
        .any(|scope| scope == "internal")
    {
        files.push((
            "internal/bootstrap/bootstrap.go".to_string(),
            "package bootstrap\n\nfunc Ready() bool {\n\treturn true\n}\n".to_string(),
        ));
    }
    if contract.allowed_scope.iter().any(|scope| scope == "pkg") {
        files.push((
            "pkg/bootstrap/bootstrap.go".to_string(),
            "package bootstrap\n\nfunc Ready() bool {\n\treturn true\n}\n".to_string(),
        ));
    }
    Some(files)
}

fn python_package_bootstrap_templates(contract: &Contract) -> Option<Vec<(String, String)>> {
    if contract.entry_points != vec!["pyproject.toml".to_string()] {
        return None;
    }
    if !contract_has_check(contract, "pytest") {
        return None;
    }
    if !contract.allowed_scope.iter().any(|scope| scope == "src")
        || !contract.allowed_scope.iter().any(|scope| scope == "tests")
    {
        return None;
    }
    let slug = infer_rust_bootstrap_app_slug(contract).unwrap_or_else(|| "app".to_string());
    let distribution_name = slug.replace('-', "_");
    Some(vec![
        (
            "pyproject.toml".to_string(),
            render_python_pyproject(&distribution_name),
        ),
        (
            format!("src/{distribution_name}/__init__.py"),
            "__all__ = [\"ready\"]\n\n\ndef ready() -> bool:\n    return True\n".to_string(),
        ),
        (
            "tests/test_bootstrap.py".to_string(),
            format!(
                "from {distribution_name} import ready\n\n\ndef test_bootstrap_smoke() -> None:\n    assert ready() is True\n"
            ),
        ),
    ])
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
    candidates.extend(extract_prompt_bootstrap_targets(&contract.prompt_source));
    candidates.extend(extract_backticked_identifiers(&contract.prompt_source));
    for item in &contract.expected_interfaces {
        if let Some(cli_name) = extract_cli_name(item) {
            candidates.push(cli_name);
        }
        candidates.extend(extract_backticked_identifiers(item));
    }
    for item in &contract.behavior_requirements {
        if let Some(cli_name) = extract_cli_name(item) {
            candidates.push(cli_name);
        }
        candidates.extend(extract_backticked_identifiers(item));
    }
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
        .find(|window| {
            window[1] == "cli"
                && !matches!(window[0].as_str(), "a" | "an" | "the")
                && is_viable_bootstrap_app_slug(&window[0])
        })
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
        || normalized == "a"
        || normalized == "an"
        || normalized == "the"
    {
        return false;
    }
    normalized
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn controller_bootstrap_scaffold_paths(input: &ExecuteInput) -> Vec<String> {
    controller_bootstrap_scaffold_templates(input)
        .map(|files| files.into_iter().map(|(path, _)| path).collect())
        .unwrap_or_default()
}

fn legacy_controller_bootstrap_scaffold_paths(contract: &Contract) -> Vec<String> {
    legacy_controller_bootstrap_scaffold_templates(contract)
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

fn render_go_module_manifest(module_slug: &str) -> String {
    format!("module example.com/{module_slug}\n\ngo 1.22\n")
}

fn normalize_go_module_slug(slug: &str) -> String {
    slug.trim().trim_matches('/').replace('_', "-")
}

fn render_python_pyproject(distribution_name: &str) -> String {
    format!(
        "[build-system]\nrequires = [\"setuptools>=68\"]\nbuild-backend = \"setuptools.build_meta\"\n\n[project]\nname = \"{distribution_name}\"\nversion = \"0.1.0\"\ndependencies = []\n\n[tool.pytest.ini_options]\npythonpath = [\"src\"]\n"
    )
}

fn contract_has_check(contract: &Contract, expected: &str) -> bool {
    contract
        .target_checks
        .iter()
        .chain(contract.integrity_checks.iter())
        .any(|check| check.trim() == expected)
}

fn maybe_execute_controller_pubpunk_init_recipe(
    input: &ExecuteInput,
    start: Instant,
) -> Result<Option<ExecuteOutput>> {
    let Some(updated_paths) =
        apply_controller_pubpunk_init_recipe(&input.repo_root, &input.contract)?
    else {
        return Ok(None);
    };

    let checks = collect_contract_checks(&input.contract);
    match run_contract_checks(
        &input.repo_root,
        &input.contract,
        &checks,
        &input.stdout_path,
        &input.stderr_path,
    ) {
        Ok(checks_run) => {
            let summary = if updated_paths.is_empty() {
                "PUNK_EXECUTION_COMPLETE: controller pubpunk init recipe already satisfied and checks passed"
                    .to_string()
            } else {
                format!(
                    "PUNK_EXECUTION_COMPLETE: controller pubpunk init recipe applied and checks passed for {}",
                    updated_paths.join(", ")
                )
            };
            Ok(Some(ExecuteOutput {
                success: true,
                summary,
                checks_run,
                cost_usd: None,
                duration_ms: start.elapsed().as_millis() as u64,
            }))
        }
        Err(err) => Ok(Some(ExecuteOutput {
            success: false,
            summary: format!(
                "controller pubpunk init recipe applied but verification failed: {err}"
            ),
            checks_run: Vec::new(),
            cost_usd: None,
            duration_ms: start.elapsed().as_millis() as u64,
        })),
    }
}

fn maybe_execute_controller_pubpunk_cleanup_recipe(
    input: &ExecuteInput,
    start: Instant,
) -> Result<Option<ExecuteOutput>> {
    let Some(updated_paths) =
        apply_controller_pubpunk_cleanup_recipe(&input.repo_root, &input.contract)?
    else {
        return Ok(None);
    };

    let checks = collect_contract_checks(&input.contract);
    match run_contract_checks(
        &input.repo_root,
        &input.contract,
        &checks,
        &input.stdout_path,
        &input.stderr_path,
    ) {
        Ok(checks_run) => {
            let summary = if updated_paths.is_empty() {
                "PUNK_EXECUTION_COMPLETE: controller pubpunk cleanup recipe already satisfied and checks passed"
                    .to_string()
            } else {
                format!(
                    "PUNK_EXECUTION_COMPLETE: controller pubpunk cleanup recipe applied and checks passed for {}",
                    updated_paths.join(", ")
                )
            };
            Ok(Some(ExecuteOutput {
                success: true,
                summary,
                checks_run,
                cost_usd: None,
                duration_ms: start.elapsed().as_millis() as u64,
            }))
        }
        Err(err) => Ok(Some(ExecuteOutput {
            success: false,
            summary: format!(
                "controller pubpunk cleanup recipe applied but verification failed: {err}"
            ),
            checks_run: Vec::new(),
            cost_usd: None,
            duration_ms: start.elapsed().as_millis() as u64,
        })),
    }
}

fn maybe_execute_controller_pubpunk_validate_parseability_recipe(
    input: &ExecuteInput,
    start: Instant,
) -> Result<Option<ExecuteOutput>> {
    let Some(updated_paths) =
        apply_controller_pubpunk_validate_parseability_recipe(&input.repo_root, &input.contract)?
    else {
        return Ok(None);
    };

    let checks = collect_contract_checks(&input.contract);
    match run_contract_checks(
        &input.repo_root,
        &input.contract,
        &checks,
        &input.stdout_path,
        &input.stderr_path,
    ) {
        Ok(checks_run) => {
            let summary = if updated_paths.is_empty() {
                "PUNK_EXECUTION_COMPLETE: controller pubpunk validate parseability recipe already satisfied and checks passed"
                    .to_string()
            } else {
                format!(
                    "PUNK_EXECUTION_COMPLETE: controller pubpunk validate parseability recipe applied and checks passed for {}",
                    updated_paths.join(", ")
                )
            };
            Ok(Some(ExecuteOutput {
                success: true,
                summary,
                checks_run,
                cost_usd: None,
                duration_ms: start.elapsed().as_millis() as u64,
            }))
        }
        Err(err) => Ok(Some(ExecuteOutput {
            success: false,
            summary: format!(
                "controller pubpunk validate parseability recipe applied but verification failed: {err}"
            ),
            checks_run: Vec::new(),
            cost_usd: None,
            duration_ms: start.elapsed().as_millis() as u64,
        })),
    }
}

fn maybe_execute_controller_pubpunk_validate_file_parseability_recipe(
    input: &ExecuteInput,
    start: Instant,
) -> Result<Option<ExecuteOutput>> {
    let Some(updated_paths) = apply_controller_pubpunk_validate_file_parseability_recipe(
        &input.repo_root,
        &input.contract,
    )?
    else {
        return Ok(None);
    };

    let checks = collect_contract_checks(&input.contract);
    match run_contract_checks(
        &input.repo_root,
        &input.contract,
        &checks,
        &input.stdout_path,
        &input.stderr_path,
    ) {
        Ok(checks_run) => {
            let summary = if updated_paths.is_empty() {
                "PUNK_EXECUTION_COMPLETE: controller pubpunk validate file parseability recipe already satisfied and checks passed"
                    .to_string()
            } else {
                format!(
                    "PUNK_EXECUTION_COMPLETE: controller pubpunk validate file parseability recipe applied and checks passed for {}",
                    updated_paths.join(", ")
                )
            };
            Ok(Some(ExecuteOutput {
                success: true,
                summary,
                checks_run,
                cost_usd: None,
                duration_ms: start.elapsed().as_millis() as u64,
            }))
        }
        Err(err) => Ok(Some(ExecuteOutput {
            success: false,
            summary: format!(
                "controller pubpunk validate file parseability recipe applied but verification failed: {err}"
            ),
            checks_run: Vec::new(),
            cost_usd: None,
            duration_ms: start.elapsed().as_millis() as u64,
        })),
    }
}

fn maybe_execute_controller_pubpunk_validate_recipe(
    input: &ExecuteInput,
    start: Instant,
) -> Result<Option<ExecuteOutput>> {
    let Some(updated_paths) =
        apply_controller_pubpunk_validate_recipe(&input.repo_root, &input.contract)?
    else {
        return Ok(None);
    };

    let checks = collect_contract_checks(&input.contract);
    match run_contract_checks(
        &input.repo_root,
        &input.contract,
        &checks,
        &input.stdout_path,
        &input.stderr_path,
    ) {
        Ok(checks_run) => {
            let summary = if updated_paths.is_empty() {
                "PUNK_EXECUTION_COMPLETE: controller pubpunk validate recipe already satisfied and checks passed"
                    .to_string()
            } else {
                format!(
                    "PUNK_EXECUTION_COMPLETE: controller pubpunk validate recipe applied and checks passed for {}",
                    updated_paths.join(", ")
                )
            };
            Ok(Some(ExecuteOutput {
                success: true,
                summary,
                checks_run,
                cost_usd: None,
                duration_ms: start.elapsed().as_millis() as u64,
            }))
        }
        Err(err) => Ok(Some(ExecuteOutput {
            success: false,
            summary: format!(
                "controller pubpunk validate recipe applied but verification failed: {err}"
            ),
            checks_run: Vec::new(),
            cost_usd: None,
            duration_ms: start.elapsed().as_millis() as u64,
        })),
    }
}

fn apply_controller_pubpunk_init_recipe(
    repo_root: &Path,
    contract: &Contract,
) -> Result<Option<Vec<String>>> {
    let Some(files) = controller_pubpunk_init_templates(repo_root, contract) else {
        return Ok(None);
    };

    let mut updated_paths = Vec::new();
    for (path, contents) in files {
        if !path_is_in_allowed_scope(&path, &contract.allowed_scope) {
            continue;
        }
        let file_path = repo_root.join(&path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create controller pubpunk init parent {}", path))?;
        }
        let current = fs::read_to_string(&file_path).ok();
        if current.as_deref() == Some(contents.as_str()) {
            continue;
        }
        fs::write(&file_path, contents)
            .with_context(|| format!("write controller pubpunk init file {}", path))?;
        updated_paths.push(path);
    }

    Ok(Some(updated_paths))
}

fn apply_controller_pubpunk_cleanup_recipe(
    repo_root: &Path,
    contract: &Contract,
) -> Result<Option<Vec<String>>> {
    let Some(files) = controller_pubpunk_cleanup_templates(repo_root, contract) else {
        return Ok(None);
    };

    let mut updated_paths = Vec::new();
    for (path, contents) in files {
        if !path_is_in_allowed_scope(&path, &contract.allowed_scope) {
            continue;
        }
        let file_path = repo_root.join(&path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create controller pubpunk cleanup parent {}", path))?;
        }
        let current = fs::read_to_string(&file_path).ok();
        if current.as_deref() == Some(contents.as_str()) {
            continue;
        }
        fs::write(&file_path, contents)
            .with_context(|| format!("write controller pubpunk cleanup file {}", path))?;
        updated_paths.push(path);
    }

    Ok(Some(updated_paths))
}

fn apply_controller_pubpunk_validate_parseability_recipe(
    repo_root: &Path,
    contract: &Contract,
) -> Result<Option<Vec<String>>> {
    let Some(files) = controller_pubpunk_validate_parseability_templates(repo_root, contract)
    else {
        return Ok(None);
    };

    let mut updated_paths = Vec::new();
    for (path, contents) in files {
        if !path_is_in_allowed_scope(&path, &contract.allowed_scope) {
            continue;
        }
        let file_path = repo_root.join(&path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "create controller pubpunk validate parseability parent {}",
                    path
                )
            })?;
        }
        let current = fs::read_to_string(&file_path).ok();
        if current.as_deref() == Some(contents.as_str()) {
            continue;
        }
        fs::write(&file_path, contents).with_context(|| {
            format!(
                "write controller pubpunk validate parseability file {}",
                path
            )
        })?;
        updated_paths.push(path);
    }

    Ok(Some(updated_paths))
}

fn apply_controller_pubpunk_validate_file_parseability_recipe(
    repo_root: &Path,
    contract: &Contract,
) -> Result<Option<Vec<String>>> {
    let Some(files) = controller_pubpunk_validate_file_parseability_templates(repo_root, contract)
    else {
        return Ok(None);
    };

    let mut updated_paths = Vec::new();
    for (path, contents) in files {
        if !path_is_in_allowed_scope(&path, &contract.allowed_scope) {
            continue;
        }
        let file_path = repo_root.join(&path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "create controller pubpunk validate file parseability parent {}",
                    path
                )
            })?;
        }
        let current = fs::read_to_string(&file_path).ok();
        if current.as_deref() == Some(contents.as_str()) {
            continue;
        }
        fs::write(&file_path, contents).with_context(|| {
            format!(
                "write controller pubpunk validate file parseability file {}",
                path
            )
        })?;
        updated_paths.push(path);
    }

    Ok(Some(updated_paths))
}

fn apply_controller_pubpunk_validate_recipe(
    repo_root: &Path,
    contract: &Contract,
) -> Result<Option<Vec<String>>> {
    let Some(files) = controller_pubpunk_validate_templates(repo_root, contract) else {
        return Ok(None);
    };

    let mut updated_paths = Vec::new();
    for (path, contents) in files {
        if !path_is_in_allowed_scope(&path, &contract.allowed_scope) {
            continue;
        }
        let file_path = repo_root.join(&path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create controller pubpunk validate parent {}", path))?;
        }
        let current = fs::read_to_string(&file_path).ok();
        if current.as_deref() == Some(contents.as_str()) {
            continue;
        }
        fs::write(&file_path, contents)
            .with_context(|| format!("write controller pubpunk validate file {}", path))?;
        updated_paths.push(path);
    }

    Ok(Some(updated_paths))
}

fn controller_pubpunk_init_templates(
    repo_root: &Path,
    contract: &Contract,
) -> Option<Vec<(String, String)>> {
    if !is_controller_pubpunk_init_recipe(contract) {
        return None;
    }
    if !repo_root.join("Cargo.toml").exists()
        || !repo_root.join("crates/pubpunk-cli/Cargo.toml").exists()
        || !repo_root.join("crates/pubpunk-core/Cargo.toml").exists()
    {
        return None;
    }

    Some(vec![
        (
            "crates/pubpunk-core/src/lib.rs".to_string(),
            render_pubpunk_init_core_source(),
        ),
        (
            "crates/pubpunk-cli/src/main.rs".to_string(),
            render_pubpunk_init_cli_source(),
        ),
        (
            "tests/init_json.rs".to_string(),
            render_pubpunk_init_tests_source(),
        ),
    ])
}

fn controller_pubpunk_cleanup_templates(
    repo_root: &Path,
    contract: &Contract,
) -> Option<Vec<(String, String)>> {
    if !is_controller_pubpunk_cleanup_recipe(contract) {
        return None;
    }
    if !repo_root.join("crates/pubpunk-core/src/lib.rs").exists()
        || !repo_root.join("tests/init_json.rs").exists()
    {
        return None;
    }

    Some(vec![
        (
            "crates/pubpunk-core/src/lib.rs".to_string(),
            render_pubpunk_cleanup_core_source(),
        ),
        (
            "tests/init_json.rs".to_string(),
            render_pubpunk_cleanup_tests_source(),
        ),
    ])
}

fn controller_pubpunk_validate_templates(
    repo_root: &Path,
    contract: &Contract,
) -> Option<Vec<(String, String)>> {
    if !is_controller_pubpunk_validate_recipe(contract) {
        return None;
    }
    if !repo_root.join("Cargo.toml").exists()
        || !repo_root.join("crates/pubpunk-cli/Cargo.toml").exists()
        || !repo_root.join("crates/pubpunk-core/Cargo.toml").exists()
        || !repo_root.join("crates/pubpunk-core/src/lib.rs").exists()
        || !repo_root.join("crates/pubpunk-cli/src/main.rs").exists()
    {
        return None;
    }

    Some(vec![
        (
            "crates/pubpunk-core/src/lib.rs".to_string(),
            render_pubpunk_validate_core_source(),
        ),
        (
            "crates/pubpunk-cli/src/main.rs".to_string(),
            render_pubpunk_validate_cli_source(),
        ),
        (
            "tests/validate_json.rs".to_string(),
            render_pubpunk_validate_tests_source(),
        ),
    ])
}

fn controller_pubpunk_validate_parseability_templates(
    repo_root: &Path,
    contract: &Contract,
) -> Option<Vec<(String, String)>> {
    if !is_controller_pubpunk_validate_parseability_recipe(contract) {
        return None;
    }
    if !repo_root.join("crates/pubpunk-core/src/lib.rs").exists() {
        return None;
    }

    Some(vec![(
        "crates/pubpunk-core/src/lib.rs".to_string(),
        render_pubpunk_validate_parseability_core_source(),
    )])
}

fn controller_pubpunk_validate_file_parseability_templates(
    repo_root: &Path,
    contract: &Contract,
) -> Option<Vec<(String, String)>> {
    if !is_controller_pubpunk_validate_file_parseability_recipe(contract) {
        return None;
    }
    if !repo_root.join("crates/pubpunk-core/src/lib.rs").exists()
        || !repo_root.join("tests/validate_json.rs").exists()
    {
        return None;
    }

    Some(vec![
        (
            "crates/pubpunk-core/src/lib.rs".to_string(),
            render_pubpunk_validate_file_parseability_core_source(),
        ),
        (
            "tests/validate_json.rs".to_string(),
            render_pubpunk_validate_file_parseability_tests_source(),
        ),
    ])
}

fn is_controller_pubpunk_cleanup_recipe(contract: &Contract) -> bool {
    let has_core = contract
        .entry_points
        .iter()
        .any(|path| path == "crates/pubpunk-core/src/lib.rs")
        || path_is_in_allowed_scope("crates/pubpunk-core/src/lib.rs", &contract.allowed_scope);
    let has_tests = contract
        .entry_points
        .iter()
        .any(|path| path == "tests/init_json.rs")
        || contract
            .allowed_scope
            .iter()
            .any(|path| path == "tests" || path.starts_with("tests/"));
    if !(has_core && has_tests) {
        return false;
    }

    let combined = pubpunk_init_contract_text(contract);
    (combined.contains("style/examples") || combined.contains("style examples"))
        && (combined.contains("remove") || combined.contains("cleanup"))
}

fn is_controller_pubpunk_validate_parseability_recipe(contract: &Contract) -> bool {
    let core_only = contract.entry_points == ["crates/pubpunk-core/src/lib.rs".to_string()]
        && contract.allowed_scope == ["crates/pubpunk-core/src/lib.rs".to_string()];
    if !core_only {
        return false;
    }

    let combined = pubpunk_init_contract_text(contract);
    combined.contains("core-only validate parseability helper slice")
        && combined.contains("validate_report")
        && combined.contains("json envelope unchanged")
        && combined.contains("style/targets/review/lint")
        && combined.contains("do not touch cli")
        && combined.contains("cargo.toml")
        && combined.contains("init files")
}

fn is_controller_pubpunk_validate_file_parseability_recipe(contract: &Contract) -> bool {
    let exact_entry_points = contract.entry_points
        == vec![
            "crates/pubpunk-core/src/lib.rs".to_string(),
            "tests/validate_json.rs".to_string(),
        ];
    let exact_scope = contract.allowed_scope
        == vec![
            "crates/pubpunk-core/src/lib.rs".to_string(),
            "tests/validate_json.rs".to_string(),
        ];
    if !(exact_entry_points && exact_scope) {
        return false;
    }

    let combined = pubpunk_init_contract_text(contract);
    combined.contains("validate parse-check extension")
        && combined.contains("tests/validate_json.rs")
        && combined.contains(".pubpunk/style/style.toml")
        && combined.contains(".pubpunk/targets")
        && combined.contains("target .toml file")
        && combined.contains("do not touch cli")
        && combined.contains("do not touch cli or cargo")
}

fn is_controller_pubpunk_validate_recipe(contract: &Contract) -> bool {
    let has_cli = contract
        .entry_points
        .iter()
        .any(|path| path == "crates/pubpunk-cli/src/main.rs")
        || path_is_in_allowed_scope("crates/pubpunk-cli/src/main.rs", &contract.allowed_scope);
    let has_core = contract
        .entry_points
        .iter()
        .any(|path| path == "crates/pubpunk-core/src/lib.rs")
        || path_is_in_allowed_scope("crates/pubpunk-core/src/lib.rs", &contract.allowed_scope);
    let has_tests = contract
        .entry_points
        .iter()
        .any(|path| path == "tests/validate_json.rs")
        || path_is_in_allowed_scope("tests/validate_json.rs", &contract.allowed_scope)
        || contract
            .allowed_scope
            .iter()
            .any(|path| path == "tests" || path.starts_with("tests/"));
    if !(has_cli && has_core && has_tests) {
        return false;
    }

    let combined = pubpunk_init_contract_text(contract);
    contract_looks_like_stable_pubpunk_recipe_scope(contract)
        && combined.contains("pubpunk validate")
        && combined.contains("validate-only")
        && combined.contains("structured json envelope")
        && (combined.contains("do not add init behavior") || combined.contains("no init work"))
        && (combined.contains("project-root")
            || combined.contains("project_root")
            || combined.contains("project root"))
}

fn is_controller_pubpunk_init_recipe(contract: &Contract) -> bool {
    let has_cli = contract
        .entry_points
        .iter()
        .any(|path| path == "crates/pubpunk-cli/src/main.rs")
        || path_is_in_allowed_scope("crates/pubpunk-cli/src/main.rs", &contract.allowed_scope);
    let has_core = contract
        .entry_points
        .iter()
        .any(|path| path == "crates/pubpunk-core/src/lib.rs")
        || path_is_in_allowed_scope("crates/pubpunk-core/src/lib.rs", &contract.allowed_scope);
    let has_tests = contract
        .allowed_scope
        .iter()
        .any(|path| path == "tests" || path.starts_with("tests/"));
    if !(has_cli && has_core && has_tests) {
        return false;
    }

    let combined = pubpunk_init_contract_text(contract);
    contract_looks_like_stable_pubpunk_recipe_scope(contract)
        && combined.contains("pubpunk init")
        && combined.contains("json")
        && (combined.contains("canonical .pubpunk skeleton")
            || combined.contains("canonical .pubpunk tree")
            || combined.contains("canonical starter files"))
        && (combined.contains("create")
            || combined.contains("creates")
            || combined.contains("materialize"))
}

fn pubpunk_init_contract_text(contract: &Contract) -> String {
    let mut combined = contract.prompt_source.to_ascii_lowercase();
    for item in contract
        .expected_interfaces
        .iter()
        .chain(contract.behavior_requirements.iter())
    {
        combined.push('\n');
        combined.push_str(&item.to_ascii_lowercase());
    }
    combined
}

fn contract_looks_like_stable_pubpunk_recipe_scope(contract: &Contract) -> bool {
    let allowed_prefixes = [
        "Cargo.toml",
        "crates",
        "crates/pubpunk-cli",
        "crates/pubpunk-core",
        "tests",
        "local/",
    ];
    contract
        .entry_points
        .iter()
        .chain(contract.allowed_scope.iter())
        .all(|path| {
            allowed_prefixes.iter().any(|prefix| {
                path == prefix
                    || path.starts_with(&format!("{prefix}/"))
                    || (prefix.ends_with('/') && path.starts_with(prefix))
            })
        })
}

fn render_pubpunk_cleanup_core_source() -> String {
    render_pubpunk_init_core_source()
        .replace("    \".pubpunk/style/examples\",\n", "")
        .replace(
            "        assert!(root.join(\".pubpunk/style/examples\").is_dir());\n",
            "",
        )
}

fn render_pubpunk_cleanup_tests_source() -> String {
    render_pubpunk_init_tests_source().replace("        \".pubpunk/style/examples\",\n", "")
}

fn render_pubpunk_validate_core_source() -> String {
    let source = render_pubpunk_cleanup_core_source();
    let source = source.replace(
        "use std::fs;\nuse std::io;\nuse std::path::{Path, PathBuf};\n",
        "use std::collections::BTreeMap;\nuse std::fs;\nuse std::io;\nuse std::path::{Path, PathBuf};\n",
    );
    let source = source.replace(
        "pub fn validate(root: &Path) -> io::Result<()> {\n    let root = prepare_existing_root(root)?;\n    for relative in SKELETON_DIRS {\n        let path = root.join(relative);\n        if !path.is_dir() {\n            return Err(io::Error::new(\n                io::ErrorKind::NotFound,\n                format!(\"missing directory: {}\", path.display()),\n            ));\n        }\n    }\n    for (relative, _) in SKELETON_FILES {\n        let path = root.join(relative);\n        if !path.is_file() {\n            return Err(io::Error::new(\n                io::ErrorKind::NotFound,\n                format!(\"missing file: {}\", path.display()),\n            ));\n        }\n    }\n\n    let project_toml = fs::read_to_string(root.join(\".pubpunk/project.toml\"))?;\n    validate_project_toml(&project_toml)?;\n    Ok(())\n}\n\nfn validate_project_toml(contents: &str) -> io::Result<()> {\n    for required in REQUIRED_PROJECT_LINES {\n        if !contents.contains(required) {\n            return Err(io::Error::new(\n                io::ErrorKind::InvalidData,\n                format!(\"missing required project.toml line: {required}\"),\n            ));\n        }\n    }\n\n    let project_id = assignment_value(contents, \"project_id\").ok_or_else(|| {\n        io::Error::new(io::ErrorKind::InvalidData, \"missing required project_id\")\n    })?;\n    if !is_slug_safe(project_id) {\n        return Err(io::Error::new(\n            io::ErrorKind::InvalidData,\n            format!(\"project_id is not slug-safe: {project_id}\"),\n        ));\n    }\n\n    for key in PATH_KEYS {\n        let value = assignment_value(contents, key).ok_or_else(|| {\n            io::Error::new(io::ErrorKind::InvalidData, format!(\"missing required key: {key}\"))\n        })?;\n        if Path::new(value).is_absolute() {\n            return Err(io::Error::new(\n                io::ErrorKind::InvalidData,\n                format!(\"{key} must stay relative under .pubpunk: {value}\"),\n            ));\n        }\n    }\n\n    for key in LOCAL_KEYS {\n        let value = assignment_value(contents, key).ok_or_else(|| {\n            io::Error::new(io::ErrorKind::InvalidData, format!(\"missing required key: {key}\"))\n        })?;\n        if !value.starts_with(\"local/\") || Path::new(value).is_absolute() {\n            return Err(io::Error::new(\n                io::ErrorKind::InvalidData,\n                format!(\"{key} must stay under local/: {value}\"),\n            ));\n        }\n    }\n\n    let lowered = contents.to_ascii_lowercase();\n    for secret_like in [\"token\", \"secret\", \"password\", \"api_key\"] {\n        if lowered.contains(secret_like) {\n            return Err(io::Error::new(\n                io::ErrorKind::InvalidData,\n                format!(\"project.toml contains obvious secret-like field: {secret_like}\"),\n            ));\n        }\n    }\n\n    Ok(())\n}\n\nfn assignment_value<'a>(contents: &'a str, key: &str) -> Option<&'a str> {\n    for line in contents.lines() {\n        let trimmed = line.trim();\n        let prefix = format!(\"{key} = \");\n        let Some(value) = trimmed.strip_prefix(&prefix) else {\n            continue;\n        };\n        let value = value.trim();\n        if value.starts_with('\"') && value.ends_with('\"') && value.len() >= 2 {\n            return Some(&value[1..value.len() - 1]);\n        }\n    }\n    None\n}\n",
        "#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateIssue {\n    pub code: &'static str,\n    pub path: Option<PathBuf>,\n    pub message: String,\n}\n\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct ValidateReport {\n    pub root: PathBuf,\n    pub issues: Vec<ValidateIssue>,\n}\n\nimpl ValidateReport {\n    pub fn ok(&self) -> bool {\n        self.issues.is_empty()\n    }\n\n    pub fn to_json(&self) -> String {\n        let root = escape_json_string(&self.root.display().to_string());\n        let issues = self\n            .issues\n            .iter()\n            .map(|issue| {\n                let code = escape_json_string(issue.code);\n                let message = escape_json_string(&issue.message);\n                let path = issue\n                    .path\n                    .as_ref()\n                    .map(|path| format!(\"\\\"{}\\\"\", escape_json_string(&path.display().to_string())))\n                    .unwrap_or_else(|| \"null\".to_string());\n                format!(\n                    \"{{\\\"code\\\":\\\"{code}\\\",\\\"path\\\":{path},\\\"message\\\":\\\"{message}\\\"}}\"\n                )\n            })\n            .collect::<Vec<_>>()\n            .join(\",\");\n        format!(\n            \"{{\\\"ok\\\":{},\\\"root\\\":\\\"{root}\\\",\\\"issues\\\":[{issues}]}}\",\n            if self.ok() { \"true\" } else { \"false\" }\n        )\n    }\n\n    pub fn summary_message(&self) -> String {\n        self.issues\n            .first()\n            .map(|issue| issue.message.clone())\n            .unwrap_or_else(|| \"Validated .pubpunk skeleton\".to_string())\n    }\n\n    pub fn into_result(self) -> io::Result<()> {\n        if self.ok() {\n            Ok(())\n        } else {\n            Err(io::Error::new(io::ErrorKind::InvalidData, self.summary_message()))\n        }\n    }\n}\n\npub fn validate(root: &Path) -> io::Result<()> {\n    validate_report(root).into_result()\n}\n\npub fn validate_report(root: &Path) -> ValidateReport {\n    let root = match prepare_existing_root(root) {\n        Ok(root) => root,\n        Err(err) => {\n            return ValidateReport {\n                root: root.to_path_buf(),\n                issues: vec![ValidateIssue {\n                    code: \"root_missing\",\n                    path: None,\n                    message: err.to_string(),\n                }],\n            };\n        }\n    };\n\n    let mut issues = Vec::new();\n    for relative in SKELETON_DIRS {\n        let path = root.join(relative);\n        if !path.is_dir() {\n            issues.push(ValidateIssue {\n                code: \"missing_directory\",\n                path: Some(PathBuf::from(relative)),\n                message: format!(\"missing directory: {}\", path.display()),\n            });\n        }\n    }\n    for (relative, _) in SKELETON_FILES {\n        let path = root.join(relative);\n        if !path.is_file() {\n            issues.push(ValidateIssue {\n                code: \"missing_file\",\n                path: Some(PathBuf::from(relative)),\n                message: format!(\"missing file: {}\", path.display()),\n            });\n        }\n    }\n\n    let project_relative = PathBuf::from(\".pubpunk/project.toml\");\n    let project_path = root.join(&project_relative);\n    if project_path.is_file() {\n        match fs::read_to_string(&project_path) {\n            Ok(contents) => issues.extend(validate_project_toml(&contents)),\n            Err(err) => issues.push(ValidateIssue {\n                code: \"unreadable_project_toml\",\n                path: Some(project_relative),\n                message: format!(\"unable to read {}: {err}\", project_path.display()),\n            }),\n        }\n    }\n\n    ValidateReport { root, issues }\n}\n\nfn validate_project_toml(contents: &str) -> Vec<ValidateIssue> {\n    let mut issues = Vec::new();\n    let parsed = match parse_project_toml(contents) {\n        Ok(parsed) => parsed,\n        Err(message) => {\n            issues.push(ValidateIssue {\n                code: \"unparseable_project_toml\",\n                path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n                message,\n            });\n            return issues;\n        }\n    };\n\n    for required in REQUIRED_PROJECT_LINES {\n        if !contents.contains(required) {\n            issues.push(ValidateIssue {\n                code: \"missing_required_line\",\n                path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n                message: format!(\"missing required project.toml line: {required}\"),\n            });\n        }\n    }\n\n    match parsed.get(\"project_id\") {\n        Some(project_id) if is_slug_safe(project_id) => {}\n        Some(project_id) => issues.push(ValidateIssue {\n            code: \"invalid_project_id\",\n            path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n            message: format!(\"project_id is not slug-safe: {project_id}\"),\n        }),\n        None => issues.push(ValidateIssue {\n            code: \"missing_project_id\",\n            path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n            message: \"missing required project_id\".to_string(),\n        }),\n    }\n\n    match parsed.get(\"schema_version\") {\n        Some(value) if value == \"pubpunk.project.v1\" => {}\n        Some(value) => issues.push(ValidateIssue {\n            code: \"invalid_schema_version\",\n            path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n            message: format!(\"schema_version must be pubpunk.project.v1, got: {value}\"),\n        }),\n        None => issues.push(ValidateIssue {\n            code: \"missing_schema_version\",\n            path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n            message: \"missing required schema_version\".to_string(),\n        }),\n    }\n\n    for key in PATH_KEYS {\n        let full_key = format!(\"paths.{key}\");\n        match parsed.get(&full_key) {\n            Some(value) if !Path::new(value).is_absolute() => {}\n            Some(value) => issues.push(ValidateIssue {\n                code: \"absolute_path\",\n                path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n                message: format!(\"{key} must stay relative under .pubpunk: {value}\"),\n            }),\n            None => issues.push(ValidateIssue {\n                code: \"missing_path_key\",\n                path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n                message: format!(\"missing required key: {key}\"),\n            }),\n        }\n    }\n\n    for key in LOCAL_KEYS {\n        let full_key = format!(\"local.{key}\");\n        match parsed.get(&full_key) {\n            Some(value) if value.starts_with(\"local/\") && !Path::new(value).is_absolute() => {}\n            Some(value) => issues.push(ValidateIssue {\n                code: \"invalid_local_path\",\n                path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n                message: format!(\"{key} must stay under local/: {value}\"),\n            }),\n            None => issues.push(ValidateIssue {\n                code: \"missing_local_key\",\n                path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n                message: format!(\"missing required key: {key}\"),\n            }),\n        }\n    }\n\n    for key in parsed.keys() {\n        let lowered = key.to_ascii_lowercase();\n        let leaf = lowered.rsplit('.').next().unwrap_or(lowered.as_str());\n        if [\"token\", \"secret\", \"password\", \"api_key\"]\n            .iter()\n            .any(|needle| leaf.contains(needle) || lowered.contains(needle))\n        {\n            issues.push(ValidateIssue {\n                code: \"secret_like_key\",\n                path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n                message: format!(\"project.toml contains obvious secret-like field: {key}\"),\n            });\n        }\n    }\n\n    issues\n}\n\nfn parse_project_toml(contents: &str) -> Result<BTreeMap<String, String>, String> {\n    let mut values = BTreeMap::new();\n    let mut current_section: Option<String> = None;\n\n    for (index, raw_line) in contents.lines().enumerate() {\n        let trimmed = raw_line.trim();\n        if trimmed.is_empty() || trimmed.starts_with('#') {\n            continue;\n        }\n        if trimmed.starts_with('[') {\n            if !trimmed.ends_with(']') || trimmed.len() < 3 {\n                return Err(format!(\"project.toml is not parseable near line {}\", index + 1));\n            }\n            current_section = Some(trimmed[1..trimmed.len() - 1].trim().to_string());\n            continue;\n        }\n        let Some((key, value)) = trimmed.split_once('=') else {\n            return Err(format!(\"project.toml is not parseable near line {}\", index + 1));\n        };\n        let key = key.trim();\n        if key.is_empty() {\n            return Err(format!(\"project.toml is not parseable near line {}\", index + 1));\n        }\n        let value = parse_project_toml_value(value.trim())\n            .map_err(|_| format!(\"project.toml is not parseable near line {}\", index + 1))?;\n        let full_key = current_section\n            .as_ref()\n            .map(|section| format!(\"{section}.{key}\"))\n            .unwrap_or_else(|| key.to_string());\n        values.insert(full_key, value);\n    }\n\n    Ok(values)\n}\n\nfn parse_project_toml_value(value: &str) -> Result<String, ()> {\n    if value.len() >= 2\n        && ((value.starts_with('\"') && value.ends_with('\"'))\n            || (value.starts_with('\\'') && value.ends_with('\\'')))\n    {\n        return Ok(value[1..value.len() - 1].to_string());\n    }\n    Err(())\n}\n",
    );
    source
}

fn render_pubpunk_validate_parseability_core_source() -> String {
    let source = render_pubpunk_validate_core_source();
    let source = source.replace(
        "const LOCAL_KEYS: &[&str] = &[\n    \"state_db\",\n    \"drafts_dir\",\n    \"reports_dir\",\n    \"cache_dir\",\n    \"generated_dir\",\n];\n",
        "const LOCAL_KEYS: &[&str] = &[\n    \"state_db\",\n    \"drafts_dir\",\n    \"reports_dir\",\n    \"cache_dir\",\n    \"generated_dir\",\n];\n\nconst RESERVED_SURFACE_PREFIXES: &[(&str, &str)] = &[\n    (\"style.\", \"style\"),\n    (\"targets.\", \"targets\"),\n    (\"review.\", \"review\"),\n    (\"lint.\", \"lint\"),\n];\n",
    );
    let source = source.replace(
        "    for key in parsed.keys() {\n        let lowered = key.to_ascii_lowercase();\n        let leaf = lowered.rsplit('.').next().unwrap_or(lowered.as_str());\n        if [\"token\", \"secret\", \"password\", \"api_key\"]\n            .iter()\n            .any(|needle| leaf.contains(needle) || lowered.contains(needle))\n        {\n            issues.push(ValidateIssue {\n                code: \"secret_like_key\",\n                path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n                message: format!(\"project.toml contains obvious secret-like field: {key}\"),\n            });\n        }\n    }\n\n    issues\n}\n\nfn parse_project_toml(contents: &str) -> Result<BTreeMap<String, String>, String> {\n",
        "    for key in parsed.keys() {\n        let lowered = key.to_ascii_lowercase();\n        let leaf = lowered.rsplit('.').next().unwrap_or(lowered.as_str());\n        if [\"token\", \"secret\", \"password\", \"api_key\"]\n            .iter()\n            .any(|needle| leaf.contains(needle) || lowered.contains(needle))\n        {\n            issues.push(ValidateIssue {\n                code: \"secret_like_key\",\n                path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n                message: format!(\"project.toml contains obvious secret-like field: {key}\"),\n            });\n        }\n    }\n\n    issues.extend(validate_reserved_surface_inputs(&parsed));\n\n    issues\n}\n\nfn validate_reserved_surface_inputs(parsed: &BTreeMap<String, String>) -> Vec<ValidateIssue> {\n    let mut issues = Vec::new();\n    for key in parsed.keys() {\n        for (prefix, section) in RESERVED_SURFACE_PREFIXES {\n            if key.starts_with(prefix) {\n                issues.push(ValidateIssue {\n                    code: \"unsupported_surface_input\",\n                    path: Some(PathBuf::from(\".pubpunk/project.toml\")),\n                    message: format!(\n                        \"project.toml does not support [{section}] entries yet: {key}\"\n                    ),\n                });\n                break;\n            }\n        }\n    }\n    issues\n}\n\nfn parse_project_toml(contents: &str) -> Result<BTreeMap<String, String>, String> {\n",
    );
    source.replace(
        "    #[test]\n    fn json_is_machine_readable() {\n",
        "    #[test]\n    fn validate_report_flags_unsupported_style_targets_review_and_lint_inputs() {\n        let root = temp_dir(\"core-validate-surface-inputs\");\n        init_with_options(&root, false).unwrap();\n        let project_toml = root.join(\".pubpunk/project.toml\");\n        let current = fs::read_to_string(&project_toml).unwrap();\n        fs::write(\n            &project_toml,\n            format!(\n                \"{current}\\n[style]\\nvoice = \\\"strict\\\"\\n[targets]\\nprimary = \\\"forem\\\"\\n[review]\\nmode = \\\"required\\\"\\n[lint]\\nprofile = \\\"standard\\\"\\n\"\n            ),\n        )\n        .unwrap();\n\n        let report = validate_report(&root);\n        let messages = report\n            .issues\n            .iter()\n            .map(|issue| issue.message.as_str())\n            .collect::<Vec<_>>();\n        assert!(messages\n            .iter()\n            .any(|message| message.contains(\"[style]\") && message.contains(\"style.voice\")));\n        assert!(messages\n            .iter()\n            .any(|message| message.contains(\"[targets]\") && message.contains(\"targets.primary\")));\n        assert!(messages\n            .iter()\n            .any(|message| message.contains(\"[review]\") && message.contains(\"review.mode\")));\n        assert!(messages\n            .iter()\n            .any(|message| message.contains(\"[lint]\") && message.contains(\"lint.profile\")));\n        fs::remove_dir_all(&root).unwrap();\n    }\n\n    #[test]\n    fn json_is_machine_readable() {\n",
    )
}

fn render_pubpunk_validate_file_parseability_core_source() -> String {
    let source = render_pubpunk_validate_parseability_core_source();
    let source = source.replace(
        "    ValidateReport { root, issues }\n}\n\nfn validate_project_toml(contents: &str) -> Vec<ValidateIssue> {\n",
        "    extend_toml_surface_issues(&root, &mut issues);\n\n    ValidateReport { root, issues }\n}\n\nfn validate_project_toml(contents: &str) -> Vec<ValidateIssue> {\n",
    );
    source.replace(
        "fn parse_project_toml(contents: &str) -> Result<BTreeMap<String, String>, String> {\n",
        "fn extend_toml_surface_issues(root: &Path, issues: &mut Vec<ValidateIssue>) {\n    for relative in [\n        \".pubpunk/style/style.toml\",\n        \".pubpunk/style/lexicon.toml\",\n        \".pubpunk/style/normalize.toml\",\n    ] {\n        validate_toml_surface_file(root, relative, \"unreadable_style_toml\", \"unparseable_style_toml\", issues);\n    }\n\n    validate_toml_surface_dir(root, \".pubpunk/targets\", \"unreadable_target_toml\", \"unparseable_target_toml\", issues);\n    validate_toml_surface_dir(root, \".pubpunk/review\", \"unreadable_review_toml\", \"unparseable_review_toml\", issues);\n    validate_toml_surface_dir(root, \".pubpunk/lint\", \"unreadable_lint_toml\", \"unparseable_lint_toml\", issues);\n}\n\nfn validate_toml_surface_dir(\n    root: &Path,\n    relative_dir: &str,\n    unreadable_code: &'static str,\n    unparseable_code: &'static str,\n    issues: &mut Vec<ValidateIssue>,\n) {\n    let dir = root.join(relative_dir);\n    let Ok(entries) = fs::read_dir(&dir) else {\n        return;\n    };\n    let mut toml_paths = entries\n        .filter_map(|entry| entry.ok())\n        .filter_map(|entry| {\n            let file_type = entry.file_type().ok()?;\n            if !file_type.is_file() {\n                return None;\n            }\n            let file_name = entry.file_name();\n            let file_name = file_name.to_str()?;\n            if !file_name.ends_with(\".toml\") {\n                return None;\n            }\n            Some(format!(\"{relative_dir}/{file_name}\"))\n        })\n        .collect::<Vec<_>>();\n    toml_paths.sort();\n    for relative in toml_paths {\n        validate_toml_surface_file(root, &relative, unreadable_code, unparseable_code, issues);\n    }\n}\n\nfn validate_toml_surface_file(\n    root: &Path,\n    relative: &str,\n    unreadable_code: &'static str,\n    unparseable_code: &'static str,\n    issues: &mut Vec<ValidateIssue>,\n) {\n    let path = root.join(relative);\n    let contents = match fs::read_to_string(&path) {\n        Ok(contents) => contents,\n        Err(err) => {\n            issues.push(ValidateIssue {\n                code: unreadable_code,\n                path: Some(PathBuf::from(relative)),\n                message: format!(\"unable to read {}: {err}\", path.display()),\n            });\n            return;\n        }\n    };\n    if let Err(message) = parse_project_toml(&contents) {\n        issues.push(ValidateIssue {\n            code: unparseable_code,\n            path: Some(PathBuf::from(relative)),\n            message: format!(\"{} is not parseable TOML: {message}\", relative),\n        });\n    }\n}\n\nfn parse_project_toml(contents: &str) -> Result<BTreeMap<String, String>, String> {\n",
    )
}

fn render_pubpunk_validate_file_parseability_tests_source() -> String {
    let source = render_pubpunk_validate_tests_source();
    source.replace(
        "\nfn temp_dir(prefix: &str) -> PathBuf {\n",
        "\n#[test]\nfn validate_json_rejects_unparseable_style_toml() {\n    let root = temp_dir(\"pubpunk-validate-json-style-toml\");\n    pubpunk_core::init_with_options(&root, false).unwrap();\n    fs::write(root.join(\".pubpunk/style/style.toml\"), \"[style\\nbroken\").unwrap();\n\n    let output = cli::run(\n        [\n            \"validate\".to_string(),\n            \"--json\".to_string(),\n            \"--project-root\".to_string(),\n            root.display().to_string(),\n        ],\n        Path::new(\".\"),\n    )\n    .expect_err(\"validate json should fail\");\n\n    assert!(\n        output.text.contains(\"\\\"code\\\":\\\"unparseable_style_toml\\\"\"),\n        \"json={}\",\n        output.text\n    );\n    assert!(output.text.contains(\".pubpunk/style/style.toml\"), \"json={}\", output.text);\n    fs::remove_dir_all(&root).unwrap();\n}\n\n#[test]\nfn validate_json_rejects_unparseable_target_toml() {\n    let root = temp_dir(\"pubpunk-validate-json-target-toml\");\n    pubpunk_core::init_with_options(&root, false).unwrap();\n    fs::write(root.join(\".pubpunk/targets/demo.toml\"), \"[target\\nbroken\").unwrap();\n\n    let output = cli::run(\n        [\n            \"validate\".to_string(),\n            \"--json\".to_string(),\n            \"--project-root\".to_string(),\n            root.display().to_string(),\n        ],\n        Path::new(\".\"),\n    )\n    .expect_err(\"validate json should fail\");\n\n    assert!(\n        output.text.contains(\"\\\"code\\\":\\\"unparseable_target_toml\\\"\"),\n        \"json={}\",\n        output.text\n    );\n    assert!(output.text.contains(\".pubpunk/targets/demo.toml\"), \"json={}\", output.text);\n    fs::remove_dir_all(&root).unwrap();\n}\n\nfn temp_dir(prefix: &str) -> PathBuf {\n",
    )
}

fn render_pubpunk_validate_cli_source() -> String {
    r##"use std::env;
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};

fn main() -> ExitCode {
    match run(env::args().skip(1), Path::new(".")) {
        Ok(output) => {
            if !output.text.is_empty() {
                println!("{}", output.text);
            }
            ExitCode::SUCCESS
        }
        Err(output) => {
            if !output.text.is_empty() {
                if output.prefer_stdout {
                    println!("{}", output.text);
                } else {
                    eprintln!("{}", output.text);
                }
            }
            process::ExitCode::from(1)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    pub text: String,
    pub prefer_stdout: bool,
}

impl RunOutput {
    fn stdout(text: String) -> Self {
        Self {
            text,
            prefer_stdout: true,
        }
    }

    fn stderr(text: String) -> Self {
        Self {
            text,
            prefer_stdout: false,
        }
    }
}

pub fn run<I>(args: I, cwd: &Path) -> Result<RunOutput, RunOutput>
where
    I: IntoIterator<Item = String>,
{
    let command = parse_args(args).map_err(RunOutput::stderr)?;
    match command {
        Command::Init {
            json,
            force,
            project_root,
        } => {
            let root = project_root.unwrap_or_else(|| cwd.to_path_buf());
            let result = pubpunk_core::init_with_options(&root, force)
                .map_err(|err| RunOutput::stderr(format!("pubpunk init failed: {err}")))?;
            if json {
                Ok(RunOutput::stdout(result.to_json()))
            } else if result.created.is_empty() && result.rewritten.is_empty() {
                Ok(RunOutput::stdout(
                    "Initialized .pubpunk skeleton (no changes)".to_string(),
                ))
            } else {
                Ok(RunOutput::stdout("Initialized .pubpunk skeleton".to_string()))
            }
        }
        Command::Validate { json, project_root } => {
            let root = project_root.unwrap_or_else(|| cwd.to_path_buf());
            let report = pubpunk_core::validate_report(&root);
            if json {
                if report.ok() {
                    Ok(RunOutput::stdout(report.to_json()))
                } else {
                    Err(RunOutput::stdout(report.to_json()))
                }
            } else {
                report
                    .clone()
                    .into_result()
                    .map_err(|err| RunOutput::stderr(format!("pubpunk validate failed: {err}")))?;
                Ok(RunOutput::stdout("Validated .pubpunk skeleton".to_string()))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Init {
        json: bool,
        force: bool,
        project_root: Option<PathBuf>,
    },
    Validate {
        json: bool,
        project_root: Option<PathBuf>,
    },
}

fn parse_args<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Err(usage());
    };

    match command.as_str() {
        "init" => {
            let mut json = false;
            let mut force = false;
            let mut project_root = None;
            let mut args = args.peekable();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--json" => json = true,
                    "--force" => force = true,
                    "--project-root" => {
                        let Some(path) = args.next() else {
                            return Err(format!("missing value for --project-root\n{}", usage()));
                        };
                        project_root = Some(PathBuf::from(path));
                    }
                    _ => return Err(format!("unknown argument for init: {arg}\n{}", usage())),
                }
            }
            Ok(Command::Init {
                json,
                force,
                project_root,
            })
        }
        "validate" => {
            let mut json = false;
            let mut project_root = None;
            let mut args = args.peekable();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--json" => json = true,
                    "--project-root" => {
                        let Some(path) = args.next() else {
                            return Err(format!("missing value for --project-root\n{}", usage()));
                        };
                        project_root = Some(PathBuf::from(path));
                    }
                    _ => {
                        return Err(format!("unknown argument for validate: {arg}\n{}", usage()))
                    }
                }
            }
            Ok(Command::Validate { json, project_root })
        }
        _ => Err(format!("unknown command: {command}\n{}", usage())),
    }
}

fn usage() -> String {
    "usage: pubpunk <init|validate> [--json] [--force] [--project-root <path>]".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_init_flags() {
        let command = parse_args([
            "init".to_string(),
            "--json".to_string(),
            "--force".to_string(),
            "--project-root".to_string(),
            "/tmp/project".to_string(),
        ])
        .unwrap();
        assert_eq!(
            command,
            Command::Init {
                json: true,
                force: true,
                project_root: Some(PathBuf::from("/tmp/project")),
            }
        );
    }

    #[test]
    fn parse_validate_flags() {
        let command = parse_args([
            "validate".to_string(),
            "--json".to_string(),
            "--project-root".to_string(),
            "/tmp/project".to_string(),
        ])
        .unwrap();
        assert_eq!(
            command,
            Command::Validate {
                json: true,
                project_root: Some(PathBuf::from("/tmp/project")),
            }
        );
    }

    #[test]
    fn run_init_json_returns_machine_output() {
        let root = temp_dir("cli-json");
        let output = run(
            [
                "init".to_string(),
                "--json".to_string(),
                "--project-root".to_string(),
                root.display().to_string(),
            ],
            Path::new("."),
        )
        .unwrap();
        assert!(output.text.starts_with("{\"ok\":true,"));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}
"##
    .to_string()
}

fn render_pubpunk_validate_tests_source() -> String {
    r####"use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "../crates/pubpunk-cli/src/main.rs"]
mod cli;

#[test]
fn validate_json_reports_success_for_initialized_tree() {
    let root = temp_dir("pubpunk-validate-json-ok");
    pubpunk_core::init_with_options(&root, false).unwrap();

    let output = cli::run(
        [
            "validate".to_string(),
            "--json".to_string(),
            "--project-root".to_string(),
            root.display().to_string(),
        ],
        Path::new("."),
    )
    .expect("validate json should succeed");

    assert!(output.text.contains("\"ok\":true"), "json={}", output.text);
    assert!(output.text.contains("\"issues\":[]"), "json={}", output.text);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn validate_json_reports_missing_directory_issue() {
    let root = temp_dir("pubpunk-validate-json-missing");
    pubpunk_core::init_with_options(&root, false).unwrap();
    fs::remove_dir_all(root.join(".pubpunk/targets")).unwrap();

    let output = cli::run(
        [
            "validate".to_string(),
            "--json".to_string(),
            "--project-root".to_string(),
            root.display().to_string(),
        ],
        Path::new("."),
    )
    .expect_err("validate json should fail");

    assert!(output.text.contains("\"ok\":false"), "json={}", output.text);
    assert!(
        output.text.contains("\"code\":\"missing_directory\""),
        "json={}",
        output.text
    );
    assert!(output.text.contains(".pubpunk/targets"), "json={}", output.text);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn validate_json_rejects_unparseable_project_toml() {
    let root = temp_dir("pubpunk-validate-json-parse");
    pubpunk_core::init_with_options(&root, false).unwrap();
    fs::write(root.join(".pubpunk/project.toml"), "[defaults\nbroken").unwrap();

    let output = cli::run(
        [
            "validate".to_string(),
            "--json".to_string(),
            "--project-root".to_string(),
            root.display().to_string(),
        ],
        Path::new("."),
    )
    .expect_err("validate json should fail");

    assert!(
        output.text.contains("\"code\":\"unparseable_project_toml\""),
        "json={}",
        output.text
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn validate_json_rejects_secret_like_keys() {
    let root = temp_dir("pubpunk-validate-json-secret");
    pubpunk_core::init_with_options(&root, false).unwrap();
    let project_toml = root.join(".pubpunk/project.toml");
    let current = fs::read_to_string(&project_toml).unwrap();
    fs::write(project_toml, format!("{current}api_key = \"secret\"\n")).unwrap();

    let output = cli::run(
        [
            "validate".to_string(),
            "--json".to_string(),
            "--project-root".to_string(),
            root.display().to_string(),
        ],
        Path::new("."),
    )
    .expect_err("validate json should fail");

    assert!(
        output.text.contains("\"code\":\"secret_like_key\""),
        "json={}",
        output.text
    );
    fs::remove_dir_all(&root).unwrap();
}

fn temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
    fs::create_dir_all(&path).unwrap();
    path
}
"####
        .to_string()
}

fn render_pubpunk_init_core_source() -> String {
    r####"use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SKELETON_DIRS: &[&str] = &[
    ".pubpunk",
    ".pubpunk/style",
    ".pubpunk/style/examples",
    ".pubpunk/targets",
    ".pubpunk/review",
    ".pubpunk/lint",
    ".pubpunk/agent",
    ".pubpunk/local",
    ".pubpunk/local/drafts",
    ".pubpunk/local/reports",
    ".pubpunk/local/cache",
    ".pubpunk/local/generated",
];

const PROJECT_TOML: &str = concat!(
    "schema_version = \"pubpunk.project.v1\"\n",
    "project_id = \"pubpunk\"\n",
    "display_name = \"Pubpunk\"\n",
    "\n",
    "[defaults]\n",
    "target = \"forem\"\n",
    "task = \"draft\"\n",
    "locale = \"en\"\n",
    "publish_mode = \"draft\"\n",
    "\n",
    "[paths]\n",
    "style_dir = \"style\"\n",
    "targets_dir = \"targets\"\n",
    "review_dir = \"review\"\n",
    "lint_dir = \"lint\"\n",
    "agent_dir = \"agent\"\n",
    "local_dir = \"local\"\n",
    "\n",
    "[agent]\n",
    "skill_path = \"agent/skill.md\"\n",
    "\n",
    "[local]\n",
    "state_db = \"local/state.db\"\n",
    "drafts_dir = \"local/drafts\"\n",
    "reports_dir = \"local/reports\"\n",
    "cache_dir = \"local/cache\"\n",
    "generated_dir = \"local/generated\"\n",
);

const SKELETON_FILES: &[(&str, &str)] = &[
    (".pubpunk/project.toml", PROJECT_TOML),
    (".pubpunk/style/style.toml", concat!("[style]\n", "version = 1\n")),
    (".pubpunk/style/voice.md", "# voice\n"),
    (".pubpunk/style/lexicon.toml", "[lexicon]\n"),
    (".pubpunk/style/normalize.toml", "[normalize]\n"),
    (".pubpunk/agent/skill.md", "# pubpunk local skill\n"),
    (".pubpunk/local/.gitignore", concat!("*\n", "!.gitignore\n")),
];

const REQUIRED_PROJECT_LINES: &[&str] = &[
    "schema_version = \"pubpunk.project.v1\"",
    "project_id = \"pubpunk\"",
    "display_name = \"Pubpunk\"",
    "target = \"forem\"",
    "task = \"draft\"",
    "locale = \"en\"",
    "publish_mode = \"draft\"",
    "style_dir = \"style\"",
    "targets_dir = \"targets\"",
    "review_dir = \"review\"",
    "lint_dir = \"lint\"",
    "agent_dir = \"agent\"",
    "local_dir = \"local\"",
    "skill_path = \"agent/skill.md\"",
    "state_db = \"local/state.db\"",
    "drafts_dir = \"local/drafts\"",
    "reports_dir = \"local/reports\"",
    "cache_dir = \"local/cache\"",
    "generated_dir = \"local/generated\"",
];

const PATH_KEYS: &[&str] = &[
    "style_dir",
    "targets_dir",
    "review_dir",
    "lint_dir",
    "agent_dir",
    "local_dir",
];

const LOCAL_KEYS: &[&str] = &[
    "state_db",
    "drafts_dir",
    "reports_dir",
    "cache_dir",
    "generated_dir",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitResult {
    pub root: PathBuf,
    pub created: Vec<PathBuf>,
    pub existing: Vec<PathBuf>,
    pub rewritten: Vec<PathBuf>,
}

impl InitResult {
    pub fn to_json(&self) -> String {
        let root = escape_json_string(&self.root.display().to_string());
        let created = json_path_array(&self.created);
        let existing = json_path_array(&self.existing);
        let rewritten = json_path_array(&self.rewritten);
        format!(
            "{{\"ok\":true,\"root\":\"{root}\",\"created\":{created},\"existing\":{existing},\"rewritten\":{rewritten}}}"
        )
    }
}

pub fn init(root: &Path) -> io::Result<InitResult> {
    init_with_options(root, false)
}

pub fn try_init(root: &Path) -> io::Result<InitResult> {
    init_with_options(root, false)
}

pub fn init_with_options(root: &Path, force: bool) -> io::Result<InitResult> {
    let root = prepare_root(root)?;
    let mut created = Vec::new();
    let mut existing = Vec::new();
    let mut rewritten = Vec::new();

    for relative in SKELETON_DIRS {
        let path = root.join(relative);
        if path.is_dir() {
            existing.push(PathBuf::from(relative));
            continue;
        }
        if path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} exists and is not a directory", path.display()),
            ));
        }
        fs::create_dir_all(&path)?;
        created.push(PathBuf::from(relative));
    }

    for (relative, contents) in SKELETON_FILES {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if path.exists() {
            let current = fs::read_to_string(&path)?;
            if force && current != *contents {
                fs::write(&path, contents)?;
                rewritten.push(PathBuf::from(relative));
            } else {
                existing.push(PathBuf::from(relative));
            }
            continue;
        }
        fs::write(&path, contents)?;
        created.push(PathBuf::from(relative));
    }

    Ok(InitResult {
        root,
        created,
        existing,
        rewritten,
    })
}

pub fn validate(root: &Path) -> io::Result<()> {
    let root = prepare_existing_root(root)?;
    for relative in SKELETON_DIRS {
        let path = root.join(relative);
        if !path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("missing directory: {}", path.display()),
            ));
        }
    }
    for (relative, _) in SKELETON_FILES {
        let path = root.join(relative);
        if !path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("missing file: {}", path.display()),
            ));
        }
    }

    let project_toml = fs::read_to_string(root.join(".pubpunk/project.toml"))?;
    validate_project_toml(&project_toml)?;
    Ok(())
}

fn validate_project_toml(contents: &str) -> io::Result<()> {
    for required in REQUIRED_PROJECT_LINES {
        if !contents.contains(required) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("missing required project.toml line: {required}"),
            ));
        }
    }

    let project_id = assignment_value(contents, "project_id").ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing required project_id")
    })?;
    if !is_slug_safe(project_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("project_id is not slug-safe: {project_id}"),
        ));
    }

    for key in PATH_KEYS {
        let value = assignment_value(contents, key).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, format!("missing required key: {key}"))
        })?;
        if Path::new(value).is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{key} must stay relative under .pubpunk: {value}"),
            ));
        }
    }

    for key in LOCAL_KEYS {
        let value = assignment_value(contents, key).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, format!("missing required key: {key}"))
        })?;
        if !value.starts_with("local/") || Path::new(value).is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{key} must stay under local/: {value}"),
            ));
        }
    }

    let lowered = contents.to_ascii_lowercase();
    for secret_like in ["token", "secret", "password", "api_key"] {
        if lowered.contains(secret_like) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("project.toml contains obvious secret-like field: {secret_like}"),
            ));
        }
    }

    Ok(())
}

fn assignment_value<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    for line in contents.lines() {
        let trimmed = line.trim();
        let prefix = format!("{key} = ");
        let Some(value) = trimmed.strip_prefix(&prefix) else {
            continue;
        };
        let value = value.trim();
        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            return Some(&value[1..value.len() - 1]);
        }
    }
    None
}

fn is_slug_safe(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
}

fn prepare_root(root: &Path) -> io::Result<PathBuf> {
    if root.exists() {
        return root.canonicalize().or_else(|_| Ok(root.to_path_buf()));
    }
    fs::create_dir_all(root)?;
    root.canonicalize().or_else(|_| Ok(root.to_path_buf()))
}

fn prepare_existing_root(root: &Path) -> io::Result<PathBuf> {
    if !root.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("root does not exist: {}", root.display()),
        ));
    }
    root.canonicalize().or_else(|_| Ok(root.to_path_buf()))
}

fn json_path_array(paths: &[PathBuf]) -> String {
    let items = paths
        .iter()
        .map(|path| format!("\"{}\"", escape_json_string(&path.display().to_string())))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{items}]")
}

fn escape_json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            c => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn init_creates_canonical_pubpunk_tree_and_is_idempotent() {
        let root = temp_dir("core-init");
        let first = init_with_options(&root, false).unwrap();
        assert!(first
            .created
            .iter()
            .any(|path| path == &PathBuf::from(".pubpunk/project.toml")));
        assert!(root.join(".pubpunk/targets").is_dir());
        assert!(root.join(".pubpunk/review").is_dir());
        assert!(root.join(".pubpunk/lint").is_dir());
        assert!(root.join(".pubpunk/local/drafts").is_dir());
        assert!(root.join(".pubpunk/style/examples").is_dir());
        assert_eq!(
            fs::read_to_string(root.join(".pubpunk/project.toml")).unwrap(),
            PROJECT_TOML
        );

        let second = init_with_options(&root, false).unwrap();
        assert!(second.created.is_empty());
        assert!(second
            .existing
            .iter()
            .any(|path| path == &PathBuf::from(".pubpunk/style/style.toml")));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn force_rewrites_modified_project_file() {
        let root = temp_dir("core-force");
        init_with_options(&root, false).unwrap();
        fs::write(root.join(".pubpunk/project.toml"), "schema_version = \"wrong\"\n").unwrap();
        let result = init_with_options(&root, true).unwrap();
        assert!(result
            .rewritten
            .iter()
            .any(|path| path == &PathBuf::from(".pubpunk/project.toml")));
        assert_eq!(
            fs::read_to_string(root.join(".pubpunk/project.toml")).unwrap(),
            PROJECT_TOML
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn validate_rejects_missing_required_directory() {
        let root = temp_dir("core-validate-missing");
        init_with_options(&root, false).unwrap();
        fs::remove_dir_all(root.join(".pubpunk/targets")).unwrap();
        let err = validate(&root).expect_err("validate should fail when required tree is missing");
        assert!(err.to_string().contains("missing directory"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn json_is_machine_readable() {
        let result = InitResult {
            root: PathBuf::from("/tmp/pubpunk"),
            created: vec![PathBuf::from(".pubpunk/project.toml")],
            existing: vec![],
            rewritten: vec![],
        };
        assert!(result
            .to_json()
            .contains("\"created\":[\".pubpunk/project.toml\"]"));
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
"####
    .to_string()
}

fn render_pubpunk_init_cli_source() -> String {
    r##"use std::env;
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};

fn main() -> ExitCode {
    match run(env::args().skip(1), Path::new(".")) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            process::ExitCode::from(1)
        }
    }
}

fn run<I>(args: I, cwd: &Path) -> Result<String, String>
where
    I: IntoIterator<Item = String>,
{
    let command = parse_args(args)?;
    match command {
        Command::Init {
            json,
            force,
            project_root,
        } => {
            let root = project_root.unwrap_or_else(|| cwd.to_path_buf());
            let result = pubpunk_core::init_with_options(&root, force)
                .map_err(|err| format!("pubpunk init failed: {err}"))?;
            if json {
                Ok(result.to_json())
            } else if result.created.is_empty() && result.rewritten.is_empty() {
                Ok("Initialized .pubpunk skeleton (no changes)".to_string())
            } else {
                Ok("Initialized .pubpunk skeleton".to_string())
            }
        }
        Command::Validate { project_root } => {
            let root = project_root.unwrap_or_else(|| cwd.to_path_buf());
            pubpunk_core::validate(&root)
                .map_err(|err| format!("pubpunk validate failed: {err}"))?;
            Ok("Validated .pubpunk skeleton".to_string())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Init {
        json: bool,
        force: bool,
        project_root: Option<PathBuf>,
    },
    Validate {
        project_root: Option<PathBuf>,
    },
}

fn parse_args<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Err(usage());
    };

    match command.as_str() {
        "init" => {
            let mut json = false;
            let mut force = false;
            let mut project_root = None;
            let mut args = args.peekable();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--json" => json = true,
                    "--force" => force = true,
                    "--project-root" => {
                        let Some(path) = args.next() else {
                            return Err(format!("missing value for --project-root\n{}", usage()));
                        };
                        project_root = Some(PathBuf::from(path));
                    }
                    _ => return Err(format!("unknown argument for init: {arg}\n{}", usage())),
                }
            }
            Ok(Command::Init {
                json,
                force,
                project_root,
            })
        }
        "validate" => {
            let mut project_root = None;
            let mut args = args.peekable();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--project-root" => {
                        let Some(path) = args.next() else {
                            return Err(format!("missing value for --project-root\n{}", usage()));
                        };
                        project_root = Some(PathBuf::from(path));
                    }
                    _ => {
                        return Err(format!("unknown argument for validate: {arg}\n{}", usage()))
                    }
                }
            }
            Ok(Command::Validate { project_root })
        }
        _ => Err(format!("unknown command: {command}\n{}", usage())),
    }
}

fn usage() -> String {
    "usage: pubpunk <init|validate> [--json] [--force] [--project-root <path>]".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_init_flags() {
        let command = parse_args([
            "init".to_string(),
            "--json".to_string(),
            "--force".to_string(),
            "--project-root".to_string(),
            "/tmp/project".to_string(),
        ])
        .unwrap();
        assert_eq!(
            command,
            Command::Init {
                json: true,
                force: true,
                project_root: Some(PathBuf::from("/tmp/project")),
            }
        );
    }

    #[test]
    fn run_init_json_returns_machine_output() {
        let root = temp_dir("cli-json");
        let output = run(
            [
                "init".to_string(),
                "--json".to_string(),
                "--project-root".to_string(),
                root.display().to_string(),
            ],
            Path::new("."),
        )
        .unwrap();
        assert!(output.starts_with("{\"ok\":true,"));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}
"##
    .to_string()
}

fn render_pubpunk_init_tests_source() -> String {
    r####"use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const EXPECTED_PROJECT_TOML: &str = concat!(
    "schema_version = \"pubpunk.project.v1\"\n",
    "project_id = \"pubpunk\"\n",
    "display_name = \"Pubpunk\"\n",
    "\n",
    "[defaults]\n",
    "target = \"forem\"\n",
    "task = \"draft\"\n",
    "locale = \"en\"\n",
    "publish_mode = \"draft\"\n",
    "\n",
    "[paths]\n",
    "style_dir = \"style\"\n",
    "targets_dir = \"targets\"\n",
    "review_dir = \"review\"\n",
    "lint_dir = \"lint\"\n",
    "agent_dir = \"agent\"\n",
    "local_dir = \"local\"\n",
    "\n",
    "[agent]\n",
    "skill_path = \"agent/skill.md\"\n",
    "\n",
    "[local]\n",
    "state_db = \"local/state.db\"\n",
    "drafts_dir = \"local/drafts\"\n",
    "reports_dir = \"local/reports\"\n",
    "cache_dir = \"local/cache\"\n",
    "generated_dir = \"local/generated\"\n",
);

#[test]
fn init_creates_canonical_pubpunk_skeleton() {
    let root = temp_dir("pubpunk-test-init");
    let result = pubpunk_core::init_with_options(&root, false).expect("init should succeed");
    assert!(result
        .created
        .iter()
        .any(|path| path == &PathBuf::from(".pubpunk/project.toml")));
    for relative in [
        ".pubpunk/style/examples",
        ".pubpunk/targets",
        ".pubpunk/review",
        ".pubpunk/lint",
        ".pubpunk/local/drafts",
        ".pubpunk/local/reports",
        ".pubpunk/local/cache",
        ".pubpunk/local/generated",
    ] {
        assert!(root.join(relative).is_dir(), "missing {relative}");
    }
    assert_eq!(
        fs::read_to_string(root.join(".pubpunk/project.toml")).unwrap(),
        EXPECTED_PROJECT_TOML
    );
    pubpunk_core::validate(&root).expect("validate should accept initialized tree");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn init_json_reports_rewrites_after_force() {
    let root = temp_dir("pubpunk-test-json");
    pubpunk_core::init_with_options(&root, false).unwrap();
    fs::write(root.join(".pubpunk/project.toml"), "schema_version = \"wrong\"\n").unwrap();
    let result = pubpunk_core::init_with_options(&root, true).unwrap();
    let json = result.to_json();
    assert!(
        json.contains("\"rewritten\":[\".pubpunk/project.toml\"]"),
        "json={json}"
    );
    assert_eq!(
        fs::read_to_string(root.join(".pubpunk/project.toml")).unwrap(),
        EXPECTED_PROJECT_TOML
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn validate_rejects_missing_targets_dir() {
    let root = temp_dir("pubpunk-test-validate");
    pubpunk_core::init_with_options(&root, false).unwrap();
    fs::remove_dir_all(root.join(".pubpunk/targets")).unwrap();
    let err =
        pubpunk_core::validate(&root).expect_err("validate should fail when required tree is missing");
    assert!(err.to_string().contains("missing directory"));
    fs::remove_dir_all(&root).unwrap();
}

fn temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
    fs::create_dir_all(&path).unwrap();
    path
}
"####
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

fn effective_execution_contract(repo_root: &Path, contract: &Contract) -> Result<Contract> {
    if is_bounded_execution_task(contract)
        || contract.allowed_scope.is_empty()
        || contract.allowed_scope.len() > 5
        || contract.entry_points.is_empty()
        || contract.entry_points.len() > 5
    {
        return Ok(contract.clone());
    }
    let needs_creatable_tests_scope = contract_needs_creatable_tests_scope(repo_root, contract)?;
    let bootstrap_scaffold_paths = legacy_controller_bootstrap_scaffold_paths(contract);
    if !bootstrap_scaffold_paths.is_empty()
        && bootstrap_scaffold_paths
            .iter()
            .any(|path| !repo_root.join(path).exists())
    {
        return Ok(contract.clone());
    }

    let mut remaining_scope_files = 16usize;
    let mut expanded_scope = Vec::new();
    for scope in &contract.allowed_scope {
        if is_explicit_repo_file_scope(scope) {
            expanded_scope.push(scope.clone());
            continue;
        }
        let discovered =
            collect_existing_allowed_scope_files(repo_root, scope, &mut remaining_scope_files)?;
        extend_unique_paths(&mut expanded_scope, &discovered);
        if remaining_scope_files == 0 {
            break;
        }
    }
    if contract.allowed_scope.iter().any(|path| path == "tests") {
        expanded_scope.retain(|path| is_executable_test_surface(path));
    }
    if needs_creatable_tests_scope {
        extend_unique_paths(
            &mut expanded_scope,
            &[synthesized_test_entrypoint(contract).to_string()],
        );
    }
    if expanded_scope.is_empty()
        || expanded_scope.len() > 8
        || expanded_scope
            .iter()
            .any(|path| !is_explicit_repo_file_scope(path))
    {
        return Ok(contract.clone());
    }

    let mut effective = contract.clone();
    effective.allowed_scope = expanded_scope.clone();
    effective.entry_points = contract.entry_points.clone();
    extend_unique_paths(&mut effective.entry_points, &expanded_scope);
    Ok(effective)
}

fn contract_needs_creatable_tests_scope(repo_root: &Path, contract: &Contract) -> Result<bool> {
    if !contract.allowed_scope.iter().any(|path| path == "tests") {
        return Ok(false);
    }
    let mut remaining = 16usize;
    let discovered = collect_existing_allowed_scope_files(repo_root, "tests", &mut remaining)?;
    Ok(!discovered
        .iter()
        .any(|path| is_executable_test_surface(path)))
}

fn is_executable_test_surface(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    !(lower.ends_with("/readme.md")
        || lower.ends_with("/readme.txt")
        || lower.ends_with("/.gitkeep")
        || lower == "tests/readme.md"
        || lower == "tests/readme.txt"
        || lower == "tests/.gitkeep")
}

fn synthesized_test_entrypoint(contract: &Contract) -> &'static str {
    let mut text = contract.prompt_source.to_ascii_lowercase();
    for item in contract
        .expected_interfaces
        .iter()
        .chain(contract.behavior_requirements.iter())
    {
        text.push('\n');
        text.push_str(&item.to_ascii_lowercase());
    }
    if text.contains("json") {
        "tests/init_json.rs"
    } else {
        "tests/init.rs"
    }
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
    if contract.allowed_scope.len() > 5 || contract.entry_points.len() > 5 {
        return false;
    }
    if contract.allowed_scope.is_empty() || contract.entry_points.is_empty() {
        return false;
    }
    if !contract.allowed_scope.iter().all(|path| {
        repo_root.join(path).exists()
            || (is_explicit_repo_file_scope(path)
                && contract.entry_points.iter().any(|entry| entry == path))
    }) {
        return false;
    }
    if !contract
        .entry_points
        .iter()
        .all(|path| repo_root.join(path).exists() || is_explicit_repo_file_scope(path))
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
        "init",
        "validate",
        "json",
        "skeleton",
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
    stall_timeout: Duration,
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
        progress.clone(),
        false,
        Some(stderr_live.clone()),
    );
    let start = Instant::now();
    let (timed_out, stalled, orphaned, response) = loop {
        if let Some(response) = detect_patch_lane_response(&stdout_live, &stderr_live) {
            terminate_process_tree(&mut child, child_pid);
            break (false, false, false, Some(response));
        }
        if child.try_wait()?.is_some() {
            if !wait_for_stream_completion(
                &stdout_handle,
                &stderr_handle,
                codex_executor_orphan_grace_timeout(),
            ) {
                terminate_process_tree(&mut child, child_pid);
                break (false, false, true, None);
            }
            break (false, false, false, None);
        }
        if progress.stalled_for(stall_timeout) {
            let response = detect_patch_lane_response(&stdout_live, &stderr_live);
            terminate_process_tree(&mut child, child_pid);
            break (false, response.is_none(), false, response);
        }
        if start.elapsed() >= timeout {
            let response = detect_patch_lane_response(&stdout_live, &stderr_live);
            terminate_process_tree(&mut child, child_pid);
            break (response.is_none(), false, false, response);
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
        stalled,
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

fn restore_damaged_entry_point_snapshots(
    repo_root: &std::path::Path,
    snapshots: &[EntryPointSnapshot],
) -> Result<Vec<String>> {
    let mut restored = Vec::new();
    for snapshot in snapshots {
        let Some(original) = snapshot.content.as_ref() else {
            continue;
        };
        if original.is_empty() {
            continue;
        }
        let file_path = repo_root.join(&snapshot.path);
        let damaged = match fs::metadata(&file_path) {
            Ok(metadata) => metadata.len() == 0,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("read entry point damage probe {}", snapshot.path));
            }
        };
        if !damaged {
            continue;
        }
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create restored entry point parent {}", snapshot.path))?;
        }
        fs::write(&file_path, original)
            .with_context(|| format!("restore damaged entry point {}", snapshot.path))?;
        restored.push(snapshot.path.clone());
    }
    Ok(restored)
}

fn restore_entry_point_snapshots(
    repo_root: &std::path::Path,
    snapshots: &[EntryPointSnapshot],
) -> Result<Vec<String>> {
    let mut restored = Vec::new();
    for snapshot in snapshots {
        let file_path = repo_root.join(&snapshot.path);
        match snapshot.content.as_ref() {
            Some(original) => {
                let current = fs::read_to_string(&file_path).ok();
                if current.as_ref() == Some(original) {
                    continue;
                }
                if let Some(parent) = file_path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("create restored entry point parent {}", snapshot.path)
                    })?;
                }
                fs::write(&file_path, original)
                    .with_context(|| format!("restore entry point {}", snapshot.path))?;
                restored.push(snapshot.path.clone());
            }
            None => {
                if !file_path.exists() {
                    continue;
                }
                fs::remove_file(&file_path)
                    .with_context(|| format!("remove created entry point {}", snapshot.path))?;
                restored.push(snapshot.path.clone());
            }
        }
    }
    Ok(restored)
}

fn maybe_rollback_failed_patch_lane_output(
    repo_root: &std::path::Path,
    snapshots: &[EntryPointSnapshot],
    patched_worktree: bool,
    mut output: ExecuteOutput,
) -> Result<ExecuteOutput> {
    if !patched_worktree || output.success {
        return Ok(output);
    }
    let restored_paths = restore_entry_point_snapshots(repo_root, snapshots)?;
    if restored_paths.is_empty() {
        return Ok(output);
    }
    let original_summary = output.summary;
    output.success = false;
    output.checks_run.clear();
    output.summary = format!(
        "failed bounded patch/apply changes were rolled back for {}; original outcome: {}",
        restored_paths.join(", "),
        original_summary
    );
    Ok(output)
}

fn finalize_patch_lane_output(
    repo_root: &std::path::Path,
    snapshots: &[EntryPointSnapshot],
    mut output: ExecuteOutput,
) -> Result<ExecuteOutput> {
    let restored_paths = restore_damaged_entry_point_snapshots(repo_root, snapshots)?;
    if restored_paths.is_empty() {
        return Ok(output);
    }
    let original_summary = output.summary;
    output.success = false;
    output.checks_run.clear();
    output.summary = format!(
        "{BLOCKED_EXECUTION_SENTINEL} bounded patch/apply execution damaged {}; original contents were restored (original outcome: {})",
        restored_paths.join(", "),
        original_summary
    );
    Ok(output)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApplyPatchUpdate {
    path: String,
    hunks: Vec<ApplyPatchHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApplyPatchSnapshot {
    path: String,
    original: String,
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
    let snapshots = collect_apply_patch_snapshots(repo_root, updates)?;
    let apply_result = apply_patch_updates_in_repo(repo_root, updates)
        .and_then(|_| validate_patch_apply_results(repo_root, &snapshots));
    if let Err(err) = apply_result {
        restore_apply_patch_snapshots(repo_root, &snapshots)
            .with_context(|| "restore patch targets after failed apply")?;
        return Err(err);
    }
    Ok(())
}

fn collect_apply_patch_snapshots(
    repo_root: &Path,
    updates: &[ApplyPatchUpdate],
) -> Result<Vec<ApplyPatchSnapshot>> {
    let mut snapshots = Vec::with_capacity(updates.len());
    for update in updates {
        let file_path = repo_root.join(&update.path);
        if !file_path.exists() {
            return Err(anyhow!("patch target does not exist: {}", update.path));
        }
        snapshots.push(ApplyPatchSnapshot {
            path: update.path.clone(),
            original: fs::read_to_string(&file_path)
                .with_context(|| format!("read patch target {}", file_path.display()))?,
        });
    }
    Ok(snapshots)
}

fn apply_patch_updates_in_repo(repo_root: &Path, updates: &[ApplyPatchUpdate]) -> Result<()> {
    for update in updates {
        apply_update_in_repo(repo_root, update)?;
    }
    Ok(())
}

fn validate_patch_apply_results(repo_root: &Path, snapshots: &[ApplyPatchSnapshot]) -> Result<()> {
    let mut corrupted_paths = Vec::new();
    for snapshot in snapshots {
        let file_path = repo_root.join(&snapshot.path);
        let metadata = fs::metadata(&file_path)
            .with_context(|| format!("read patch target metadata {}", file_path.display()))?;
        if metadata.len() == 0 && !snapshot.original.is_empty() {
            corrupted_paths.push(snapshot.path.clone());
        }
    }
    if !corrupted_paths.is_empty() {
        return Err(anyhow!(
            "{BLOCKED_EXECUTION_SENTINEL} patch/apply aborted because {} are currently zero-byte; original contents were restored",
            corrupted_paths.join(", ")
        ));
    }
    Ok(())
}

fn restore_apply_patch_snapshots(repo_root: &Path, snapshots: &[ApplyPatchSnapshot]) -> Result<()> {
    for snapshot in snapshots {
        let file_path = repo_root.join(&snapshot.path);
        fs::write(&file_path, &snapshot.original)
            .with_context(|| format!("restore patch target {}", file_path.display()))?;
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
        assert!(prompt.contains("Original goal: x"));
        assert!(prompt.contains(
            "Treat the original goal as authoritative if the behavior requirements below are abbreviated."
        ));
        assert!(prompt.contains("Start by inspecting only the listed entry points"));
        assert!(prompt.contains("Do not perform broad repo-wide search."));
        assert!(prompt.contains(blocked_execution_template()));
        assert!(prompt.contains(successful_execution_template()));
    }

    #[test]
    fn build_exec_prompt_mentions_visible_allowed_files_for_directory_scope() {
        let contract = Contract {
            id: "ct_dir".into(),
            feature_id: "feat_dir".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "implement pubpunk init".into(),
            entry_points: vec!["crates/pubpunk-cli/Cargo.toml".into()],
            import_paths: vec![],
            expected_interfaces: vec!["bounded implementation slice".into()],
            behavior_requirements: vec!["implement pubpunk init".into()],
            allowed_scope: vec!["crates/pubpunk-cli".into(), "tests".into()],
            target_checks: vec!["cargo test -p pubpunk-cli".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let prompt = build_exec_prompt_with_mode_and_visible_files(
            &contract,
            None,
            &[],
            false,
            &[
                "crates/pubpunk-cli/Cargo.toml".into(),
                "crates/pubpunk-cli/src/main.rs".into(),
                "tests/init.rs".into(),
            ],
        );
        assert!(prompt.contains(
            "Current allowed files available for direct edit: crates/pubpunk-cli/Cargo.toml, crates/pubpunk-cli/src/main.rs, tests/init.rs."
        ));
        assert!(prompt.contains(
            "For this directory-scoped slice, treat these listed files as the initial bounded edit set before doing any more orientation."
        ));
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
                capability_resolution: None,
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
                capability_resolution: None,
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
                capability_resolution: None,
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
                capability_resolution: None,
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
        let prompt = build_patch_apply_prompt(&contract, &ContextPack::default(), 0, 2, None);
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
            "The following controller-owned scaffold files were materialized for this run and must remain present"
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
                capability_resolution: None,
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
    fn execution_lane_uses_patch_apply_for_init_slice_when_tests_dir_needs_new_test_file() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-patch-lane-init-{}-{}",
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
        fs::write(root.join("crates/pubpunk-cli/Cargo.toml"), "[package]\n").unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(root.join("crates/pubpunk-core/Cargo.toml"), "[package]\n").unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            "pub fn init() {}\n",
        )
        .unwrap();
        fs::write(root.join("tests/README.md"), "# tests\n").unwrap();

        let contract = Contract {
            id: "ct_init".into(),
            feature_id: "feat_init".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "implement pubpunk init command".into(),
            entry_points: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["bounded implementation slice".into()],
            behavior_requirements: vec!["implement pubpunk init".into()],
            allowed_scope: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests".into(),
            ],
            target_checks: vec!["cargo test -p pubpunk-cli".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let effective = effective_execution_contract(&root, &contract).unwrap();
        assert!(is_bounded_execution_task(&effective));
        assert!(effective
            .allowed_scope
            .contains(&"tests/init.rs".to_string()));
        assert_eq!(
            execution_lane_for_contract(&root, &effective),
            ExecutionLane::PatchApply
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn execution_lane_uses_patch_apply_for_init_slice_with_existing_test_file() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-patch-lane-init-existing-tests-{}-{}",
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
        fs::write(root.join("crates/pubpunk-cli/Cargo.toml"), "[package]\n").unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(root.join("crates/pubpunk-core/Cargo.toml"), "[package]\n").unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            "pub fn init() {}\n",
        )
        .unwrap();
        fs::write(root.join("tests/README.md"), "# tests\n").unwrap();
        fs::write(root.join("tests/init_json.rs"), "#[test]\nfn smoke() {}\n").unwrap();

        let contract = Contract {
            id: "ct_init_existing_tests".into(),
            feature_id: "feat_init_existing_tests".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "implement pubpunk init command".into(),
            entry_points: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["bounded implementation slice".into()],
            behavior_requirements: vec!["implement pubpunk init".into()],
            allowed_scope: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests".into(),
            ],
            target_checks: vec!["cargo test -p pubpunk-cli".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let effective = effective_execution_contract(&root, &contract).unwrap();
        assert!(is_bounded_execution_task(&effective));
        assert!(!effective
            .allowed_scope
            .contains(&"tests/README.md".to_string()));
        assert!(effective
            .allowed_scope
            .contains(&"tests/init_json.rs".to_string()));
        assert_eq!(
            execution_lane_for_contract(&root, &effective),
            ExecutionLane::PatchApply
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rollback_failed_patch_lane_output_restores_pre_run_snapshots() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-rollback-failed-patch-lane-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn stable() {}\n").unwrap();
        fs::write(
            root.join("tests/init_json.rs"),
            "#[test]\nfn init_json() {}\n",
        )
        .unwrap();

        let contract = Contract {
            id: "ct_cleanup".into(),
            feature_id: "feat_cleanup".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "remove style/examples cleanup".into(),
            entry_points: vec!["src/lib.rs".into(), "tests/init_json.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["cleanup slice".into()],
            behavior_requirements: vec!["remove obsolete style examples references".into()],
            allowed_scope: vec!["src/lib.rs".into(), "tests/init_json.rs".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        let snapshots = capture_entry_point_snapshots(&root, &contract, &[]).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn broken() {\n").unwrap();
        fs::write(root.join("tests/init_json.rs"), "#[test]\nfn broken() {\n").unwrap();

        let output = maybe_rollback_failed_patch_lane_output(
            &root,
            &snapshots,
            true,
            ExecuteOutput {
                success: false,
                summary: "patch/apply lane failed to apply patch: apply hunk in src/lib.rs".into(),
                checks_run: vec!["cargo test --workspace".into()],
                cost_usd: None,
                duration_ms: 0,
            },
        )
        .unwrap();

        assert!(!output.success);
        assert!(output.summary.contains("rolled back"), "{}", output.summary);
        assert!(output.checks_run.is_empty());
        assert_eq!(
            fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "pub fn stable() {}\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("tests/init_json.rs")).unwrap(),
            "#[test]\nfn init_json() {}\n"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn finalize_patch_lane_output_restores_damaged_entry_points() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-finalize-zero-byte-restore-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn stable() {}\n").unwrap();
        fs::write(root.join("tests/cleanup.rs"), "#[test]\nfn cleanup() {}\n").unwrap();

        let contract = Contract {
            id: "ct_cleanup".into(),
            feature_id: "feat_cleanup".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "remove style/examples cleanup".into(),
            entry_points: vec!["src/lib.rs".into(), "tests/cleanup.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["cleanup slice".into()],
            behavior_requirements: vec!["remove obsolete style examples references".into()],
            allowed_scope: vec!["src/lib.rs".into(), "tests/cleanup.rs".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        let snapshots = capture_entry_point_snapshots(&root, &contract, &[]).unwrap();
        fs::write(root.join("src/lib.rs"), "").unwrap();
        fs::write(root.join("tests/cleanup.rs"), "").unwrap();
        let output = finalize_patch_lane_output(
            &root,
            &snapshots,
            ExecuteOutput {
                success: false,
                summary: "PUNK_EXECUTION_BLOCKED: Missing file context for cleanup patch".into(),
                checks_run: Vec::new(),
                cost_usd: None,
                duration_ms: 0,
            },
        )
        .unwrap();

        assert!(!output.success);
        assert!(output.summary.starts_with(BLOCKED_EXECUTION_SENTINEL));
        assert!(
            output.summary.contains("original contents were restored"),
            "{}",
            output.summary
        );
        assert_eq!(
            fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "pub fn stable() {}\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("tests/cleanup.rs")).unwrap(),
            "#[test]\nfn cleanup() {}\n"
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
    fn patch_apply_retry_feedback_includes_summary_and_recent_logs() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-patch-retry-feedback-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let stdout = root.join("stdout.log");
        let stderr = root.join("stderr.log");
        fs::write(
            &stdout,
            "[punk check] cargo test -p pubpunk-cli\nrecent stdout\n",
        )
        .unwrap();
        fs::write(&stderr, "error[E0408]: compile failed\n").unwrap();

        let feedback = patch_apply_retry_feedback(
            "patch/apply lane check failed: cargo test -p pubpunk-cli: compile failed",
            &root,
            &["crates/pubpunk-cli/src/main.rs".into()],
            &stdout,
            &stderr,
        )
        .unwrap();
        assert!(feedback.contains("Summary: patch/apply lane check failed"));
        assert!(feedback.contains("Recent stdout:"));
        assert!(feedback.contains("Recent stderr:"));
        assert!(feedback.contains("error[E0408]"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn build_patch_apply_prompt_includes_retry_feedback_on_second_pass() {
        let contract = Contract {
            id: "ct_retry".into(),
            feature_id: "feat_retry".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "implement pubpunk init command".into(),
            entry_points: vec!["crates/pubpunk-cli/src/main.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["pubpunk init".into()],
            behavior_requirements: vec!["add --json behavior".into()],
            allowed_scope: vec!["crates/pubpunk-cli/src/main.rs".into()],
            target_checks: vec!["cargo test -p pubpunk-cli".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        let pack = ContextPack::default();
        let prompt = build_patch_apply_prompt(
            &contract,
            &pack,
            1,
            3,
            Some("Summary: patch/apply lane check failed: cargo test -p pubpunk-cli"),
        );
        assert!(prompt.contains("patch/apply repair pass 2 of 3 after a failed prior pass"));
        assert!(prompt.contains("Latest repair feedback:"));
        assert!(prompt.contains("cargo test -p pubpunk-cli"));
        assert!(prompt.contains("repair only the concrete issue described in the feedback below"));
        assert!(prompt.contains("duplicate definitions or duplicate test modules"));
    }

    #[test]
    fn patch_apply_max_attempts_allows_sequential_failed_checks() {
        let one_check = Contract {
            id: "ct_one".into(),
            feature_id: "feat_one".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "implement".into(),
            entry_points: vec!["src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["iface".into()],
            behavior_requirements: vec!["behavior".into()],
            allowed_scope: vec!["src/lib.rs".into()],
            target_checks: vec!["cargo test -p one".into()],
            integrity_checks: vec![],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        let two_checks = Contract {
            integrity_checks: vec!["cargo test --workspace".into()],
            ..one_check.clone()
        };

        assert_eq!(patch_apply_max_attempts(&one_check), 2);
        assert_eq!(patch_apply_max_attempts(&two_checks), 3);
        assert!(should_retry_patch_apply_after_check_failure(
            &two_checks,
            "patch/apply lane check failed: cargo test -p one: compile failed",
            0,
            patch_apply_max_attempts(&two_checks),
        ));
        assert!(should_retry_patch_apply_after_check_failure(
            &two_checks,
            "patch/apply lane check failed: cargo test --workspace: compile failed",
            1,
            patch_apply_max_attempts(&two_checks),
        ));
        assert!(!should_retry_patch_apply_after_check_failure(
            &two_checks,
            "patch/apply lane check failed: cargo test --workspace: compile failed",
            2,
            patch_apply_max_attempts(&two_checks),
        ));
    }

    #[test]
    fn patch_apply_retry_feedback_includes_current_source_snippets_for_reported_lines() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-patch-retry-snippets-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/pubpunk-cli/src")).unwrap();
        let stdout = root.join("stdout.log");
        let stderr = root.join("stderr.log");
        fs::write(
            root.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {}\n\n#[cfg(test)]\nmod tests {}\n\n#[cfg(test)]\nmod tests {}\n",
        )
        .unwrap();
        fs::write(&stdout, "").unwrap();
        fs::write(
            &stderr,
            "error[E0428]: the name `tests` is defined multiple times\n  --> crates/pubpunk-cli/src/main.rs:6:1\n",
        )
        .unwrap();

        let feedback = patch_apply_retry_feedback(
            "patch/apply lane check failed: cargo test -p pubpunk-cli: compile failed",
            &root,
            &["crates/pubpunk-cli/src/main.rs".into()],
            &stdout,
            &stderr,
        )
        .unwrap();
        assert!(feedback.contains("Repair directives:"));
        assert!(feedback.contains("Keep exactly one `mod tests` block per file"));
        assert!(feedback.contains("Current source near reported failures:"));
        assert!(feedback.contains("crates/pubpunk-cli/src/main.rs around line 6"));
        assert!(feedback.contains("mod tests {}"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn patch_apply_retry_feedback_adds_timeout_directive() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-patch-timeout-feedback-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let stdout = root.join("stdout.log");
        let stderr = root.join("stderr.log");
        fs::write(&stdout, "").unwrap();
        fs::write(
            &stderr,
            "- if implementation is blocked inside allowed scope, output exactly one `PUNK_EXECUTION_BLOCKED: <reason>` line and nothing else\n",
        )
        .unwrap();

        let feedback = patch_apply_retry_feedback(
            "codex command timed out after 90s before emitting a patch",
            &root,
            &["crates/pubpunk-core/src/lib.rs".into()],
            &stdout,
            &stderr,
        )
        .unwrap();
        assert!(feedback.contains("Emit the smallest valid apply_patch response immediately"));

        let _ = fs::remove_dir_all(&root);
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
    fn apply_patch_in_repo_restores_prior_updates_when_later_hunk_fails() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-apply-patch-rollback-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.rs"), "pub fn a() {}\n").unwrap();
        fs::write(root.join("src/b.rs"), "pub fn b() {}\n").unwrap();

        let patch = "*** Begin Patch\n*** Update File: src/a.rs\n@@\n-pub fn a() {}\n+pub fn a_changed() {}\n*** Update File: src/b.rs\n@@\n-pub fn missing() {}\n+pub fn b_changed() {}\n*** End Patch\n";
        let updates =
            validate_patch_scope(patch, &[String::from("src/a.rs"), String::from("src/b.rs")])
                .unwrap();
        let err = apply_patch_in_repo(&root, &updates).unwrap_err();
        assert!(err.to_string().contains("apply hunk in src/b.rs"));
        assert_eq!(
            fs::read_to_string(root.join("src/a.rs")).unwrap(),
            "pub fn a() {}\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("src/b.rs")).unwrap(),
            "pub fn b() {}\n"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_patch_in_repo_blocks_and_restores_zero_byte_results() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-apply-patch-zero-byte-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn old() {}").unwrap();

        let patch =
            "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-pub fn old() {}\n*** End Patch\n";
        let updates = validate_patch_scope(patch, &[String::from("src/lib.rs")]).unwrap();
        let err = apply_patch_in_repo(&root, &updates).unwrap_err();
        assert!(err
            .to_string()
            .contains("PUNK_EXECUTION_BLOCKED: patch/apply aborted because src/lib.rs are currently zero-byte"));
        assert_eq!(
            fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "pub fn old() {}"
        );

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
            Duration::from_secs(1),
            stdout_path,
            stderr_path,
            executor_pid,
        )
        .unwrap();

        assert!(!output.timed_out);
        assert!(!output.stalled);
        assert!(!output.orphaned);
        assert!(matches!(output.response, Some(PatchLaneResponse::Patch(_))));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_patch_lane_command_with_timeout_marks_no_output_stall() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-patch-stall-{}-{}",
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
        command
            .arg("-lc")
            .arg("printf 'controller prompt\\n'; sleep 5");

        let output = run_patch_lane_command_with_timeout(
            &mut command,
            Duration::from_secs(5),
            Duration::from_millis(400),
            stdout_path,
            stderr_path,
            executor_pid,
        )
        .unwrap();

        assert!(!output.timed_out);
        assert!(output.stalled);
        assert!(!output.orphaned);
        assert!(output.response.is_none());

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
                capability_resolution: None,
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
                capability_resolution: None,
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
                capability_resolution: None,
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
    fn materialize_controller_bootstrap_scaffold_uses_frozen_go_kind() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-go-bootstrap-materialize-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let created = materialize_controller_bootstrap_scaffold(&ExecuteInput {
            repo_root: root.clone(),
            contract: Contract {
                id: "ct_go".into(),
                feature_id: "feat_go".into(),
                version: 1,
                status: punk_domain::ContractStatus::Approved,
                prompt_source: "bootstrap initial Go module for pubpunk".into(),
                entry_points: vec!["go.mod".into()],
                import_paths: vec![],
                expected_interfaces: vec!["initial Go scaffold".into()],
                behavior_requirements: vec!["bootstrap project".into()],
                allowed_scope: vec!["go.mod".into(), "cmd".into(), "internal".into()],
                target_checks: vec!["go test ./...".into()],
                integrity_checks: vec!["go test ./...".into()],
                risk_level: "medium".into(),
                created_at: "now".into(),
                approved_at: Some("now".into()),
            },
            capability_resolution: Some(punk_domain::FrozenCapabilityResolution {
                schema: "specpunk/contract-capability-resolution/v1".into(),
                version: 1,
                contract_id: "ct_go".into(),
                project_capability_index_ref: ".punk/project/capabilities.json".into(),
                project_capability_index_sha256: "idx-sha".into(),
                selected_capabilities: Vec::new(),
                ignore_rules: Vec::new(),
                scope_seeds: punk_domain::CapabilityScopeSeeds::default(),
                target_checks: vec!["go test ./...".into()],
                integrity_checks: vec!["go test ./...".into()],
                controller_scaffold_kind: Some("go-mod".into()),
                generated_at: "now".into(),
            }),
            stdout_path: root.join("stdout.log"),
            stderr_path: root.join("stderr.log"),
            executor_pid_path: root.join("executor.json"),
        })
        .unwrap();
        assert!(created.iter().any(|path| path == "go.mod"));
        assert!(created
            .iter()
            .any(|path| path.starts_with("cmd/") && path.ends_with("/main.go")));
        assert!(created
            .iter()
            .any(|path| path == "internal/bootstrap/bootstrap.go"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn materialize_controller_bootstrap_scaffold_uses_frozen_python_kind() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-python-bootstrap-materialize-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let created = materialize_controller_bootstrap_scaffold(&ExecuteInput {
            repo_root: root.clone(),
            contract: Contract {
                id: "ct_py".into(),
                feature_id: "feat_py".into(),
                version: 1,
                status: punk_domain::ContractStatus::Approved,
                prompt_source: "bootstrap initial Python package for pubpunk".into(),
                entry_points: vec!["pyproject.toml".into()],
                import_paths: vec![],
                expected_interfaces: vec!["initial Python scaffold".into()],
                behavior_requirements: vec!["bootstrap project".into()],
                allowed_scope: vec!["pyproject.toml".into(), "src".into(), "tests".into()],
                target_checks: vec!["pytest".into()],
                integrity_checks: vec!["pytest".into()],
                risk_level: "medium".into(),
                created_at: "now".into(),
                approved_at: Some("now".into()),
            },
            capability_resolution: Some(punk_domain::FrozenCapabilityResolution {
                schema: "specpunk/contract-capability-resolution/v1".into(),
                version: 1,
                contract_id: "ct_py".into(),
                project_capability_index_ref: ".punk/project/capabilities.json".into(),
                project_capability_index_sha256: "idx-sha".into(),
                selected_capabilities: Vec::new(),
                ignore_rules: Vec::new(),
                scope_seeds: punk_domain::CapabilityScopeSeeds::default(),
                target_checks: vec!["pytest".into()],
                integrity_checks: vec!["pytest".into()],
                controller_scaffold_kind: Some("python-pyproject-pytest".into()),
                generated_at: "now".into(),
            }),
            stdout_path: root.join("stdout.log"),
            stderr_path: root.join("stderr.log"),
            executor_pid_path: root.join("executor.json"),
        })
        .unwrap();
        assert!(created.iter().any(|path| path == "pyproject.toml"));
        assert!(created.iter().any(|path| path == "tests/test_bootstrap.py"));
        assert!(created
            .iter()
            .any(|path| path.starts_with("src/") && path.ends_with("/__init__.py")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn controller_pubpunk_init_recipe_matches_stable_canonical_slice() {
        let contract = Contract {
            id: "ct_followup".into(),
            feature_id: "feat_followup".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "implement pubpunk init command in crates/pubpunk-cli and crates/pubpunk-core with tests: when run, it creates the canonical .pubpunk skeleton and returns JSON for --json; keep cargo test --workspace green".into(),
            entry_points: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec![
                "CLI surface in crates/pubpunk-cli exposes init and forwards execution into core logic.".into(),
                "Core library in crates/pubpunk-core provides the init/skeleton creation behavior and result data consumable by CLI JSON output.".into(),
                "Tests validate filesystem effects and JSON-facing behavior without expanding scope beyond init.".into(),
            ],
            behavior_requirements: vec![
                "Add a pubpunk init command wired through crates/pubpunk-cli and crates/pubpunk-core.".into(),
                "When invoked, create the canonical .pubpunk skeleton conservatively and idempotently.".into(),
                "Support --json output for init with machine-readable success data.".into(),
                "Add or update tests covering skeleton creation and JSON behavior.".into(),
            ],
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

        assert!(is_controller_pubpunk_init_recipe(&contract));
    }

    #[test]
    fn controller_pubpunk_init_recipe_does_not_match_incremental_json_tweak() {
        let contract = Contract {
            id: "ct_followup_incremental".into(),
            feature_id: "feat_followup_incremental".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source:
                "add a version field to pubpunk init --json without changing filesystem behavior"
                    .into(),
            entry_points: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec![
                "CLI supports `pubpunk init --json`.".into(),
                "JSON output includes a version field.".into(),
            ],
            behavior_requirements: vec![
                "Add a version field to init JSON output.".into(),
                "Do not rewrite the canonical starter files.".into(),
            ],
            allowed_scope: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests".into(),
            ],
            target_checks: vec!["cargo test -p pubpunk-cli".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert!(!is_controller_pubpunk_init_recipe(&contract));
    }

    #[test]
    fn controller_pubpunk_init_recipe_materializes_compile_ready_workspace() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-controller-pubpunk-init-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let bootstrap = Contract {
            id: "ct_bootstrap".into(),
            feature_id: "feat_bootstrap".into(),
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
        materialize_rust_workspace_bootstrap_scaffold(&root, &bootstrap).unwrap();

        let contract = Contract {
            id: "ct_pubpunk_init".into(),
            feature_id: "feat_pubpunk_init".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "implement pubpunk init command in crates/pubpunk-cli and crates/pubpunk-core with tests: when run, it creates the canonical .pubpunk skeleton and returns JSON for --json; keep cargo test --workspace green".into(),
            entry_points: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-cli/Cargo.toml".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec![
                "CLI surface in crates/pubpunk-cli exposes init and forwards execution into core logic.".into(),
                "Core library in crates/pubpunk-core provides the init/skeleton creation behavior and result data consumable by CLI JSON output.".into(),
                "Tests validate filesystem effects and JSON-facing behavior without expanding scope beyond init.".into(),
            ],
            behavior_requirements: vec![
                "Add a pubpunk init command wired through crates/pubpunk-cli and crates/pubpunk-core.".into(),
                "When invoked, create the canonical .pubpunk skeleton conservatively and idempotently.".into(),
                "Support --json output for init with machine-readable success data.".into(),
                "Keep cargo test --workspace green.".into(),
                "Add or update tests covering skeleton creation and JSON behavior.".into(),
            ],
            allowed_scope: vec![
                "crates/pubpunk-cli".into(),
                "crates/pubpunk-core".into(),
                "tests".into(),
            ],
            target_checks: vec!["cargo test -p pubpunk-cli".into(), "cargo test -p pubpunk-core".into(), "cargo test --tests".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let updated = apply_controller_pubpunk_init_recipe(&root, &contract)
            .unwrap()
            .expect("controller recipe should apply");
        assert!(updated.contains(&"crates/pubpunk-core/src/lib.rs".to_string()));
        assert!(updated.contains(&"crates/pubpunk-cli/src/main.rs".to_string()));
        assert!(updated.contains(&"tests/init_json.rs".to_string()));

        let status = Command::new("cargo")
            .args(["test", "--workspace"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(status.success());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn controller_pubpunk_cleanup_recipe_removes_style_examples_references() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-controller-pubpunk-cleanup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/pubpunk-core/src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            render_pubpunk_init_core_source(),
        )
        .unwrap();
        fs::write(
            root.join("tests/init_json.rs"),
            render_pubpunk_init_tests_source(),
        )
        .unwrap();

        let contract = Contract {
            id: "ct_cleanup".into(),
            feature_id: "feat_cleanup".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "remove style/examples cleanup".into(),
            entry_points: vec![
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/init_json.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec![
                "cleanup slice".into(),
                "remove style/examples references from core and tests".into(),
            ],
            behavior_requirements: vec![
                "remove obsolete style examples references".into(),
                "keep cargo test --workspace green".into(),
            ],
            allowed_scope: vec![
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/init_json.rs".into(),
            ],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert!(is_controller_pubpunk_cleanup_recipe(&contract));

        let updated = apply_controller_pubpunk_cleanup_recipe(&root, &contract)
            .unwrap()
            .expect("controller cleanup recipe should apply");
        assert!(updated.contains(&"crates/pubpunk-core/src/lib.rs".to_string()));
        assert!(updated.contains(&"tests/init_json.rs".to_string()));

        let core = fs::read_to_string(root.join("crates/pubpunk-core/src/lib.rs")).unwrap();
        let tests = fs::read_to_string(root.join("tests/init_json.rs")).unwrap();
        assert!(!core.contains(".pubpunk/style/examples"));
        assert!(!tests.contains(".pubpunk/style/examples"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn controller_pubpunk_validate_recipe_matches_exact_slice() {
        let contract = Contract {
            id: "ct_validate".into(),
            feature_id: "feat_validate".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "draft a bounded validate-only rust change. Entry points must be exactly crates/pubpunk-cli/src/main.rs, crates/pubpunk-core/src/lib.rs, and tests/validate_json.rs. Allowed scope should cover only those surfaces and the tests directory if needed. No Cargo.toml, no local/, no init work. Implement pubpunk validate with --json and --project-root, structured JSON envelope.".into(),
            entry_points: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/validate_json.rs".into(),
            ],
            import_paths: vec!["crates/pubpunk-core/src/lib.rs".into()],
            expected_interfaces: vec![
                "`pubpunk validate --json --project-root <path>` is exposed from the CLI.".into(),
                "Core library exposes validation logic consumable by the CLI.".into(),
                "JSON output uses a stable structured envelope suitable for tests.".into(),
            ],
            behavior_requirements: vec![
                "Add a validate-only Rust change for `pubpunk validate`.".into(),
                "Support `--json` and `--project-root`.".into(),
                "Do not add init behavior or init-side effects.".into(),
            ],
            allowed_scope: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/validate_json.rs".into(),
                "tests".into(),
                "Cargo.toml".into(),
            ],
            target_checks: vec!["cargo test --tests".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert!(is_controller_pubpunk_validate_recipe(&contract));
    }

    #[test]
    fn controller_pubpunk_validate_recipe_does_not_match_wording_tweak() {
        let contract = Contract {
            id: "ct_validate_wording".into(),
            feature_id: "feat_validate_wording".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source:
                "change pubpunk validate --json wording for project-root errors in the CLI".into(),
            entry_points: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec![
                "`pubpunk validate --json --project-root <path>` is exposed from the CLI.".into(),
                "Error wording should be clearer.".into(),
            ],
            behavior_requirements: vec![
                "Change only the JSON wording for validate failures.".into(),
                "Do not add new validation rules or new tests.".into(),
            ],
            allowed_scope: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests".into(),
            ],
            target_checks: vec!["cargo test -p pubpunk-cli".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert!(!is_controller_pubpunk_validate_recipe(&contract));
    }

    #[test]
    fn controller_pubpunk_validate_recipe_materializes_compile_ready_workspace() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-controller-pubpunk-validate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let bootstrap = Contract {
            id: "ct_bootstrap".into(),
            feature_id: "feat_bootstrap".into(),
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
        materialize_rust_workspace_bootstrap_scaffold(&root, &bootstrap).unwrap();

        let init_contract = Contract {
            id: "ct_pubpunk_init".into(),
            feature_id: "feat_pubpunk_init".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "implement pubpunk init command in crates/pubpunk-cli and crates/pubpunk-core with tests: when run, it creates the canonical .pubpunk skeleton and returns JSON for --json; keep cargo test --workspace green".into(),
            entry_points: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec![
                "CLI surface in crates/pubpunk-cli exposes init and forwards execution into core logic.".into(),
                "Core library in crates/pubpunk-core provides the init/skeleton creation behavior and result data consumable by CLI JSON output.".into(),
                "Tests validate filesystem effects and JSON-facing behavior without expanding scope beyond init.".into(),
            ],
            behavior_requirements: vec![
                "Add a pubpunk init command wired through crates/pubpunk-cli and crates/pubpunk-core.".into(),
                "When invoked, create the canonical .pubpunk skeleton conservatively and idempotently.".into(),
                "Support --json output for init with machine-readable success data.".into(),
                "Add or update tests covering skeleton creation and JSON behavior.".into(),
            ],
            allowed_scope: vec![
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
        apply_controller_pubpunk_init_recipe(&root, &init_contract)
            .unwrap()
            .expect("controller init recipe should apply");

        let cleanup_contract = Contract {
            id: "ct_cleanup".into(),
            feature_id: "feat_cleanup".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "remove style examples cleanup".into(),
            entry_points: vec![
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/init_json.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["cleanup slice".into()],
            behavior_requirements: vec!["remove style examples".into()],
            allowed_scope: vec![
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/init_json.rs".into(),
            ],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        apply_controller_pubpunk_cleanup_recipe(&root, &cleanup_contract)
            .unwrap()
            .expect("controller cleanup recipe should apply");

        let validate_contract = Contract {
            id: "ct_validate".into(),
            feature_id: "feat_validate".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "draft a bounded validate-only rust change. Implement pubpunk validate with --json and --project-root, structured JSON envelope, and checks for required .pubpunk tree/files, parseable project.toml, exact schema_version pubpunk.project.v1, slug-safe project_id, no absolute paths in paths.*, local.* under local/, and obvious secret-like keys token/secret/password/api_key.".into(),
            entry_points: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/validate_json.rs".into(),
            ],
            import_paths: vec!["crates/pubpunk-core/src/lib.rs".into()],
            expected_interfaces: vec![
                "`pubpunk validate --json --project-root <path>` is exposed from the CLI.".into(),
                "Core library exposes validation logic consumable by the CLI.".into(),
                "JSON output uses a stable structured envelope suitable for tests.".into(),
            ],
            behavior_requirements: vec![
                "Add a validate-only Rust change for `pubpunk validate`.".into(),
                "Support `--json` and `--project-root`.".into(),
                "Do not add init behavior or init-side effects.".into(),
            ],
            allowed_scope: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/validate_json.rs".into(),
                "tests".into(),
            ],
            target_checks: vec![
                "cargo test -p pubpunk-cli".into(),
                "cargo test -p pubpunk-core".into(),
                "cargo test --tests".into(),
            ],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let updated = apply_controller_pubpunk_validate_recipe(&root, &validate_contract)
            .unwrap()
            .expect("controller validate recipe should apply");
        assert!(updated.contains(&"crates/pubpunk-core/src/lib.rs".to_string()));
        assert!(updated.contains(&"crates/pubpunk-cli/src/main.rs".to_string()));
        assert!(updated.contains(&"tests/validate_json.rs".to_string()));

        let status = Command::new("cargo")
            .args(["test", "--workspace"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(status.success());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn controller_pubpunk_validate_parseability_recipe_matches_exact_slice() {
        let contract = Contract {
            id: "ct_validate_parseability".into(),
            feature_id: "feat_validate_parseability".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "Core-only validate parseability helper slice for pubpunk. Goal: keep validate_report JSON envelope unchanged while extending TOML surface parseability review so invalid style/targets/review/lint inputs become explicit issue strings instead of silent omissions. Edit only crates/pubpunk-core/src/lib.rs. Do not touch CLI, tests, Cargo.toml, local/, or init files.".into(),
            entry_points: vec!["crates/pubpunk-core/src/lib.rs".into()],
            import_paths: vec!["crates/pubpunk-core/src/lib.rs".into()],
            expected_interfaces: vec![
                "validate_report JSON envelope stays unchanged.".into(),
                "Invalid style/targets/review/lint inputs become explicit issue strings.".into(),
            ],
            behavior_requirements: vec![
                "Edit only crates/pubpunk-core/src/lib.rs.".into(),
                "Do not touch CLI, tests, Cargo.toml, local/, or init files.".into(),
            ],
            allowed_scope: vec!["crates/pubpunk-core/src/lib.rs".into()],
            target_checks: vec!["cargo test -p pubpunk-core".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert!(is_controller_pubpunk_validate_parseability_recipe(
            &contract
        ));
    }

    #[test]
    fn controller_pubpunk_validate_parseability_recipe_does_not_match_cli_wording_slice() {
        let contract = Contract {
            id: "ct_validate_parseability_wording".into(),
            feature_id: "feat_validate_parseability_wording".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "Change validate_report wording for invalid style config in pubpunk CLI"
                .into(),
            entry_points: vec!["crates/pubpunk-core/src/lib.rs".into()],
            import_paths: vec!["crates/pubpunk-core/src/lib.rs".into()],
            expected_interfaces: vec!["CLI wording stays more descriptive.".into()],
            behavior_requirements: vec!["Do not add new validation behavior.".into()],
            allowed_scope: vec!["crates/pubpunk-core/src/lib.rs".into()],
            target_checks: vec!["cargo test -p pubpunk-core".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert!(!is_controller_pubpunk_validate_parseability_recipe(
            &contract
        ));
    }

    #[test]
    fn controller_pubpunk_validate_parseability_recipe_materializes_compile_ready_workspace() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-controller-pubpunk-validate-parseability-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let bootstrap = Contract {
            id: "ct_bootstrap".into(),
            feature_id: "feat_bootstrap".into(),
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
        materialize_rust_workspace_bootstrap_scaffold(&root, &bootstrap).unwrap();

        let init_contract = Contract {
            id: "ct_pubpunk_init".into(),
            feature_id: "feat_pubpunk_init".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "implement pubpunk init command in crates/pubpunk-cli and crates/pubpunk-core with tests: when run, it creates the canonical .pubpunk skeleton and returns JSON for --json; keep cargo test --workspace green".into(),
            entry_points: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec![
                "CLI surface in crates/pubpunk-cli exposes init and forwards execution into core logic.".into(),
                "Core library in crates/pubpunk-core provides the init/skeleton creation behavior and result data consumable by CLI JSON output.".into(),
                "Tests validate filesystem effects and JSON-facing behavior without expanding scope beyond init.".into(),
            ],
            behavior_requirements: vec![
                "Add a pubpunk init command wired through crates/pubpunk-cli and crates/pubpunk-core.".into(),
                "When invoked, create the canonical .pubpunk skeleton conservatively and idempotently.".into(),
                "Support --json output for init with machine-readable success data.".into(),
                "Add or update tests covering skeleton creation and JSON behavior.".into(),
            ],
            allowed_scope: vec![
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
        apply_controller_pubpunk_init_recipe(&root, &init_contract)
            .unwrap()
            .expect("controller init recipe should apply");

        let cleanup_contract = Contract {
            id: "ct_cleanup".into(),
            feature_id: "feat_cleanup".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "remove style examples cleanup".into(),
            entry_points: vec![
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/init_json.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["cleanup slice".into()],
            behavior_requirements: vec!["remove style examples".into()],
            allowed_scope: vec![
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/init_json.rs".into(),
            ],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        apply_controller_pubpunk_cleanup_recipe(&root, &cleanup_contract)
            .unwrap()
            .expect("controller cleanup recipe should apply");

        let validate_contract = Contract {
            id: "ct_validate".into(),
            feature_id: "feat_validate".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "draft a bounded validate-only rust change. Implement pubpunk validate with --json and --project-root, structured JSON envelope, and checks for required .pubpunk tree/files, parseable project.toml, exact schema_version pubpunk.project.v1, slug-safe project_id, no absolute paths in paths.*, local.* under local/, and obvious secret-like keys token/secret/password/api_key.".into(),
            entry_points: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/validate_json.rs".into(),
            ],
            import_paths: vec!["crates/pubpunk-core/src/lib.rs".into()],
            expected_interfaces: vec![
                "`pubpunk validate --json --project-root <path>` is exposed from the CLI.".into(),
                "Core library exposes validation logic consumable by the CLI.".into(),
                "JSON output uses a stable structured envelope suitable for tests.".into(),
            ],
            behavior_requirements: vec![
                "Add a validate-only Rust change for `pubpunk validate`.".into(),
                "Support `--json` and `--project-root`.".into(),
                "Do not add init behavior or init-side effects.".into(),
            ],
            allowed_scope: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/validate_json.rs".into(),
                "tests".into(),
            ],
            target_checks: vec![
                "cargo test -p pubpunk-cli".into(),
                "cargo test -p pubpunk-core".into(),
                "cargo test --tests".into(),
            ],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        apply_controller_pubpunk_validate_recipe(&root, &validate_contract)
            .unwrap()
            .expect("controller validate recipe should apply");

        let parseability_contract = Contract {
            id: "ct_validate_parseability".into(),
            feature_id: "feat_validate_parseability".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "Core-only validate parseability helper slice for pubpunk. Goal: keep validate_report JSON envelope unchanged while extending TOML surface parseability review so invalid style/targets/review/lint inputs become explicit issue strings instead of silent omissions. Edit only crates/pubpunk-core/src/lib.rs. Do not touch CLI, tests, Cargo.toml, local/, or init files.".into(),
            entry_points: vec!["crates/pubpunk-core/src/lib.rs".into()],
            import_paths: vec!["crates/pubpunk-core/src/lib.rs".into()],
            expected_interfaces: vec![
                "validate_report JSON envelope stays unchanged.".into(),
                "Invalid style/targets/review/lint inputs become explicit issue strings.".into(),
            ],
            behavior_requirements: vec![
                "Edit only crates/pubpunk-core/src/lib.rs.".into(),
                "Do not touch CLI, tests, Cargo.toml, local/, or init files.".into(),
            ],
            allowed_scope: vec!["crates/pubpunk-core/src/lib.rs".into()],
            target_checks: vec!["cargo test -p pubpunk-core".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let updated =
            apply_controller_pubpunk_validate_parseability_recipe(&root, &parseability_contract)
                .unwrap()
                .expect("controller validate parseability recipe should apply");
        assert_eq!(updated, vec!["crates/pubpunk-core/src/lib.rs".to_string()]);

        let status = Command::new("cargo")
            .args(["test", "--workspace"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(status.success());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn controller_pubpunk_validate_file_parseability_recipe_matches_exact_slice() {
        let contract = Contract {
            id: "ct_validate_file_parseability".into(),
            feature_id: "feat_validate_file_parseability".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "bounded core-only validate parse-check extension. Edit only crates/pubpunk-core/src/lib.rs and tests/validate_json.rs. Do not touch CLI or Cargo. Reuse the existing simple TOML parser so validate_report also checks parseability of .pubpunk/style/style.toml, .pubpunk/style/lexicon.toml, .pubpunk/style/normalize.toml, plus any *.toml files present under .pubpunk/targets, .pubpunk/review, and .pubpunk/lint. Add tests that corrupt style.toml and a target .toml file and expect structured validate JSON issues. Final check cargo test --workspace.".into(),
            entry_points: vec![
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/validate_json.rs".into(),
            ],
            import_paths: vec!["crates/pubpunk-core/src/lib.rs".into()],
            expected_interfaces: vec![
                "validate_report keeps the structured JSON envelope unchanged.".into(),
                "style.toml and target .toml parseability issues become explicit.".into(),
            ],
            behavior_requirements: vec![
                "Edit only crates/pubpunk-core/src/lib.rs and tests/validate_json.rs.".into(),
                "Do not touch CLI or Cargo.".into(),
            ],
            allowed_scope: vec![
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/validate_json.rs".into(),
            ],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert!(is_controller_pubpunk_validate_file_parseability_recipe(
            &contract
        ));
    }

    #[test]
    fn controller_pubpunk_validate_file_parseability_recipe_materializes_compile_ready_workspace() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-controller-pubpunk-validate-file-parseability-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let bootstrap = Contract {
            id: "ct_bootstrap".into(),
            feature_id: "feat_bootstrap".into(),
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
        materialize_rust_workspace_bootstrap_scaffold(&root, &bootstrap).unwrap();

        let init_contract = Contract {
            id: "ct_pubpunk_init".into(),
            feature_id: "feat_pubpunk_init".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "implement pubpunk init command in crates/pubpunk-cli and crates/pubpunk-core with tests: when run, it creates the canonical .pubpunk skeleton and returns JSON for --json; keep cargo test --workspace green".into(),
            entry_points: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec![
                "CLI surface in crates/pubpunk-cli exposes init and forwards execution into core logic.".into(),
                "Core library in crates/pubpunk-core provides the init/skeleton creation behavior and result data consumable by CLI JSON output.".into(),
                "Tests validate filesystem effects and JSON-facing behavior without expanding scope beyond init.".into(),
            ],
            behavior_requirements: vec![
                "Add a pubpunk init command wired through crates/pubpunk-cli and crates/pubpunk-core.".into(),
                "When invoked, create the canonical .pubpunk skeleton conservatively and idempotently.".into(),
                "Support --json output for init with machine-readable success data.".into(),
                "Add or update tests covering skeleton creation and JSON behavior.".into(),
            ],
            allowed_scope: vec![
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
        apply_controller_pubpunk_init_recipe(&root, &init_contract)
            .unwrap()
            .expect("controller init recipe should apply");

        let cleanup_contract = Contract {
            id: "ct_cleanup".into(),
            feature_id: "feat_cleanup".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "remove style examples cleanup".into(),
            entry_points: vec![
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/init_json.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["cleanup slice".into()],
            behavior_requirements: vec!["remove style examples".into()],
            allowed_scope: vec![
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/init_json.rs".into(),
            ],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        apply_controller_pubpunk_cleanup_recipe(&root, &cleanup_contract)
            .unwrap()
            .expect("controller cleanup recipe should apply");

        let validate_contract = Contract {
            id: "ct_validate".into(),
            feature_id: "feat_validate".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "draft a bounded validate-only rust change. Implement pubpunk validate with --json and --project-root, structured JSON envelope, and checks for required .pubpunk tree/files, parseable project.toml, exact schema_version pubpunk.project.v1, slug-safe project_id, no absolute paths in paths.*, local.* under local/, and obvious secret-like keys token/secret/password/api_key.".into(),
            entry_points: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/validate_json.rs".into(),
            ],
            import_paths: vec!["crates/pubpunk-core/src/lib.rs".into()],
            expected_interfaces: vec![
                "`pubpunk validate --json --project-root <path>` is exposed from the CLI.".into(),
                "Core library exposes validation logic consumable by the CLI.".into(),
                "JSON output uses a stable structured envelope suitable for tests.".into(),
            ],
            behavior_requirements: vec![
                "Add a validate-only Rust change for `pubpunk validate`.".into(),
                "Support `--json` and `--project-root`.".into(),
                "Do not add init behavior or init-side effects.".into(),
            ],
            allowed_scope: vec![
                "crates/pubpunk-cli/src/main.rs".into(),
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/validate_json.rs".into(),
                "tests".into(),
            ],
            target_checks: vec![
                "cargo test -p pubpunk-cli".into(),
                "cargo test -p pubpunk-core".into(),
                "cargo test --tests".into(),
            ],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };
        apply_controller_pubpunk_validate_recipe(&root, &validate_contract)
            .unwrap()
            .expect("controller validate recipe should apply");

        let parseability_contract = Contract {
            id: "ct_validate_file_parseability".into(),
            feature_id: "feat_validate_file_parseability".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "bounded core-only validate parse-check extension. Edit only crates/pubpunk-core/src/lib.rs and tests/validate_json.rs. Do not touch CLI or Cargo. Reuse the existing simple TOML parser so validate_report also checks parseability of .pubpunk/style/style.toml, .pubpunk/style/lexicon.toml, .pubpunk/style/normalize.toml, plus any *.toml files present under .pubpunk/targets, .pubpunk/review, and .pubpunk/lint. Add tests that corrupt style.toml and a target .toml file and expect structured validate JSON issues. Final check cargo test --workspace.".into(),
            entry_points: vec![
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/validate_json.rs".into(),
            ],
            import_paths: vec!["crates/pubpunk-core/src/lib.rs".into()],
            expected_interfaces: vec![
                "validate_report keeps the structured JSON envelope unchanged.".into(),
                "style.toml and target .toml parseability issues become explicit.".into(),
            ],
            behavior_requirements: vec![
                "Edit only crates/pubpunk-core/src/lib.rs and tests/validate_json.rs.".into(),
                "Do not touch CLI or Cargo.".into(),
            ],
            allowed_scope: vec![
                "crates/pubpunk-core/src/lib.rs".into(),
                "tests/validate_json.rs".into(),
            ],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let updated = apply_controller_pubpunk_validate_file_parseability_recipe(
            &root,
            &parseability_contract,
        )
        .unwrap()
        .expect("controller validate file parseability recipe should apply");
        assert!(updated.contains(&"crates/pubpunk-core/src/lib.rs".to_string()));
        assert!(updated.contains(&"tests/validate_json.rs".to_string()));

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
    fn materialize_rust_workspace_bootstrap_scaffold_ignores_article_cli_phrase_when_inferring_slug(
    ) {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-greenfield-bootstrap-article-cli-{}-{}",
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
            prompt_source:
                "scaffold Rust workspace and implement pubpunk init command with --json output and tests"
                    .into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec![],
            expected_interfaces: vec![
                "A Rust workspace manifest at Cargo.toml.".into(),
                "A CLI binary exposing pubpunk init.".into(),
                "A --json output mode for pubpunk init.".into(),
                "Workspace tests validating init behavior and JSON output.".into(),
            ],
            behavior_requirements: vec![
                "Scaffold a minimal Rust workspace rooted at Cargo.toml for the pubpunk repository."
                    .into(),
                "Implement a pubpunk init command conservatively, keeping scope limited to workspace bootstrap and init behavior only."
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
        assert!(created
            .iter()
            .any(|path| path == "crates/pubpunk-cli/Cargo.toml"));
        assert!(created
            .iter()
            .any(|path| path == "crates/pubpunk-core/Cargo.toml"));
        assert!(!created.iter().any(|path| path.contains("crates/a-cli")));
        assert!(!created.iter().any(|path| path.contains("crates/a-core")));

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
    fn effective_execution_contract_expands_directory_scope_to_existing_files() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-effective-contract-{}-{}",
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
        fs::write(root.join("crates/pubpunk-cli/Cargo.toml"), "[package]\n").unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(root.join("crates/pubpunk-core/Cargo.toml"), "[package]\n").unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            "pub fn init() {}\n",
        )
        .unwrap();
        fs::write(root.join("tests/init_json.rs"), "#[test]\nfn smoke() {}\n").unwrap();

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

        let effective = effective_execution_contract(&root, &contract).unwrap();
        assert!(is_bounded_execution_task(&effective));
        assert_eq!(
            effective.allowed_scope,
            vec![
                "crates/pubpunk-cli/Cargo.toml".to_string(),
                "crates/pubpunk-cli/src/main.rs".to_string(),
                "crates/pubpunk-core/Cargo.toml".to_string(),
                "crates/pubpunk-core/src/lib.rs".to_string(),
                "tests/init_json.rs".to_string(),
            ]
        );
        assert!(effective
            .entry_points
            .contains(&"crates/pubpunk-cli/src/main.rs".to_string()));
        assert!(effective
            .entry_points
            .contains(&"crates/pubpunk-core/src/lib.rs".to_string()));
        assert!(effective
            .entry_points
            .contains(&"tests/init_json.rs".to_string()));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn effective_execution_contract_synthesizes_missing_test_entrypoint_when_only_placeholder_exists(
    ) {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-effective-contract-preserve-tests-{}-{}",
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
        fs::write(root.join("crates/pubpunk-cli/Cargo.toml"), "[package]\n").unwrap();
        fs::write(
            root.join("crates/pubpunk-cli/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(root.join("crates/pubpunk-core/Cargo.toml"), "[package]\n").unwrap();
        fs::write(
            root.join("crates/pubpunk-core/src/lib.rs"),
            "pub fn init() {}\n",
        )
        .unwrap();
        fs::write(root.join("tests/README.md"), "# tests\n").unwrap();

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

        let effective = effective_execution_contract(&root, &contract).unwrap();
        assert!(effective
            .allowed_scope
            .contains(&"tests/init.rs".to_string()));
        assert!(effective
            .entry_points
            .contains(&"tests/init.rs".to_string()));
        assert!(!effective
            .allowed_scope
            .contains(&"tests/README.md".to_string()));
        assert!(!effective.allowed_scope.iter().any(|path| path == "tests"));
        assert!(is_bounded_execution_task(&effective));
        assert_eq!(
            execution_lane_for_contract(&root, &effective),
            ExecutionLane::PatchApply
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn synthesized_test_entrypoint_prefers_json_named_test_when_contract_mentions_json() {
        let contract = Contract {
            id: "ct_json_test".into(),
            feature_id: "feat_json_test".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "implement pubpunk init".into(),
            entry_points: vec!["crates/pubpunk-cli/src/main.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["pubpunk init --json".into()],
            behavior_requirements: vec!["add --json behavior".into()],
            allowed_scope: vec!["crates/pubpunk-cli/src/main.rs".into(), "tests".into()],
            target_checks: vec!["cargo test -p pubpunk-cli".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        assert_eq!(synthesized_test_entrypoint(&contract), "tests/init_json.rs");
    }

    #[test]
    fn effective_execution_contract_preserves_missing_bootstrap_scaffold_scope() {
        let root = std::env::temp_dir().join(format!(
            "punk-adapters-effective-bootstrap-scope-{}-{}",
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

        let effective = effective_execution_contract(&root, &contract).unwrap();
        assert_eq!(effective.allowed_scope, contract.allowed_scope);
        assert_eq!(effective.entry_points, contract.entry_points);

        let _ = fs::remove_dir_all(&root);
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
