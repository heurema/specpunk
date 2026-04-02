use std::env;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand};
use punk_adapters::{CodexCliContractDrafter, CodexCliExecutor};
use punk_gate::GateService;
use punk_orch::OrchService;
use punk_proof::ProofService;
use punk_vcs::{
    current_snapshot_ref, detect_backend, detect_mode as detect_vcs_mode,
    enable_jj as enable_jj_for_repo, VcsMode,
};

#[derive(Parser)]
#[command(
    name = "punk",
    about = "Local-first, modal AI engineering CLI",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Start(StartCommand),
    Plot(PlotCommand),
    Cut(CutCommand),
    Gate(GateCommand),
    Status(StatusCommand),
    Inspect(InspectCommand),
    Vcs(VcsCommand),
}

#[derive(Args)]
struct StartCommand {
    goal: String,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct StatusCommand {
    id: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct InspectCommand {
    id: String,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct VcsCommand {
    #[command(subcommand)]
    action: VcsAction,
}

#[derive(Subcommand)]
enum VcsAction {
    Status,
    EnableJj,
}

#[derive(Args)]
struct PlotCommand {
    #[command(subcommand)]
    action: PlotAction,
}

#[derive(Subcommand)]
enum PlotAction {
    Contract {
        prompt: String,
        #[arg(long)]
        json: bool,
    },
    Refine {
        contract_id: String,
        guidance: String,
        #[arg(long)]
        json: bool,
    },
    Approve {
        contract_id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
struct CutCommand {
    #[command(subcommand)]
    action: CutAction,
}

#[derive(Subcommand)]
enum CutAction {
    Run {
        contract_id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
struct GateCommand {
    #[command(subcommand)]
    action: GateAction,
}

#[derive(Subcommand)]
enum GateAction {
    Run {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    Proof {
        run_or_decision_id: String,
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("punk: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let repo_root = env::current_dir()?;
    let global_root = default_global_root()?;
    let bootstrapped = maybe_auto_bootstrap_project(&repo_root, &cli.command)?;
    if !bootstrapped {
        maybe_warn_jj_degraded_mode(&repo_root, &cli.command);
    }

    match cli.command {
        Command::Start(start) => cmd_start(&repo_root, &global_root, &start.goal, start.json),
        Command::Plot(plot) => match plot.action {
            PlotAction::Contract { prompt, json } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let drafter = CodexCliContractDrafter::default();
                let contract = orch.draft_contract(&drafter, &prompt)?;
                render(json, &contract, &format!("drafted {}", contract.id))
            }
            PlotAction::Refine {
                contract_id,
                guidance,
                json,
            } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let drafter = CodexCliContractDrafter::default();
                let contract = orch.refine_contract(&drafter, &contract_id, &guidance)?;
                render(json, &contract, &format!("refined {}", contract.id))
            }
            PlotAction::Approve { contract_id, json } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let contract = orch.approve_contract(&contract_id)?;
                render(json, &contract, &format!("approved {}", contract.id))
            }
        },
        Command::Cut(cut) => match cut.action {
            CutAction::Run { contract_id, json } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let executor = CodexCliExecutor::default();
                let (run, receipt) = orch.cut_run(&executor, &contract_id)?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(
                            &serde_json::json!({"run": run, "receipt": receipt})
                        )?
                    );
                } else {
                    println!("{}", format_cut_run_summary(&run, &receipt));
                }
                Ok(())
            }
        },
        Command::Gate(gate) => match gate.action {
            GateAction::Run { run_id, json } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let service = GateService::new(&repo_root, &global_root);
                let decision = service.gate_run(&run_id)?;
                let status = orch.status(Some(&run_id))?;
                render(
                    json,
                    &decision,
                    &format_gate_run_summary(&decision, &status),
                )
            }
            GateAction::Proof {
                run_or_decision_id,
                json,
            } => {
                let service = ProofService::new(&repo_root, &global_root);
                let proof = service.write_proofpack(&run_or_decision_id)?;
                render(json, &proof, &format!("proof {}", proof.id))
            }
        },
        Command::Status(status) => {
            let orch = OrchService::new(&repo_root, &global_root)?;
            let snapshot = orch.status(status.id.as_deref())?;
            render(
                status.json,
                &snapshot,
                &format!(
                    "project={} events={} contract={:?} run={:?} decision={:?} vcs={:?} ref={:?} dirty={} workspace_root={:?}",
                    snapshot.project_id,
                    snapshot.events_count,
                    snapshot.last_contract_id,
                    snapshot.last_run_id,
                    snapshot.last_decision_id,
                    snapshot.vcs_backend,
                    snapshot.vcs_ref,
                    snapshot.vcs_dirty,
                    snapshot.workspace_root
                ),
            )
        }
        Command::Inspect(inspect) => {
            let orch = OrchService::new(&repo_root, &global_root)?;
            if !inspect.json {
                return Err(anyhow!("inspect currently requires --json"));
            }
            let value = orch.inspect(&inspect.id)?;
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
        Command::Vcs(vcs) => match vcs.action {
            VcsAction::Status => cmd_vcs_status(&repo_root),
            VcsAction::EnableJj => cmd_vcs_enable_jj(&repo_root),
        },
    }
}

fn render<T: serde::Serialize>(json: bool, value: &T, human: &str) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("{human}");
    }
    Ok(())
}

fn format_cut_run_summary(run: &punk_domain::Run, receipt: &punk_domain::Receipt) -> String {
    format!(
        "run {} ({})\nsummary: {}\nvcs: {:?} change={} base={:?}\nworkspace: {}",
        run.id,
        receipt.status,
        receipt.summary,
        run.vcs.backend,
        run.vcs.change_ref,
        run.vcs.base_ref,
        run.vcs.workspace_ref
    )
}

fn format_gate_run_summary(
    decision: &punk_domain::DecisionObject,
    status: &punk_orch::StatusSnapshot,
) -> String {
    format!(
        "decision {:?} for {} (vcs={:?} ref={:?} dirty={} workspace_root={:?})",
        decision.decision,
        decision.run_id,
        status.vcs_backend,
        status.vcs_ref,
        status.vcs_dirty,
        status.workspace_root
    )
}

fn default_global_root() -> Result<PathBuf> {
    let home = env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| anyhow!("HOME is not set"))?;
    Ok(home.join(".punk"))
}

fn maybe_auto_bootstrap_project(repo_root: &Path, command: &Command) -> Result<bool> {
    let Some(json) = bootstrap_json_mode(command) else {
        return Ok(false);
    };

    let project_root = resolve_project_root(repo_root);
    let Some(project_id) = infer_project_id(&project_root) else {
        return Ok(false);
    };
    if !needs_project_bootstrap(&project_root, &project_id) {
        return Ok(false);
    }

    match detect_punk_run_bootstrap_support(&project_root) {
        BootstrapSupport::Supported => run_project_bootstrap(&project_root, &project_id, json)?,
        BootstrapSupport::Unavailable(reason) | BootstrapSupport::Incompatible(reason) => {
            if !json {
                eprintln!("{}", format_bootstrap_skip_note(&project_id, &reason));
            }
            return Ok(false);
        }
    }
    Ok(true)
}

fn bootstrap_json_mode(command: &Command) -> Option<bool> {
    match command {
        Command::Plot(PlotCommand {
            action: PlotAction::Contract { json, .. },
        }) => Some(*json),
        Command::Start(StartCommand { json, .. }) => Some(*json),
        _ => None,
    }
}

fn resolve_project_root(repo_root: &Path) -> PathBuf {
    detect_backend(repo_root)
        .and_then(|backend| backend.workspace_root())
        .unwrap_or_else(|_| repo_root.to_path_buf())
}

fn infer_project_id(project_root: &Path) -> Option<String> {
    project_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn project_bootstrap_file_path(project_root: &Path, project_id: &str) -> PathBuf {
    project_root
        .join(".punk")
        .join("bootstrap")
        .join(format!("{project_id}-core.md"))
}

fn needs_project_bootstrap(project_root: &Path, project_id: &str) -> bool {
    !project_bootstrap_file_path(project_root, project_id).exists()
}

enum BootstrapSupport {
    Supported,
    Unavailable(String),
    Incompatible(String),
}

fn detect_punk_run_bootstrap_support(project_root: &Path) -> BootstrapSupport {
    let output = match ProcessCommand::new("punk-run")
        .current_dir(project_root)
        .arg("init")
        .arg("--help")
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            return BootstrapSupport::Unavailable(format!("punk-run not available in PATH: {err}"));
        }
    };

