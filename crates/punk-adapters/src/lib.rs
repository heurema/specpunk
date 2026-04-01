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
    build_context_pack, ensure_retry_patch_seed, format_context_pack,
    materialize_missing_entry_points, restore_missing_materialized_entry_points,
    restore_stale_entry_point_masks, scaffold_only_entry_points, ContextPack,
    EntryPointExcerptGuard,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryPointSnapshot {
    path: String,
    content: String,
}

struct AttemptOutcome {
    timed_output: TimedOutput,
    restored_paths: Vec<String>,
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

impl Executor for CodexCliExecutor {
    fn name(&self) -> &'static str {
        "codex-cli"
    }

    fn execute_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput> {
        if let Some(summary) = manual_mode_block_summary(&input.contract) {
            return Ok(ExecuteOutput {
                success: false,
                summary,
                checks_run: Vec::new(),
                cost_usd: None,
                duration_ms: 0,
            });
        }
        let start = Instant::now();
        restore_stale_entry_point_masks(&input.repo_root)?;
        let created_entry_points = if is_fail_closed_scope_task(&input.contract) {
            materialize_missing_entry_points(&input.repo_root, &input.contract)?
        } else {
            Vec::new()
        };
        let mut attempt = self.run_execution_attempt(&input, &created_entry_points, false)?;
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
            attempt = self.run_execution_attempt(&input, &created_entry_points, true)?;
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
        let timed_output = attempt.timed_output;
        fs::write(&input.stdout_path, &timed_output.output.stdout)?;
        fs::write(&input.stderr_path, &timed_output.output.stderr)?;
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
        } else if timed_output.timed_out {
            classify_timeout_result(&stdout, &stderr, codex_executor_timeout())
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

        let result = match self.run_json_prompt_once(prompt, repo_root, &schema_path, &output_path)
        {
            Ok(proposal) => Ok(proposal),
            Err(DrafterAttemptError::TimedOut { stdout, stderr }) => {
                if let Some(retry_prompt) = retry_prompt {
                    match self.run_json_prompt_once(
                        &retry_prompt,
                        repo_root,
                        &schema_path,
                        &output_path,
                    ) {
                        Ok(proposal) => Ok(proposal),
                        Err(DrafterAttemptError::TimedOut { stdout, stderr }) => Err(anyhow!(
                            timeout_summary(codex_drafter_timeout(), &stdout, &stderr)
                        )),
                        Err(DrafterAttemptError::Failed(err)) => Err(err),
                    }
                } else {
                    Err(anyhow!(timeout_summary(
                        codex_drafter_timeout(),
                        &stdout,
                        &stderr
                    )))
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
        command.arg(prompt);

        let output =
            run_command_with_timeout(&mut command, codex_drafter_timeout()).map_err(|err| {
                DrafterAttemptError::Failed(
                    err.context(format!("spawn codex drafter in {repo_root}")),
                )
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

impl CodexCliExecutor {
    fn run_execution_attempt(
        &self,
        input: &ExecuteInput,
        created_entry_points: &[String],
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
        let entry_point_snapshots = if is_fail_closed_scope_task(&input.contract) {
            capture_entry_point_snapshots(&input.repo_root, &input.contract)?
        } else {
            Vec::new()
        };

        let prompt = build_exec_prompt_with_mode(
            &input.contract,
            context_pack.as_ref(),
            created_entry_points,
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
        command.arg(prompt);
        let timed_output = match run_command_with_timeout_and_tee(
            &mut command,
            codex_executor_timeout(),
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
                if let Some(guard) = excerpt_guard.as_mut() {
                    guard.restore()?;
                }
                output
            }
            Err(err) => {
                if let Some(guard) = excerpt_guard.as_mut() {
                    let _ = guard.restore();
                }
                let _ = restore_missing_materialized_entry_points(
                    &input.repo_root,
                    &input.contract,
                    created_entry_points,
                );
                return Err(err);
            }
        };

        let restored_paths = restore_missing_materialized_entry_points(
            &input.repo_root,
            &input.contract,
            created_entry_points,
        )?;
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
    created_entry_points: &[String],
) -> String {
    build_exec_prompt_with_mode(contract, context_pack, created_entry_points, false)
}

fn build_exec_prompt_with_mode(
    contract: &Contract,
    context_pack: Option<&ContextPack>,
    created_entry_points: &[String],
    retry_mode: bool,
) -> String {
    let scope_rule = fail_closed_scope_rule(contract);
    let meta_workflow_rule = forbid_meta_workflow_rule();
    let vcs_restore_rule = forbid_vcs_restore_rule();
    let created_entry_points_section = if created_entry_points.is_empty() {
        String::new()
    } else {
        format!(
            "The following entry-point files were materialized for this run and must remain present: {}. Edit those paths in place and do not delete or rename them.\n",
            created_entry_points.join(", ")
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

fn build_draft_prompt(input: &DraftInput) -> String {
    format!(
        "Draft an approve-ready contract proposal for the current repository.\n\
Return JSON only and match the provided schema exactly.\n\
Do not invent ids, timestamps, statuses, or event metadata.\n\
Use only repo-relative paths.\n\
Keep scope bounded and conservative.\n\
If the user prompt names exact file paths or exact shell commands, prefer those explicit user-provided details over weaker scan guesses.\n\
Prefer `allowed_scope` entries from `candidate_scope_paths`.\n\
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
        .unwrap_or(90);
    Duration::from_secs(seconds)
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

fn run_command_with_timeout(command: &mut Command, timeout: Duration) -> Result<TimedOutput> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return child
                .wait_with_output()
                .map(|output| TimedOutput {
                    output,
                    timed_out: false,
                    stalled: false,
                    orphaned: false,
                    no_progress_paths: Vec::new(),
                    scaffold_only_paths: Vec::new(),
                    post_check_zero_progress_paths: Vec::new(),
                })
                .map_err(Into::into);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let output = child.wait_with_output()?;
            return Ok(TimedOutput {
                output,
                timed_out: true,
                stalled: false,
                orphaned: false,
                no_progress_paths: Vec::new(),
                scaffold_only_paths: Vec::new(),
                post_check_zero_progress_paths: Vec::new(),
            });
        }
        thread::sleep(Duration::from_millis(200));
    }
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
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
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
            if progress.stalled_for(no_progress_timeout)
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
    let stdout = join_stream_tee(stdout_handle)?;
    let stderr = join_stream_tee(stderr_handle)?;
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

fn spawn_stream_tee<R>(
    mut reader: R,
    path: PathBuf,
    progress: Arc<ProgressTracker>,
    is_stdout: bool,
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
    let combined = format!("{stdout}\n{stderr}");
    combined.contains("test result: ok.")
        || combined.contains("Finished `test` profile")
        || combined.contains("Finished `dev` profile")
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
        || combined.contains("cargo build ")
        || combined.contains("cargo test ")
        || combined.contains("error[")
        || combined.contains("error:")
        || combined.contains(BLOCKED_EXECUTION_SENTINEL)
        || combined.contains(SUCCESSFUL_EXECUTION_SENTINEL)
}

fn logs_indicate_post_check_zero_progress_tail(
    stdout_path: &std::path::Path,
    stderr_path: &std::path::Path,
) -> bool {
    let stdout = read_tail_text(stdout_path, 1024).unwrap_or_default();
    let stderr = read_tail_text(stderr_path, 1024).unwrap_or_default();
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
) -> Result<Vec<EntryPointSnapshot>> {
    let mut snapshots = Vec::new();
    for entry_point in &contract.entry_points {
        if !entry_point.ends_with(".rs") {
            continue;
        }
        if !path_is_in_allowed_scope(entry_point, &contract.allowed_scope) {
            continue;
        }
        let file_path = repo_root.join(entry_point);
        if !file_path.exists() {
            continue;
        }
        snapshots.push(EntryPointSnapshot {
            path: entry_point.clone(),
            content: fs::read_to_string(&file_path)
                .with_context(|| format!("read entry point snapshot {entry_point}"))?,
        });
    }
    Ok(snapshots)
}

fn unchanged_entry_point_paths(
    repo_root: &std::path::Path,
    snapshots: &[EntryPointSnapshot],
) -> Result<Vec<String>> {
    let mut unchanged = Vec::new();
    for snapshot in snapshots {
        let file_path = repo_root.join(&snapshot.path);
        if !file_path.exists() {
            continue;
        }
        let current = fs::read_to_string(&file_path)
            .with_context(|| format!("read entry point progress probe {}", snapshot.path))?;
        if current == snapshot.content {
            unchanged.push(snapshot.path.clone());
        }
    }
    Ok(unchanged)
}

fn path_is_in_allowed_scope(path: &str, allowed_scope: &[String]) -> bool {
    allowed_scope.iter().any(|scope| {
        path == scope
            || path
                .strip_prefix(scope)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
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
            content: "pub fn unchanged() {}\n".into(),
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
            content: "pub fn unchanged() {}\n".into(),
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