    let help = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() && help.contains("--project") && help.contains("--verify") {
        BootstrapSupport::Supported
    } else {
        BootstrapSupport::Incompatible(
            "compatible `punk-run init --project ... --verify` support not detected".to_string(),
        )
    }
}

fn run_project_bootstrap(project_root: &Path, project_id: &str, json: bool) -> Result<()> {
    let output = ProcessCommand::new("punk-run")
        .current_dir(project_root)
        .arg("init")
        .arg("--project")
        .arg(project_id)
        .arg("--verify")
        .output()
        .map_err(|err| {
            anyhow!(format_bootstrap_error(
                project_id,
                &format!("failed to execute punk-run: {err}")
            ))
        })?;

    if !json && !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }
    if !json && !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }

    if output.status.success() {
        return Ok(());
    }

    let reason = output
        .status
        .code()
        .map(|code| format!("punk-run init exited with code {code}"))
        .unwrap_or_else(|| "punk-run init terminated by signal".to_string());
    Err(anyhow!(format_bootstrap_error(project_id, &reason)))
}

fn format_bootstrap_error(project_id: &str, reason: &str) -> String {
    format!(
        "project bootstrap failed for `{project_id}`: {reason}. Run `punk-run init --project {project_id} --enable-jj --verify` manually and retry."
    )
}

fn format_bootstrap_skip_note(project_id: &str, reason: &str) -> String {
    format!(
        "Bootstrap note: skipping optional project bootstrap for `{project_id}` because {reason}. If you need full onboarding, run `punk-run init --project {project_id} --enable-jj --verify` manually."
    )
}

fn cmd_start(repo_root: &Path, global_root: &Path, goal: &str, json: bool) -> Result<()> {
    let trimmed_goal = goal.trim();
    if trimmed_goal.is_empty() {
        return Err(anyhow!("goal must not be empty"));
    }

    let orch = OrchService::new(repo_root, global_root)?;
    let drafter = CodexCliContractDrafter::default();
    let contract = orch.draft_contract(&drafter, trimmed_goal)?;
    let status = orch.status(None)?;
    let project_root = resolve_project_root(repo_root);
    let project = infer_project_id(&project_root).unwrap_or_else(|| status.project_id.clone());
    let next_command = format!("punk plot approve {}", contract.id);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "goal": trimmed_goal,
                "project": project,
                "project_id": status.project_id,
                "contract": contract,
                "next_command": next_command,
            }))?
        );
    } else {
        println!(
            "{}",
            format_start_summary(&project, trimmed_goal, &contract.id, &next_command)
        );
    }
    Ok(())
}

fn format_start_summary(
    project: &str,
    goal: &str,
    contract_id: &str,
    next_command: &str,
) -> String {
    format!(
        "Goal: {goal}\nProject: {project}\nDrafted contract: {contract_id}\nNext: {next_command}"
    )
}

fn maybe_warn_jj_degraded_mode(repo_root: &PathBuf, command: &Command) {
    if matches!(command, Command::Vcs(_)) {
        return;
    }
    if should_warn_about_disabled_jj(detect_vcs_mode(repo_root)) {
        eprintln!("{}", format_jj_degraded_mode_warning());
    }
}

fn should_warn_about_disabled_jj(mode: VcsMode) -> bool {
    mode == VcsMode::GitWithJjAvailableButDisabled
}

fn format_vcs_status(mode: VcsMode) -> &'static str {
    match mode {
        VcsMode::Jj => "VCS mode: jj",
        VcsMode::GitOnly => "VCS mode: git-only",
        VcsMode::GitWithJjAvailableButDisabled => {
            "VCS mode: git-only (degraded; run `punk vcs enable-jj`)"
        }
        VcsMode::NoVcs => "VCS mode: no VCS detected",
    }
}

fn render_vcs_status(mode: VcsMode) -> String {
    let mut status = format_vcs_status(mode).to_string();
    if mode == VcsMode::Jj {
        status.push_str(
            "\nNote: in a colocated jj repo, `git status` may show detached HEAD. That is expected; use `jj st` as the primary status view.",
        );
    }
    status
}

fn cmd_vcs_status(repo_root: &PathBuf) -> Result<()> {
    let mode = detect_vcs_mode(repo_root);
    let workspace_root = detect_backend(repo_root)
        .and_then(|backend| backend.workspace_root())
        .ok();
    let snapshot = current_snapshot_ref(repo_root)
        .ok()
        .map(|snapshot| (snapshot.head_ref, snapshot.dirty));

    println!(
        "{}",
        render_vcs_status_with_details(mode, workspace_root.as_deref(), snapshot.as_ref())
    );
    Ok(())
}

fn render_vcs_status_with_details(
    mode: VcsMode,
    workspace_root: Option<&std::path::Path>,
    snapshot: Option<&(Option<String>, bool)>,
) -> String {
    let mut out = render_vcs_status(mode);
    if let Some(root) = workspace_root {
        out.push_str(&format!("\nWorkspace root: {}", root.display()));
    }
    if let Some(snapshot) = snapshot {
        if let Some(head_ref) = snapshot.0.as_deref() {
            out.push_str(&format!("\nCurrent ref: {head_ref}"));
        }
        out.push_str(&format!(
            "\nWorking copy: {}",
            if snapshot.1 { "dirty" } else { "clean" }
        ));
    }
    out
}

fn format_jj_degraded_mode_warning() -> &'static str {
    "Warning: running in degraded git-only mode; enable jj for fuller punk functionality with `punk vcs enable-jj`"
}

fn cmd_vcs_enable_jj(repo_root: &PathBuf) -> Result<()> {
    match detect_vcs_mode(repo_root) {
        VcsMode::Jj => {
            println!("jj is already enabled for this repo.");
            Ok(())
        }
        VcsMode::GitWithJjAvailableButDisabled => {
            enable_jj_for_repo(repo_root)?;
            println!("Enabled jj for this repo.");
            Ok(())
        }
        VcsMode::GitOnly => Err(anyhow!(
            "jj is not installed; cannot enable jj for this repo"
        )),
        VcsMode::NoVcs => Err(anyhow!(
            "no Git or jj repo detected in the current directory"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use punk_domain::{
        CheckStatus, Decision, DecisionObject, DeterministicStatus, Receipt, ReceiptArtifacts, Run,
        RunStatus, RunVcs, VcsKind,
    };
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("punk-cli-{label}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_disabled_jj_git_mode_warns() {
        assert!(should_warn_about_disabled_jj(
            VcsMode::GitWithJjAvailableButDisabled
        ));
        assert!(!should_warn_about_disabled_jj(VcsMode::Jj));
        assert!(!should_warn_about_disabled_jj(VcsMode::GitOnly));
        assert!(!should_warn_about_disabled_jj(VcsMode::NoVcs));
    }

    #[test]
    fn vcs_status_mentions_enable_command_for_degraded_mode() {
        assert_eq!(
            format_vcs_status(VcsMode::GitWithJjAvailableButDisabled),
            "VCS mode: git-only (degraded; run `punk vcs enable-jj`)"
        );
        assert_eq!(format_vcs_status(VcsMode::Jj), "VCS mode: jj");
    }

    #[test]
    fn rendered_jj_status_explains_detached_head_behavior() {
        let rendered = render_vcs_status(VcsMode::Jj);
        assert!(rendered.contains("VCS mode: jj"));
        assert!(rendered.contains("detached HEAD"));
        assert!(rendered.contains("jj st"));
    }

    #[test]
    fn rendered_status_with_details_includes_root_ref_and_dirty_state() {
        let snapshot = (Some("abc123".to_string()), true);
        let rendered = render_vcs_status_with_details(
            VcsMode::Jj,
            Some(std::path::Path::new("/repo")),
            Some(&snapshot),
        );
        assert!(rendered.contains("Workspace root: /repo"));
        assert!(rendered.contains("Current ref: abc123"));
        assert!(rendered.contains("Working copy: dirty"));
    }

    #[test]
    fn degraded_warning_mentions_root_cli_enable_path() {
        let warning = format_jj_degraded_mode_warning();
        assert!(warning.contains("degraded git-only mode"));
        assert!(warning.contains("punk vcs enable-jj"));
        assert!(warning.contains("fuller punk functionality"));
    }

    #[test]
    fn cut_run_summary_mentions_vcs_change_and_workspace() {
        let run = Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: punk_domain::ModeId::Cut,
            vcs: RunVcs {
                backend: VcsKind::Jj,
                workspace_ref: "/repo".into(),
                change_ref: "abc123".into(),
                base_ref: Some("base123".into()),
            },
            started_at: "now".into(),
            ended_at: Some("later".into()),
        };
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "executor".into(),
            changed_files: vec![],
            artifacts: ReceiptArtifacts {
                stdout_ref: "out".into(),
                stderr_ref: "err".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: "now".into(),
        };
        let rendered = format_cut_run_summary(&run, &receipt);
        assert!(rendered.contains("run run_1 (success)"));
        assert!(rendered.contains("vcs: Jj change=abc123"));
        assert!(rendered.contains("workspace: /repo"));
    }

    #[test]
    fn gate_run_summary_mentions_live_vcs_fields() {
        let decision = DecisionObject {
            id: "dec_1".into(),
            run_id: "run_1".into(),
            contract_id: "ct_1".into(),
            decision: Decision::Accept,
            deterministic_status: DeterministicStatus::Pass,
            target_status: CheckStatus::Pass,
            integrity_status: CheckStatus::Pass,
            confidence_estimate: 0.95,
            decision_basis: vec!["checks passed".into()],
            contract_ref: "ct.json".into(),
            receipt_ref: "rcpt.json".into(),
            check_refs: vec![],
            created_at: "now".into(),
        };
        let status = punk_orch::StatusSnapshot {
            project_id: "proj".into(),
            events_count: 1,
            last_contract_id: Some("ct_1".into()),
            last_run_id: Some("run_1".into()),
            last_decision_id: Some("dec_1".into()),
            vcs_backend: Some(VcsKind::Jj),
            vcs_ref: Some("abc123".into()),
            vcs_dirty: true,
            workspace_root: Some("/repo".into()),
        };
        let rendered = format_gate_run_summary(&decision, &status);
        assert!(rendered.contains("decision Accept for run_1"));
        assert!(rendered.contains("vcs=Some(Jj)"));
        assert!(rendered.contains("ref=Some(\"abc123\")"));
        assert!(rendered.contains("dirty=true"));
    }

    #[test]
    fn infer_project_id_uses_repo_root_basename() {
        let root = PathBuf::from("/tmp/interviewcoach");
        assert_eq!(infer_project_id(&root).as_deref(), Some("interviewcoach"));
    }

    #[test]
    fn bootstrap_detection_checks_repo_local_skill_file() {
        let root = temp_test_dir("bootstrap-detect");

        assert!(needs_project_bootstrap(&root, "interviewcoach"));

        let bootstrap = project_bootstrap_file_path(&root, "interviewcoach");
        fs::create_dir_all(bootstrap.parent().unwrap()).unwrap();
        fs::write(&bootstrap, "core rules\n").unwrap();

        assert!(!needs_project_bootstrap(&root, "interviewcoach"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bootstrap_error_mentions_manual_recovery_command() {
        let message = format_bootstrap_error("interviewcoach", "failed to execute punk-run");
        assert!(message.contains("project bootstrap failed"));
        assert!(message.contains("punk-run init --project interviewcoach --enable-jj --verify"));
    }

    #[test]
    fn bootstrap_json_mode_supports_start_and_plot_contract() {
        let start = Command::Start(StartCommand {
            goal: "ship interview summary".into(),
            json: true,
        });
        let plot = Command::Plot(PlotCommand {
            action: PlotAction::Contract {
                prompt: "ship interview summary".into(),
                json: false,
            },
        });
        let status = Command::Status(StatusCommand {
            id: None,
            json: false,
        });

        assert_eq!(bootstrap_json_mode(&start), Some(true));
        assert_eq!(bootstrap_json_mode(&plot), Some(false));
        assert_eq!(bootstrap_json_mode(&status), None);
    }

    #[test]
    fn start_summary_mentions_goal_project_and_next_step() {
        let rendered = format_start_summary(
            "interviewcoach",
            "add interview feedback summary endpoint",
            "ct_123",
            "punk plot approve ct_123",
        );
        assert!(rendered.contains("Goal: add interview feedback summary endpoint"));
        assert!(rendered.contains("Project: interviewcoach"));
        assert!(rendered.contains("Drafted contract: ct_123"));
        assert!(rendered.contains("Next: punk plot approve ct_123"));
    }
}
