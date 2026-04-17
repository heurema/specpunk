use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand};
use punk_adapters::{CodexCliContractDrafter, CodexCliExecutor, Executor};
use punk_domain::{
    now_rfc3339, IncidentPromotionExecution, IncidentPromotionFailure, IncidentPromotionRecord,
    Project,
};
use punk_gate::GateService;
use punk_orch::{
    project_id as runtime_project_id, read_json, relative_ref,
    suspected_runtime_bug_reasons_for_decision, write_json, ArchitectureMode, OrchService,
};
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
    Init(InitCommand),
    Go(GoCommand),
    Start(StartCommand),
    Research(ResearchCommand),
    Incident(IncidentCommand),
    Plot(PlotCommand),
    Cut(CutCommand),
    Gate(GateCommand),
    Gc(GcCommand),
    Status(StatusCommand),
    Inspect(InspectCommand),
    Vcs(VcsCommand),
}

#[derive(Args)]
struct InitCommand {
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    enable_jj: bool,
    #[arg(long)]
    verify: bool,
}

#[derive(Args)]
struct GoCommand {
    goal: String,
    #[arg(long)]
    fallback_staged: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct StartCommand {
    goal: String,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct ResearchCommand {
    #[command(subcommand)]
    action: ResearchAction,
}

#[derive(Args)]
struct IncidentCommand {
    #[command(subcommand)]
    action: IncidentAction,
}

#[derive(Subcommand)]
enum ResearchAction {
    Start {
        question: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        goal: String,
        #[arg(long = "success")]
        success_criteria: Vec<String>,
        #[arg(long = "constraint")]
        constraints: Vec<String>,
        #[arg(long)]
        subject_ref: Option<String>,
        #[arg(long = "context-ref")]
        context_refs: Vec<String>,
        #[arg(long)]
        contract_ref: Option<String>,
        #[arg(long)]
        receipt_ref: Option<String>,
        #[arg(long)]
        skill_ref: Option<String>,
        #[arg(long)]
        eval_ref: Option<String>,
        #[arg(long, default_value_t = 3)]
        max_rounds: u32,
        #[arg(long, default_value_t = 5)]
        max_worker_slots: u32,
        #[arg(long, default_value_t = 30)]
        max_duration_minutes: u32,
        #[arg(long, default_value_t = 12)]
        max_artifacts: u32,
        #[arg(long)]
        max_cost_usd: Option<f64>,
        #[arg(long)]
        json: bool,
    },
    Artifact {
        research_id: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        source_ref: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Synthesize {
        research_id: String,
        #[arg(long)]
        outcome: String,
        #[arg(long)]
        summary: String,
        #[arg(long = "artifact-ref")]
        artifact_refs: Vec<String>,
        #[arg(long = "follow-up-ref")]
        follow_up_refs: Vec<String>,
        #[arg(long)]
        replace: bool,
        #[arg(long)]
        json: bool,
    },
    Complete {
        research_id: String,
        #[arg(long)]
        json: bool,
    },
    Escalate {
        research_id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum IncidentAction {
    Capture {
        proof_id: String,
        #[arg(long)]
        json: bool,
    },
    Promote {
        incident_id: String,
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long = "auto-run")]
        auto_run: bool,
        #[arg(long)]
        json: bool,
    },
    Rerun {
        promotion_id: String,
        #[arg(long = "auto-run")]
        auto_run: bool,
        #[arg(long)]
        json: bool,
    },
    Submit {
        incident_id: String,
        #[arg(long = "github")]
        github_repo: Option<String>,
        #[arg(long)]
        publish: bool,
        #[arg(long)]
        json: bool,
    },
    Resubmit {
        submission_id: String,
        #[arg(long)]
        publish: bool,
        #[arg(long)]
        json: bool,
    },
    Defaults {
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long = "github")]
        github_repo: Option<String>,
        #[arg(long)]
        global: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
struct StatusCommand {
    id: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct InspectCommand {
    target: String,
    id: Option<String>,
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

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum ArchitectureCliMode {
    Auto,
    On,
    Off,
}

#[derive(Subcommand)]
enum PlotAction {
    Contract {
        prompt: String,
        #[arg(long, value_enum, default_value_t = ArchitectureCliMode::Auto)]
        architecture: ArchitectureCliMode,
        #[arg(long)]
        json: bool,
    },
    Refine {
        contract_id: String,
        guidance: String,
        #[arg(long, value_enum, default_value_t = ArchitectureCliMode::Auto)]
        architecture: ArchitectureCliMode,
        #[arg(long)]
        json: bool,
    },
    Approve {
        contract_id: String,
        #[arg(long)]
        json: bool,
    },
}

impl From<ArchitectureCliMode> for ArchitectureMode {
    fn from(value: ArchitectureCliMode) -> Self {
        match value {
            ArchitectureCliMode::Auto => ArchitectureMode::Auto,
            ArchitectureCliMode::On => ArchitectureMode::On,
            ArchitectureCliMode::Off => ArchitectureMode::Off,
        }
    }
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

#[derive(Args)]
struct GcCommand {
    #[command(subcommand)]
    action: GcAction,
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

#[derive(Subcommand)]
enum GcAction {
    Stale {
        #[arg(long)]
        dry_run: bool,
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
        Command::Init(init) => cmd_init(
            &repo_root,
            init.project.as_deref(),
            init.enable_jj,
            init.verify,
        ),
        Command::Go(go) => cmd_go(
            &repo_root,
            &global_root,
            &go.goal,
            go.fallback_staged,
            go.json,
        ),
        Command::Start(start) => cmd_start(&repo_root, &global_root, &start.goal, start.json),
        Command::Research(research) => match research.action {
            ResearchAction::Start {
                question,
                kind,
                goal,
                success_criteria,
                constraints,
                subject_ref,
                context_refs,
                contract_ref,
                receipt_ref,
                skill_ref,
                eval_ref,
                max_rounds,
                max_worker_slots,
                max_duration_minutes,
                max_artifacts,
                max_cost_usd,
                json,
            } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let view = orch.start_research(punk_domain::ResearchStartInput {
                    kind,
                    question,
                    goal,
                    subject_ref,
                    constraints,
                    success_criteria,
                    context_refs,
                    contract_ref,
                    receipt_ref,
                    skill_ref,
                    eval_ref,
                    budget: punk_domain::ResearchBudget {
                        max_rounds,
                        max_worker_slots,
                        max_cost_usd,
                        max_duration_minutes,
                        max_artifacts,
                    },
                })?;
                render(json, &view, &format_research_summary(&view))
            }
            ResearchAction::Artifact {
                research_id,
                kind,
                summary,
                source_ref,
                json,
            } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let view = orch.write_research_artifact(
                    &research_id,
                    punk_domain::ResearchArtifactInput {
                        kind,
                        summary,
                        source_ref,
                    },
                )?;
                render(json, &view, &format_research_summary(&view))
            }
            ResearchAction::Synthesize {
                research_id,
                outcome,
                summary,
                artifact_refs,
                follow_up_refs,
                replace,
                json,
            } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let view = orch.write_research_synthesis(
                    &research_id,
                    punk_domain::ResearchSynthesisInput {
                        outcome,
                        summary,
                        artifact_refs,
                        replace_existing: replace,
                        follow_up_refs,
                    },
                )?;
                render(json, &view, &format_research_summary(&view))
            }
            ResearchAction::Complete { research_id, json } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let view = orch.complete_research(&research_id)?;
                render(json, &view, &format_research_summary(&view))
            }
            ResearchAction::Escalate { research_id, json } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let view = orch.escalate_research(&research_id)?;
                render(json, &view, &format_research_summary(&view))
            }
        },
        Command::Incident(incident) => match incident.action {
            IncidentAction::Capture { proof_id, json } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let incident = orch.capture_incident(&proof_id)?;
                let next_actions = if json {
                    None
                } else {
                    Some(derive_incident_next_actions(&orch, &incident)?)
                };
                render(
                    json,
                    &incident,
                    &format_incident_summary(&incident, next_actions.as_ref()),
                )
            }
            IncidentAction::Promote {
                incident_id,
                repo,
                auto_run,
                json,
            } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let drafter = CodexCliContractDrafter::default();
                let target_repo = resolve_incident_promote_repo(&orch, repo.as_deref())?;
                let promotion = orch.promote_incident(&drafter, &incident_id, &target_repo)?;
                if auto_run {
                    let executor = CodexCliExecutor::default();
                    let auto_run = auto_run_existing_incident_promotion(
                        &orch,
                        &global_root,
                        &executor,
                        &promotion,
                    )?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&auto_run.promotion)?);
                    } else {
                        let policy = inspect_incident_auto_run_policy(
                            Path::new(&auto_run.promotion.target_repo_root),
                            Some(auto_run.promotion.target_project_id.as_str()),
                        );
                        println!(
                            "{}",
                            format_incident_promotion_summary(&auto_run.promotion, Some(&policy),)
                        );
                    }
                    if go_decision_succeeds(&auto_run.decision.decision) {
                        Ok(())
                    } else {
                        Err(anyhow!(format_incident_auto_run_error(
                            &auto_run.promotion,
                            &auto_run.proof.id,
                            &auto_run.decision.decision,
                        )))
                    }
                } else {
                    let policy = inspect_incident_auto_run_policy(
                        Path::new(&promotion.target_repo_root),
                        Some(promotion.target_project_id.as_str()),
                    );
                    render(
                        json,
                        &promotion,
                        &format_incident_promotion_summary(&promotion, Some(&policy)),
                    )
                }
            }
            IncidentAction::Rerun {
                promotion_id,
                auto_run,
                json,
            } => {
                if !auto_run {
                    return Err(anyhow!(
                        "incident rerun requires --auto-run to avoid silently rerunning the target contract"
                    ));
                }
                let orch = OrchService::new(&repo_root, &global_root)?;
                let executor = CodexCliExecutor::default();
                let promotion = orch.inspect_incident_promotion(&promotion_id)?;
                let auto_run = auto_run_existing_incident_promotion(
                    &orch,
                    &global_root,
                    &executor,
                    &promotion,
                )?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&auto_run.promotion)?);
                } else {
                    let policy = inspect_incident_auto_run_policy(
                        Path::new(&auto_run.promotion.target_repo_root),
                        Some(auto_run.promotion.target_project_id.as_str()),
                    );
                    println!(
                        "{}",
                        format_incident_promotion_summary(&auto_run.promotion, Some(&policy))
                    );
                }
                if go_decision_succeeds(&auto_run.decision.decision) {
                    Ok(())
                } else {
                    Err(anyhow!(format_incident_auto_run_error(
                        &auto_run.promotion,
                        &auto_run.proof.id,
                        &auto_run.decision.decision,
                    )))
                }
            }
            IncidentAction::Submit {
                incident_id,
                github_repo,
                publish,
                json,
            } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let github_repo = resolve_incident_submit_repo(&orch, github_repo.as_deref())?;
                let submission = orch.submit_incident(&incident_id, &github_repo, publish)?;
                render(
                    json,
                    &submission,
                    &format_incident_submission_summary(&submission),
                )
            }
            IncidentAction::Resubmit {
                submission_id,
                publish,
                json,
            } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let submission = orch.resubmit_incident_submission(&submission_id, publish)?;
                render(
                    json,
                    &submission,
                    &format_incident_submission_summary(&submission),
                )
            }
            IncidentAction::Defaults {
                repo,
                github_repo,
                global,
                json,
            } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let defaults = if repo.is_some() || github_repo.is_some() {
                    if global {
                        orch.set_global_incident_defaults(repo.as_deref(), github_repo.as_deref())?
                    } else {
                        orch.set_incident_defaults(repo.as_deref(), github_repo.as_deref())?
                    }
                } else if global {
                    orch.inspect_global_incident_defaults()?
                } else {
                    orch.inspect_incident_defaults()?
                };
                let label = if global {
                    "Global incident defaults"
                } else {
                    "Incident defaults"
                };
                render(
                    json,
                    &defaults,
                    &format_incident_defaults_summary(label, &defaults),
                )
            }
        },
        Command::Plot(plot) => match plot.action {
            PlotAction::Contract {
                prompt,
                architecture,
                json,
            } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let drafter = CodexCliContractDrafter::default();
                let contract =
                    orch.draft_contract_with_options(&drafter, &prompt, architecture.into())?;
                let persisted = orch.inspect(&contract.id)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&persisted)?);
                } else {
                    println!(
                        "{}",
                        format_plot_contract_summary("drafted", &contract, &persisted, &repo_root)
                    );
                }
                Ok(())
            }
            PlotAction::Refine {
                contract_id,
                guidance,
                architecture,
                json,
            } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let drafter = CodexCliContractDrafter::default();
                let contract = orch.refine_contract_with_options(
                    &drafter,
                    &contract_id,
                    &guidance,
                    architecture.into(),
                )?;
                let persisted = orch.inspect(&contract.id)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&persisted)?);
                } else {
                    println!(
                        "{}",
                        format_plot_contract_summary("refined", &contract, &persisted, &repo_root)
                    );
                }
                Ok(())
            }
            PlotAction::Approve { contract_id, json } => {
                let orch = OrchService::new(&repo_root, &global_root)?;
                let contract = orch.approve_contract(&contract_id)?;
                let persisted = orch.inspect(&contract.id)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&persisted)?);
                } else {
                    println!(
                        "{}",
                        format_plot_contract_summary("approved", &contract, &persisted, &repo_root)
                    );
                }
                Ok(())
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
                let persisted = orch.inspect(&decision.id)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&persisted)?);
                } else {
                    println!(
                        "{}",
                        format_gate_run_summary(&decision, &status, &persisted, &repo_root)
                    );
                }
                Ok(())
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
        Command::Gc(gc) => match gc.action {
            GcAction::Stale { dry_run, json } => {
                if !dry_run {
                    return Err(anyhow!(
                        "only `punk gc stale --dry-run` is supported in this slice"
                    ));
                }
                let orch = OrchService::new(&repo_root, &global_root)?;
                let report = orch.gc_stale_dry_run()?;
                render(json, &report, &format_stale_gc_report(&report))
            }
        },
        Command::Status(status) => {
            let orch = OrchService::new(&repo_root, &global_root)?;
            let snapshot = orch.status(status.id.as_deref())?;
            render(status.json, &snapshot, &format_status_summary(&snapshot))
        }
        Command::Inspect(inspect) => {
            let orch = OrchService::new(&repo_root, &global_root)?;
            if inspect.target == "project" && inspect.id.is_none() {
                let overlay = orch.inspect_project_overlay()?;
                return render(
                    inspect.json,
                    &overlay,
                    &format_project_overlay_summary(&overlay),
                );
            }
            if inspect.target == "work" {
                let ledger = orch.inspect_work_ledger(inspect.id.as_deref())?;
                let latest_proof_harness_evidence = if inspect.json {
                    None
                } else {
                    load_latest_proofpack(&repo_root, &ledger)?
                        .map(|proof| summarize_proof_harness_evidence(&proof))
                };
                return render(
                    inspect.json,
                    &ledger,
                    &format_work_ledger_summary(&ledger, latest_proof_harness_evidence.as_deref()),
                );
            }
            if !inspect.json && inspect.id.is_none() && inspect.target.starts_with("proof_") {
                let proof = orch.inspect_proofpack(&inspect.target)?;
                return render(false, &proof, &format_proofpack_summary(&proof));
            }
            if !inspect.json && inspect.id.is_none() && inspect.target.starts_with("inc_") {
                let incident = orch.inspect_incident(&inspect.target)?;
                let next_actions = derive_incident_next_actions(&orch, &incident)?;
                return render(
                    false,
                    &incident,
                    &format_incident_summary(&incident, Some(&next_actions)),
                );
            }
            if !inspect.json && inspect.id.is_none() && inspect.target.starts_with("prom_") {
                let promotion = orch.inspect_incident_promotion(&inspect.target)?;
                let policy = inspect_incident_auto_run_policy(
                    Path::new(&promotion.target_repo_root),
                    Some(promotion.target_project_id.as_str()),
                );
                return render(
                    false,
                    &promotion,
                    &format_incident_promotion_summary(&promotion, Some(&policy)),
                );
            }
            if !inspect.json && inspect.id.is_none() && inspect.target.starts_with("sub_") {
                let submission = orch.inspect_incident_submission(&inspect.target)?;
                return render(
                    false,
                    &submission,
                    &format_incident_submission_summary(&submission),
                );
            }
            if inspect.id.is_none() && inspect.target.starts_with("research_") {
                let research = orch.inspect_research(&inspect.target)?;
                return render(inspect.json, &research, &format_research_summary(&research));
            }
            if !inspect.json || inspect.id.is_some() {
                return Err(anyhow!(
                    "inspect for object ids currently requires `punk inspect <id> --json`; only `proof_<id>`, `inc_<id>`, `prom_<id>`, `sub_<id>`, and `research_<id>` currently support human inspect output. Use `punk inspect project` or `punk inspect work [id]` for human inspect views"
                ));
            }
            let value = orch.inspect(&inspect.target)?;
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

fn format_plot_contract_summary(
    action: &str,
    contract: &punk_domain::Contract,
    persisted: &serde_json::Value,
    repo_root: &Path,
) -> String {
    let mut rendered = format!("{action} {}", contract.id);
    if let Some(summary) = format_contract_architecture_summary(persisted, repo_root) {
        rendered.push_str(&format!("\n{summary}"));
    }
    rendered
}

fn format_gate_run_summary(
    decision: &punk_domain::DecisionObject,
    status: &punk_orch::StatusSnapshot,
    persisted: &serde_json::Value,
    repo_root: &Path,
) -> String {
    let mut rendered = format!(
        "decision {:?} for {} (vcs={:?} ref={:?} dirty={} workspace_root={:?})",
        decision.decision,
        decision.run_id,
        status.vcs_backend,
        status.vcs_ref,
        status.vcs_dirty,
        status.workspace_root
    );
    if let Some(assessment_ref) = decision_architecture_assessment_ref(persisted) {
        if let Ok(assessment) =
            read_json::<punk_domain::ArchitectureAssessment>(&repo_root.join(assessment_ref))
        {
            rendered.push_str(&format!(
                "\narchitecture: {} ({})\narchitecture_assessment: {}",
                architecture_assessment_outcome_label(&assessment.outcome),
                summarize_architecture_reasons(&assessment.reasons),
                assessment_ref
            ));
        } else {
            rendered.push_str(&format!("\narchitecture_assessment: {}", assessment_ref));
        }
    }
    rendered
}

fn format_contract_architecture_summary(
    persisted: &serde_json::Value,
    repo_root: &Path,
) -> Option<String> {
    let signals_ref = persisted
        .get("architecture_signals_ref")
        .and_then(|value| value.as_str());
    let brief_ref = persisted
        .get("architecture_integrity")
        .and_then(|value| value.get("brief_ref"))
        .and_then(|value| value.as_str());

    if signals_ref.is_none() && brief_ref.is_none() {
        return None;
    }

    let mut lines = Vec::new();
    if let Some(signals_ref) = signals_ref {
        if let Ok(signals) =
            read_json::<punk_domain::ArchitectureSignals>(&repo_root.join(signals_ref))
        {
            if matches!(signals.severity, punk_domain::ArchitectureSeverity::None)
                && brief_ref.is_none()
            {
                return None;
            }
            let reason_summary = summarize_architecture_reasons(&signals.trigger_reasons);
            lines.insert(
                0,
                format!(
                    "architecture: {} ({})",
                    architecture_severity_label(&signals.severity),
                    reason_summary
                ),
            );
        }
        lines.push(format!("architecture signals: {signals_ref}"));
    }
    if let Some(brief_ref) = brief_ref {
        lines.push(format!("architecture brief: {brief_ref}"));
    }
    if let Some(integrity) = persisted.get("architecture_integrity") {
        let review_required = integrity
            .get("review_required")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let touched_roots_max = integrity
            .get("touched_roots_max")
            .and_then(|value| value.as_u64())
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string());
        let file_loc_budget_count = integrity
            .get("file_loc_budgets")
            .and_then(|value| value.as_array())
            .map(|items| items.len())
            .unwrap_or(0);
        let forbidden_dependency_count = integrity
            .get("forbidden_path_dependencies")
            .and_then(|value| value.as_array())
            .map(|items| items.len())
            .unwrap_or(0);
        lines.push(format!(
            "architecture integrity: review_required={} touched_roots_max={} file_loc_budgets={} forbidden_path_dependencies={}",
            review_required,
            touched_roots_max,
            file_loc_budget_count,
            forbidden_dependency_count
        ));
    }
    Some(lines.join("\n"))
}

fn architecture_severity_label(severity: &punk_domain::ArchitectureSeverity) -> &'static str {
    match severity {
        punk_domain::ArchitectureSeverity::None => "none",
        punk_domain::ArchitectureSeverity::Warn => "warn",
        punk_domain::ArchitectureSeverity::Critical => "critical",
    }
}

fn architecture_assessment_outcome_label(
    outcome: &punk_domain::ArchitectureAssessmentOutcome,
) -> &'static str {
    match outcome {
        punk_domain::ArchitectureAssessmentOutcome::NotApplicable => "not_applicable",
        punk_domain::ArchitectureAssessmentOutcome::Pass => "pass",
        punk_domain::ArchitectureAssessmentOutcome::Block => "block",
        punk_domain::ArchitectureAssessmentOutcome::Escalate => "escalate",
    }
}

fn summarize_architecture_reasons(reasons: &[String]) -> String {
    let items = reasons
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .take(2)
        .collect::<Vec<_>>();
    if items.is_empty() {
        "no explicit architecture notes recorded".to_string()
    } else {
        items.join("; ")
    }
}

fn decision_architecture_assessment_ref(persisted: &serde_json::Value) -> Option<&str> {
    persisted
        .get("check_refs")
        .and_then(|value| value.as_array())
        .and_then(|refs| {
            refs.iter().find_map(|value| {
                let reference = value.as_str()?;
                reference
                    .ends_with("/architecture-assessment.json")
                    .then_some(reference)
            })
        })
}

fn format_status_summary(snapshot: &punk_orch::StatusSnapshot) -> String {
    let suggested_command = snapshot.suggested_command.as_deref().unwrap_or("none");
    format!(
        "project={} events={} work={:?} lifecycle={:?} autonomy_outcome={:?} recovery_contract_ref={:?} contract={:?} run={:?} decision={:?} next_action={:?} next_action_ref={:?} suggested_command={} blocked_reason={:?} vcs={:?} ref={:?} dirty={} workspace_root={:?}",
        snapshot.project_id,
        snapshot.events_count,
        snapshot.work_id,
        snapshot.lifecycle_state,
        snapshot.autonomy_outcome,
        snapshot.recovery_contract_ref,
        snapshot.last_contract_id,
        snapshot.last_run_id,
        snapshot.last_decision_id,
        snapshot.next_action,
        snapshot.next_action_ref,
        suggested_command,
        snapshot.blocked_reason,
        snapshot.vcs_backend,
        snapshot.vcs_ref,
        snapshot.vcs_dirty,
        snapshot.workspace_root
    )
}

fn format_stale_gc_report(report: &punk_orch::StaleGcReport) -> String {
    let safe_to_archive = if report.safe_to_archive.is_empty() {
        "none".to_string()
    } else {
        report
            .safe_to_archive
            .iter()
            .map(|candidate| {
                format!(
                    "- {} ({})\n  work: {}\n  ref: {}\n  reason: {}",
                    candidate.artifact_id,
                    candidate.artifact_kind,
                    candidate.work_id,
                    candidate.artifact_ref,
                    candidate.reason
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let manual_review = if report.manual_review.is_empty() {
        "none".to_string()
    } else {
        report
            .manual_review
            .iter()
            .map(|candidate| {
                format!(
                    "- {} ({})\n  work: {}\n  ref: {}\n  reason: {}",
                    candidate.artifact_id,
                    candidate.artifact_kind,
                    candidate.work_id,
                    candidate.artifact_ref,
                    candidate.reason
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "Project: {}\nGenerated at: {}\nSafe to archive:\n{}\nManual review:\n{}",
        report.project_id, report.generated_at, safe_to_archive, manual_review
    )
}

fn default_global_root() -> Result<PathBuf> {
    let home = env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| anyhow!("HOME is not set"))?;
    Ok(home.join(".punk"))
}

#[derive(Debug, Clone)]
struct NativeBootstrapResult {
    project_label: String,
    project: Project,
    bootstrap_ref: String,
    agent_guidance_refs: Vec<String>,
    vcs_mode: VcsMode,
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

    ensure_native_project_bootstrap(&project_root, &project_id, false, false)
        .map_err(|err| anyhow!(format_bootstrap_error(&project_id, &err.to_string(),)))?;
    if !json {
        eprintln!("Bootstrap: wrote missing native punk guidance for `{project_id}`.");
    }
    Ok(true)
}

fn bootstrap_json_mode(command: &Command) -> Option<bool> {
    match command {
        Command::Plot(PlotCommand {
            action: PlotAction::Contract { json, .. },
        }) => Some(*json),
        Command::Go(GoCommand { json, .. }) => Some(*json),
        Command::Start(StartCommand { json, .. }) => Some(*json),
        Command::Research(ResearchCommand {
            action: ResearchAction::Start { json, .. },
        }) => Some(*json),
        Command::Research(ResearchCommand {
            action: ResearchAction::Artifact { json, .. },
        }) => Some(*json),
        Command::Research(ResearchCommand {
            action: ResearchAction::Synthesize { json, .. },
        }) => Some(*json),
        Command::Research(ResearchCommand {
            action: ResearchAction::Complete { json, .. },
        }) => Some(*json),
        Command::Research(ResearchCommand {
            action: ResearchAction::Escalate { json, .. },
        }) => Some(*json),
        Command::Incident(IncidentCommand {
            action: IncidentAction::Capture { json, .. },
        }) => Some(*json),
        Command::Incident(IncidentCommand {
            action: IncidentAction::Promote { json, .. },
        }) => Some(*json),
        Command::Incident(IncidentCommand {
            action: IncidentAction::Rerun { json, .. },
        }) => Some(*json),
        Command::Incident(IncidentCommand {
            action: IncidentAction::Submit { json, .. },
        }) => Some(*json),
        Command::Incident(IncidentCommand {
            action: IncidentAction::Resubmit { json, .. },
        }) => Some(*json),
        Command::Incident(IncidentCommand {
            action: IncidentAction::Defaults { json, .. },
        }) => Some(*json),
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

fn existing_bootstrap_file_paths(project_root: &Path) -> Result<Vec<PathBuf>> {
    let bootstrap_dir = project_root.join(".punk").join("bootstrap");
    if !bootstrap_dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = fs::read_dir(&bootstrap_dir)
        .with_context(|| format!("failed to read {}", bootstrap_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with("-core.md"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn resolve_bootstrap_doc_path(project_root: &Path, project_id: &str) -> Result<PathBuf> {
    let expected = project_bootstrap_file_path(project_root, project_id);
    if expected.exists() {
        return Ok(expected);
    }

    let existing = existing_bootstrap_file_paths(project_root)?;
    if existing.len() == 1 {
        return Ok(existing[0].clone());
    }

    Ok(expected)
}

fn needs_project_bootstrap(project_root: &Path, project_id: &str) -> bool {
    let bootstrap_missing = resolve_bootstrap_doc_path(project_root, project_id)
        .map(|path| !path.exists())
        .unwrap_or(true);
    let project_path = project_root.join(".punk").join("project.json");
    let repo_agents_path = project_root.join("AGENTS.md");
    let agent_start_path = project_root.join(".punk").join("AGENT_START.md");

    bootstrap_missing
        || !project_path.exists()
        || !repo_agents_path.exists()
        || !agent_start_path.exists()
}

fn format_bootstrap_error(project_id: &str, reason: &str) -> String {
    format!(
        "project bootstrap failed for `{project_id}`: {reason}. Run `punk init --project {project_id} --enable-jj --verify` and retry."
    )
}

fn maybe_enable_jj_for_init(project_root: &Path, enable_jj: bool) -> Result<()> {
    if !enable_jj {
        return Ok(());
    }

    match detect_vcs_mode(&project_root.to_path_buf()) {
        VcsMode::Jj => Ok(()),
        VcsMode::GitWithJjAvailableButDisabled => enable_jj_for_repo(&project_root.to_path_buf()),
        VcsMode::GitOnly => Err(anyhow!(
            "jj is not installed; cannot enable jj for this repo"
        )),
        VcsMode::NoVcs => Err(anyhow!(
            "no Git or jj repo detected in the current directory"
        )),
    }
}

fn write_text_if_missing(path: &Path, content: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

fn write_native_project_packet(project_root: &Path) -> Result<Project> {
    fs::create_dir_all(project_root.join(".punk"))?;

    let project_path = project_root.join(".punk").join("project.json");
    let current_id = runtime_project_id(project_root)?;
    let current_path = project_root.display().to_string();
    let current_vcs_backend = detect_backend(project_root)
        .ok()
        .map(|backend| backend.kind());

    if project_path.exists() {
        let mut project: Project = read_json(&project_path)?;
        let mut changed = false;
        if project.id != current_id {
            project.id = current_id.clone();
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
            project.updated_at = punk_domain::now_rfc3339();
            write_json(&project_path, &project)?;
        }
        return Ok(project);
    }

    let project = Project {
        id: current_id,
        path: current_path,
        vcs_backend: current_vcs_backend,
        created_at: punk_domain::now_rfc3339(),
        updated_at: punk_domain::now_rfc3339(),
    };
    write_json(&project_path, &project)?;
    Ok(project)
}

fn render_agents_md(project_label: &str, bootstrap_ref: &str) -> String {
    format!(
        "# AI Contributor Guide for {project_label}\n\n\
This repo is initialized for `punk`.\n\n\
## First read\n\n\
- `{bootstrap_ref}`\n\
- `.punk/AGENT_START.md`\n\n\
## Default work intake\n\n\
For normal work, start with:\n\n\
```bash\n\
punk go --fallback-staged \"<goal>\"\n\
```\n\n\
Use staged/manual flow when autonomy is blocked or exact review is needed:\n\n\
```bash\n\
punk start \"<goal>\"\n\
punk plot approve <contract-id>\n\
punk cut run <contract-id>\n\
punk gate run <run-id>\n\
```\n\n\
## Operating rules\n\n\
- Keep one diff, one purpose.\n\
- Prefer bounded scope over broad rewrites.\n\
- Update docs in the same diff when behavior changes.\n\
- Treat `plot` / `cut` / `gate` as expert/control surfaces, not the default user path.\n"
    )
}

fn render_agent_start_md(project_label: &str, bootstrap_ref: &str) -> String {
    format!(
        "# punk agent start\n\n\
Project: `{project_label}`\n\n\
Read `{bootstrap_ref}` and `AGENTS.md` before changing code.\n\n\
Default path:\n\n\
```bash\n\
punk go --fallback-staged \"<goal>\"\n\
```\n"
    )
}

fn render_bootstrap_core_md(project_label: &str) -> String {
    format!(
        "# {project_label} core bootstrap\n\n\
Use existing architecture and naming before introducing new abstractions.\n\n\
Prefer additive changes over rewrites.\n\n\
Keep slices bounded:\n\
- 1-3 files when possible\n\
- one diff, one purpose\n\n\
Prefer existing helpers, modules, and interfaces before creating new ones.\n\n\
For behavior changes:\n\
- preserve schemas unless acceptance explicitly changes them\n\
- no silent broad refactors\n\n\
For tests:\n\
- prefer focused tests near changed behavior\n\
- no change without verification\n\n\
Fail closed instead of guessing.\n"
    )
}

fn ensure_native_project_bootstrap(
    project_root: &Path,
    project_label: &str,
    enable_jj: bool,
    verify: bool,
) -> Result<NativeBootstrapResult> {
    maybe_enable_jj_for_init(project_root, enable_jj)?;

    let bootstrap_path = resolve_bootstrap_doc_path(project_root, project_label)?;
    let bootstrap_ref = relative_ref(project_root, &bootstrap_path)?;
    write_text_if_missing(&bootstrap_path, &render_bootstrap_core_md(project_label))?;

    let agents_path = project_root.join("AGENTS.md");
    write_text_if_missing(
        &agents_path,
        &render_agents_md(project_label, &bootstrap_ref),
    )?;
    let agent_start_path = project_root.join(".punk").join("AGENT_START.md");
    write_text_if_missing(
        &agent_start_path,
        &render_agent_start_md(project_label, &bootstrap_ref),
    )?;

    let project = write_native_project_packet(project_root)?;
    ensure_default_gitignore_coverage(project_root)?;
    let vcs_mode = detect_vcs_mode(&project_root.to_path_buf());

    if verify {
        let required_paths = [
            project_root.join(".punk").join("project.json"),
            agents_path.clone(),
            agent_start_path.clone(),
            bootstrap_path.clone(),
        ];
        for path in required_paths {
            if !path.exists() {
                return Err(anyhow!(
                    "native bootstrap verification failed: missing {}",
                    path.display()
                ));
            }
        }
    }

    Ok(NativeBootstrapResult {
        project_label: project_label.to_string(),
        project,
        bootstrap_ref,
        agent_guidance_refs: vec![
            relative_ref(project_root, &agents_path)?,
            relative_ref(project_root, &agent_start_path)?,
        ],
        vcs_mode,
    })
}

fn format_init_summary(result: &NativeBootstrapResult, verify: bool) -> String {
    let verification = if verify {
        "Verification: complete"
    } else {
        "Verification: skipped"
    };
    format!(
        "Project: {project_label}\nProject id: {project_id}\n{vcs_status}\nBootstrap: {bootstrap_ref}\nGuidance: {guidance}\n{verification}",
        project_label = result.project_label,
        project_id = result.project.id,
        vcs_status = format_vcs_status(result.vcs_mode),
        bootstrap_ref = result.bootstrap_ref,
        guidance = result.agent_guidance_refs.join(", "),
        verification = verification,
    )
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

fn cmd_init(
    repo_root: &Path,
    explicit_project: Option<&str>,
    enable_jj: bool,
    verify: bool,
) -> Result<()> {
    let project_root = resolve_project_root(repo_root);
    let project_id = resolve_init_project_id(&project_root, explicit_project)?;
    let result = ensure_native_project_bootstrap(&project_root, &project_id, enable_jj, verify)
        .map_err(|err| anyhow!(format_init_error(&project_id, &err.to_string())))?;
    println!("{}", format_init_summary(&result, verify));
    Ok(())
}

fn cmd_start(repo_root: &Path, global_root: &Path, goal: &str, json: bool) -> Result<()> {
    let trimmed_goal = goal.trim();
    if trimmed_goal.is_empty() {
        return Err(anyhow!("goal must not be empty"));
    }

    let project_root = resolve_project_root(repo_root);
    let project = infer_project_id(&project_root).unwrap_or_else(|| "project".to_string());
    let retry_command = format!("punk start {}", shell_quote_goal(trimmed_goal));
    if let Some(note) =
        ensure_vcs_ready_for_goal_intake(repo_root, &project, "punk start", &retry_command)?
    {
        eprintln!("{note}");
    }

    let orch = OrchService::new(repo_root, global_root)?;
    let drafter = CodexCliContractDrafter::default();
    let contract = orch.draft_contract(&drafter, trimmed_goal)?;
    let status = orch.status(None)?;
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

fn cmd_go(
    repo_root: &Path,
    global_root: &Path,
    goal: &str,
    fallback_staged: bool,
    json: bool,
) -> Result<()> {
    let trimmed_goal = goal.trim();
    if trimmed_goal.is_empty() {
        return Err(anyhow!("goal must not be empty"));
    }

    let project_root = resolve_project_root(repo_root);
    let project = infer_project_id(&project_root).unwrap_or_else(|| "project".to_string());
    let retry_command = if fallback_staged {
        format!(
            "punk go --fallback-staged {}",
            shell_quote_goal(trimmed_goal)
        )
    } else {
        format!("punk go {}", shell_quote_goal(trimmed_goal))
    };
    if let Some(note) =
        ensure_vcs_ready_for_goal_intake(repo_root, &project, "punk go", &retry_command)?
    {
        eprintln!("{note}");
    }

    let orch = OrchService::new(repo_root, global_root)?;
    let drafter = CodexCliContractDrafter::default();
    let executor = CodexCliExecutor::default();
    let gate = GateService::new(repo_root, global_root);
    let proof_service = ProofService::new(repo_root, global_root);
    let initial_cycle = run_go_cycle(
        &orch,
        &drafter,
        &executor,
        &gate,
        &proof_service,
        trimmed_goal,
    )?;
    let follow_up_goal = if should_auto_chain_after_bootstrap(trimmed_goal, &initial_cycle) {
        Some(auto_chain_follow_up_goal(&project_root, trimmed_goal))
    } else {
        None
    };
    let follow_up_cycle = if should_auto_chain_after_bootstrap(trimmed_goal, &initial_cycle) {
        Some(run_go_cycle(
            &orch,
            &drafter,
            &executor,
            &gate,
            &proof_service,
            follow_up_goal.as_deref().unwrap_or(trimmed_goal),
        )?)
    } else {
        None
    };
    let final_cycle = follow_up_cycle.as_ref().unwrap_or(&initial_cycle);
    let outcome = go_outcome_label(&final_cycle.decision.decision);
    let success = go_decision_succeeds(&final_cycle.decision.decision);
    let basis_summary = summarize_decision_basis(&final_cycle.decision.decision_basis);
    let recovery_command = go_recovery_command(&final_cycle.decision.decision, trimmed_goal);
    let recommended_mode = go_recommended_mode(&final_cycle.decision.decision);
    let staged_recovery = if fallback_staged && !success {
        Some(orch.draft_contract(&drafter, trimmed_goal)?)
    } else {
        None
    };
    let recovery_next_command = staged_recovery
        .as_ref()
        .map(|contract| format!("punk plot approve {}", contract.id));
    let autonomy = orch.record_autonomy_outcome(
        &final_cycle.proof.id,
        staged_recovery
            .as_ref()
            .map(|contract| contract.id.as_str()),
    )?;
    let status = orch.status(Some(&final_cycle.run.id))?;
    let project = infer_project_id(&project_root).unwrap_or_else(|| status.project_id.clone());
    let next_command = format!("punk inspect {} --json", final_cycle.proof.id);
    let incident_capture_command = {
        let reasons = suspected_runtime_bug_reasons_for_decision(
            Some(final_cycle.receipt.summary.as_str()),
            &final_cycle.decision.decision_basis,
            &final_cycle.decision.decision,
        );
        (!reasons.is_empty()).then(|| format!("punk incident capture {}", final_cycle.proof.id))
    };
    let auto_chain_note = follow_up_cycle.as_ref().map(|cycle| {
        format!(
            "bootstrap proof {} triggered follow-up implementation cycle {}",
            initial_cycle.proof.id, cycle.proof.id
        )
    });

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "goal": trimmed_goal,
                "project": project,
                "project_id": status.project_id,
                "contract": &final_cycle.contract,
                "run": &final_cycle.run,
                "receipt": &final_cycle.receipt,
                "decision": &final_cycle.decision,
                "proof": &final_cycle.proof,
                "autonomy_record": autonomy,
                "outcome": outcome,
                "success": success,
                "decision_basis_summary": basis_summary,
                "recommended_mode": recommended_mode,
                "fallback_staged_enabled": fallback_staged,
                "auto_chained_after_bootstrap": follow_up_cycle.is_some(),
                "auto_chain_goal": follow_up_goal,
                "bootstrap_cycle": follow_up_cycle.as_ref().map(|_| serde_json::json!({
                    "contract": &initial_cycle.contract,
                    "run": &initial_cycle.run,
                    "receipt": &initial_cycle.receipt,
                    "decision": &initial_cycle.decision,
                    "proof": &initial_cycle.proof,
                })),
                "next_command": next_command,
                "recovery_command": recovery_command,
                "recovery_contract": staged_recovery,
                "recovery_next_command": recovery_next_command,
                "incident_capture_command": incident_capture_command,
                "auto_chain_note": auto_chain_note,
                "follow_up": next_command,
            }))?
        );
    } else {
        println!(
            "{}",
            format_go_summary(
                &project,
                trimmed_goal,
                &final_cycle.contract.id,
                &final_cycle.run.id,
                &final_cycle.receipt.status,
                &final_cycle.receipt.summary,
                outcome,
                decision_label(&final_cycle.decision.decision),
                &basis_summary,
                &final_cycle.proof.id,
                &next_command,
                auto_chain_note.as_deref(),
                recovery_command.as_deref(),
                staged_recovery
                    .as_ref()
                    .map(|contract| contract.id.as_str()),
                recovery_next_command.as_deref(),
                incident_capture_command.as_deref(),
            )
        );
    }
    if success {
        Ok(())
    } else {
        Err(anyhow!(format_go_error(
            &final_cycle.decision.decision,
            &final_cycle.proof.id,
            &next_command,
            recovery_command.as_deref(),
        )))
    }
}

struct GoCycleResult {
    contract: punk_domain::Contract,
    run: punk_domain::Run,
    receipt: punk_domain::Receipt,
    decision: punk_domain::DecisionObject,
    proof: punk_domain::Proofpack,
}

#[derive(Debug)]
struct IncidentPromotionAutoRunResult {
    promotion: IncidentPromotionRecord,
    decision: punk_domain::DecisionObject,
    proof: punk_domain::Proofpack,
}

struct IncidentPromotionAttemptError {
    phase: &'static str,
    error: anyhow::Error,
    contract_status: Option<String>,
    run_id: Option<String>,
    receipt_ref: Option<String>,
    decision_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IncidentAutoRunPolicy {
    allowed: bool,
    detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct IncidentNextActions {
    promote_target: Option<String>,
    promote_command: Option<String>,
    auto_run_policy: Option<String>,
    auto_run_command: Option<String>,
    setup_hint: Option<String>,
}

fn run_go_cycle(
    orch: &OrchService,
    drafter: &CodexCliContractDrafter,
    executor: &CodexCliExecutor,
    gate: &GateService,
    proof_service: &ProofService,
    goal: &str,
) -> Result<GoCycleResult> {
    let contract = orch.draft_contract(drafter, goal)?;
    let approved = orch.approve_contract(&contract.id)?;
    let (run, receipt) = orch.cut_run(executor, &approved.id)?;
    let decision = gate.gate_run(&run.id)?;
    let proof = proof_service.write_proofpack(&decision.id)?;
    Ok(GoCycleResult {
        contract: approved,
        run,
        receipt,
        decision,
        proof,
    })
}

fn attempt_incident_promotion_auto_run(
    source_orch: &OrchService,
    global_root: &Path,
    executor: &dyn Executor,
    promotion: IncidentPromotionRecord,
) -> std::result::Result<IncidentPromotionAutoRunResult, IncidentPromotionAttemptError> {
    let target_repo_root = PathBuf::from(&promotion.target_repo_root);
    let target_orch = OrchService::new(&target_repo_root, global_root).map_err(|error| {
        IncidentPromotionAttemptError {
            phase: "target_repo_init",
            error,
            contract_status: None,
            run_id: None,
            receipt_ref: None,
            decision_id: None,
        }
    })?;
    let approved =
        ensure_incident_promotion_contract_ready(&target_orch, &promotion.draft_contract_id)
            .map_err(|error| IncidentPromotionAttemptError {
                phase: "contract_ready",
                error,
                contract_status: target_orch
                    .inspect_contract(&promotion.draft_contract_id)
                    .ok()
                    .map(|contract| format_contract_status_label(&contract.status).to_string()),
                run_id: None,
                receipt_ref: None,
                decision_id: None,
            })?;
    let (run, receipt) = target_orch
        .cut_run(executor, &approved.id)
        .map_err(|error| IncidentPromotionAttemptError {
            phase: "cut_run",
            error,
            contract_status: Some(format_contract_status_label(&approved.status).to_string()),
            run_id: None,
            receipt_ref: None,
            decision_id: None,
        })?;
    let gate = GateService::new(&target_repo_root, global_root);
    let decision = gate
        .gate_run(&run.id)
        .map_err(|error| IncidentPromotionAttemptError {
            phase: "gate_run",
            error,
            contract_status: Some(format_contract_status_label(&approved.status).to_string()),
            run_id: Some(run.id.clone()),
            receipt_ref: Some(format!(".punk/runs/{}/receipt.json", run.id)),
            decision_id: None,
        })?;
    let proof_service = ProofService::new(&target_repo_root, global_root);
    let proof = proof_service
        .write_proofpack(&decision.id)
        .map_err(|error| IncidentPromotionAttemptError {
            phase: "proof_write",
            error,
            contract_status: Some(format_contract_status_label(&approved.status).to_string()),
            run_id: Some(run.id.clone()),
            receipt_ref: Some(decision.receipt_ref.clone()),
            decision_id: Some(decision.id.clone()),
        })?;
    let execution = IncidentPromotionExecution {
        run_id: run.id.clone(),
        receipt_ref: decision.receipt_ref.clone(),
        decision_id: decision.id.clone(),
        proof_id: proof.id.clone(),
        decision_outcome: decision_label(&decision.decision).to_string(),
        receipt_summary: receipt.summary.clone(),
        completed_at: now_rfc3339(),
    };
    let promotion = source_orch
        .record_incident_promotion_execution(&promotion.id, execution)
        .map_err(|error| IncidentPromotionAttemptError {
            phase: "promotion_record_update",
            error,
            contract_status: Some(format_contract_status_label(&approved.status).to_string()),
            run_id: Some(run.id.clone()),
            receipt_ref: Some(decision.receipt_ref.clone()),
            decision_id: Some(decision.id.clone()),
        })?;
    Ok(IncidentPromotionAutoRunResult {
        promotion,
        decision,
        proof,
    })
}

fn auto_run_existing_incident_promotion(
    source_orch: &OrchService,
    global_root: &Path,
    executor: &dyn Executor,
    promotion: &IncidentPromotionRecord,
) -> Result<IncidentPromotionAutoRunResult> {
    if promotion.execution.is_some() {
        return Err(anyhow!(
            "promotion {} already has a completed execution; inspect it before rerunning",
            promotion.id
        ));
    }
    let policy = ensure_incident_promotion_auto_run_allowed(promotion)?;
    match attempt_incident_promotion_auto_run(source_orch, global_root, executor, promotion.clone())
    {
        Ok(result) => Ok(result),
        Err(error) => {
            let failure = IncidentPromotionFailure {
                phase: error.phase.to_string(),
                summary: error.error.to_string(),
                contract_status: error.contract_status.clone(),
                run_id: error.run_id.clone(),
                receipt_ref: error.receipt_ref.clone(),
                decision_id: error.decision_id.clone(),
                failed_at: now_rfc3339(),
            };
            match source_orch.record_incident_promotion_failure(&promotion.id, failure) {
                Ok(updated) => Err(anyhow!(format_incident_auto_run_failure(
                    &updated,
                    error.error.as_ref(),
                ))),
                Err(record_error) => Err(anyhow!(
                    "{} Additional persistence error: {}",
                    format_incident_auto_run_failure_with_policy(
                        promotion,
                        error.error.as_ref(),
                        &policy,
                    ),
                    record_error
                )),
            }
        }
    }
}

fn ensure_incident_promotion_auto_run_allowed(
    promotion: &IncidentPromotionRecord,
) -> Result<IncidentAutoRunPolicy> {
    let policy = inspect_incident_auto_run_policy(
        Path::new(&promotion.target_repo_root),
        Some(promotion.target_project_id.as_str()),
    );
    if policy.allowed {
        Ok(policy)
    } else {
        Err(anyhow!(format_incident_auto_run_policy_error(
            promotion, &policy
        )))
    }
}

fn ensure_incident_promotion_contract_ready(
    target_orch: &OrchService,
    contract_id: &str,
) -> Result<punk_domain::Contract> {
    let contract = target_orch.inspect_contract(contract_id)?;
    match contract.status {
        punk_domain::ContractStatus::Draft => target_orch.approve_contract(contract_id),
        punk_domain::ContractStatus::Approved => Ok(contract),
        punk_domain::ContractStatus::Superseded | punk_domain::ContractStatus::Cancelled => {
            Err(anyhow!(
                "promotion contract {} is no longer runnable because it is {}",
                contract_id,
                format_contract_status_label(&contract.status)
            ))
        }
    }
}

fn should_auto_chain_after_bootstrap(goal: &str, cycle: &GoCycleResult) -> bool {
    go_decision_succeeds(&cycle.decision.decision)
        && cycle
            .receipt
            .summary
            .contains("controller bootstrap scaffold created and checks passed")
        && goal_requests_follow_up_implementation(goal)
}

fn goal_requests_follow_up_implementation(goal: &str) -> bool {
    let lower = goal.to_ascii_lowercase();
    ["implement", "add ", "support ", "wire ", "with tests"]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn auto_chain_follow_up_goal(repo_root: &Path, goal: &str) -> String {
    synthesize_follow_up_goal(repo_root, goal).unwrap_or_else(|| goal.to_string())
}

fn synthesize_follow_up_goal(repo_root: &Path, goal: &str) -> Option<String> {
    let lower = goal.to_ascii_lowercase();
    if !lower.contains("init") {
        return None;
    }
    let slug = infer_workspace_app_slug(repo_root, goal)?;
    let tests_clause = if lower.contains("test") {
        ", and tests"
    } else {
        ""
    };
    let mut requirements = Vec::new();
    if lower.contains("json") {
        requirements.push("add --json output");
    }
    if lower.contains("--force") || lower.contains(" force") {
        requirements.push("support --force");
    }
    if lower.contains("--project-root") || lower.contains("project-root") {
        requirements.push("support --project-root");
    }
    let mut follow_up = format!(
        "implement {slug} init command touching exactly crates/{slug}-cli/src/main.rs, crates/{slug}-core/src/lib.rs{tests_clause}"
    );
    if requirements.is_empty() {
        follow_up.push_str("; keep cargo test --workspace green");
    } else {
        follow_up.push_str("; ");
        follow_up.push_str(&requirements.join(", "));
        follow_up.push_str(", and keep cargo test --workspace green");
    }
    Some(follow_up)
}

fn infer_workspace_app_slug(repo_root: &Path, goal: &str) -> Option<String> {
    let crates_dir = repo_root.join("crates");
    let entries = fs::read_dir(&crates_dir).ok()?;
    let mut slugs = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(slug) = name.strip_suffix("-cli") else {
            continue;
        };
        if crates_dir.join(format!("{slug}-core")).is_dir() {
            slugs.push(slug.to_string());
        }
    }
    if slugs.is_empty() {
        return None;
    }
    slugs.sort();
    slugs.dedup();
    let goal_lower = goal.to_ascii_lowercase();
    if let Some(preferred) = slugs
        .iter()
        .find(|slug| goal_lower.contains(slug.as_str()))
        .cloned()
    {
        return Some(preferred);
    }
    slugs.into_iter().next()
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

fn ensure_vcs_ready_for_goal_intake(
    repo_root: &Path,
    project_id: &str,
    command_name: &str,
    retry_command: &str,
) -> Result<Option<String>> {
    if detect_vcs_mode(repo_root) == VcsMode::NoVcs {
        init_git_repo_for_goal_intake(repo_root).map_err(|error| {
            anyhow!(format_goal_intake_no_vcs_error(
                repo_root,
                project_id,
                command_name,
                retry_command,
                Some(&error.to_string())
            ))
        })?;
        if detect_vcs_mode(repo_root) == VcsMode::NoVcs {
            return Err(anyhow!(format_goal_intake_no_vcs_error(
                repo_root,
                project_id,
                command_name,
                retry_command,
                Some("git init completed but no supported VCS was detected afterward")
            )));
        }
        return Ok(Some(format!(
            "Note: initialized a Git repo for goal intake at {}. VCS mode: git-only (degraded; run `punk vcs enable-jj` for fuller punk functionality).",
            repo_root.display()
        )));
    }
    Ok(None)
}

fn init_git_repo_for_goal_intake(repo_root: &Path) -> Result<()> {
    let output = ProcessCommand::new("git")
        .args(["init", "-q"])
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("spawn git init in {}", repo_root.display()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "git init failed without output".to_string()
    };
    Err(anyhow!(detail))
}

fn format_goal_intake_no_vcs_error(
    repo_root: &Path,
    project_id: &str,
    command_name: &str,
    retry_command: &str,
    init_error: Option<&str>,
) -> String {
    let mut message = format!(
        "{command_name} requires a Git or jj-backed repo before goal intake. No VCS detected at {}.",
        repo_root.display()
    );
    if let Some(init_error) = init_error {
        message.push_str(&format!(" Automatic `git init` failed: {init_error}."));
    }
    message.push_str(&format!(
        " Recovery: run `git init`, then `punk init --project {project_id} --enable-jj --verify`, then retry `{retry_command}`."
    ));
    message
}

#[allow(clippy::too_many_arguments)]
fn format_go_summary(
    project: &str,
    goal: &str,
    contract_id: &str,
    run_id: &str,
    receipt_status: &str,
    receipt_summary: &str,
    outcome: &str,
    decision: &str,
    basis_summary: &str,
    proof_id: &str,
    next_command: &str,
    auto_chain_note: Option<&str>,
    recovery_command: Option<&str>,
    recovery_contract_id: Option<&str>,
    recovery_next_command: Option<&str>,
    incident_capture_command: Option<&str>,
) -> String {
    let mut rendered = format!(
        "Goal: {goal}\nProject: {project}\nApproved contract: {contract_id}\nRun: {run_id} ({receipt_status})\nSummary: {receipt_summary}\nOutcome: {outcome}\nGate: {decision}\nBasis: {basis_summary}\nProof: {proof_id}\nNext: {next_command}"
    );
    if let Some(auto_chain_note) = auto_chain_note {
        rendered.push_str(&format!("\nAuto-chain: {auto_chain_note}"));
    }
    if let Some(recovery_command) = recovery_command {
        rendered.push_str(&format!("\nRecovery: {recovery_command}"));
    }
    if let Some(recovery_contract_id) = recovery_contract_id {
        rendered.push_str(&format!("\nRecovery contract: {recovery_contract_id}"));
    }
    if let Some(recovery_next_command) = recovery_next_command {
        rendered.push_str(&format!("\nRecovery next: {recovery_next_command}"));
    }
    if let Some(incident_capture_command) = incident_capture_command {
        rendered.push_str(&format!("\nIncident capture: {incident_capture_command}"));
    }
    rendered
}

fn decision_label(decision: &punk_domain::Decision) -> &'static str {
    match decision {
        punk_domain::Decision::Accept => "accept",
        punk_domain::Decision::Block => "block",
        punk_domain::Decision::Escalate => "escalate",
    }
}

fn go_decision_succeeds(decision: &punk_domain::Decision) -> bool {
    matches!(decision, punk_domain::Decision::Accept)
}

fn go_outcome_label(decision: &punk_domain::Decision) -> &'static str {
    match decision {
        punk_domain::Decision::Accept => "success",
        punk_domain::Decision::Block => "blocked",
        punk_domain::Decision::Escalate => "escalated",
    }
}

fn summarize_decision_basis(basis: &[String]) -> String {
    let trimmed: Vec<_> = basis
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .take(2)
        .collect();
    if trimmed.is_empty() {
        "no explicit decision basis recorded".to_string()
    } else {
        trimmed.join("; ")
    }
}

fn go_recommended_mode(decision: &punk_domain::Decision) -> &'static str {
    match decision {
        punk_domain::Decision::Accept => "autonomous",
        punk_domain::Decision::Block | punk_domain::Decision::Escalate => "staged_review",
    }
}

fn go_recovery_command(decision: &punk_domain::Decision, goal: &str) -> Option<String> {
    match decision {
        punk_domain::Decision::Accept => None,
        punk_domain::Decision::Block | punk_domain::Decision::Escalate => {
            Some(format!("punk start {}", shell_quote_goal(goal)))
        }
    }
}

fn shell_quote_goal(goal: &str) -> String {
    format!("\"{}\"", goal.replace('\\', "\\\\").replace('"', "\\\""))
}

fn format_go_error(
    decision: &punk_domain::Decision,
    proof_id: &str,
    next_command: &str,
    recovery_command: Option<&str>,
) -> String {
    let mut rendered = format!(
        "punk go ended with gate decision {} (proof: {}). Inspect details with `{}`.",
        decision_label(decision),
        proof_id,
        next_command
    );
    if let Some(recovery_command) = recovery_command {
        rendered.push_str(&format!(" Retry in staged mode with `{recovery_command}`."));
    }
    rendered
}

fn format_incident_auto_run_error(
    promotion: &IncidentPromotionRecord,
    proof_id: &str,
    decision: &punk_domain::Decision,
) -> String {
    format!(
        "incident promote --auto-run ended with gate decision {} (proof: {}). Inspect details with `cd {} && punk inspect {}`.",
        decision_label(decision),
        proof_id,
        promotion.target_repo_root,
        proof_id
    )
}

fn format_incident_auto_run_failure(
    promotion: &IncidentPromotionRecord,
    error: &dyn std::error::Error,
) -> String {
    format!(
        "promotion {} auto-run failed: {}. Inspect with `punk inspect {}`. Retry with `punk incident rerun {} --auto-run`.",
        promotion.id, error, promotion.id, promotion.id
    )
}

fn format_incident_auto_run_failure_with_policy(
    promotion: &IncidentPromotionRecord,
    error: &dyn std::error::Error,
    policy: &IncidentAutoRunPolicy,
) -> String {
    format!(
        "{} Policy: {}.",
        format_incident_auto_run_failure(promotion, error),
        policy.detail
    )
}

fn format_incident_auto_run_policy_error(
    promotion: &IncidentPromotionRecord,
    policy: &IncidentAutoRunPolicy,
) -> String {
    format!(
        "promotion {} drafted, but auto-run is blocked by policy: {}. Inspect with `punk inspect {}`. Review the drafted contract with `cd {} && punk inspect {} --json`.",
        promotion.id,
        policy.detail,
        promotion.id,
        promotion.target_repo_root,
        promotion.draft_contract_id,
    )
}

fn inspect_incident_auto_run_policy(
    target_repo_root: &Path,
    expected_project_id: Option<&str>,
) -> IncidentAutoRunPolicy {
    let canonical_target_repo_root =
        fs::canonicalize(target_repo_root).unwrap_or_else(|_| target_repo_root.to_path_buf());
    let project_packet_path = canonical_target_repo_root.join(".punk/project.json");
    let project: Project = match read_json(&project_packet_path) {
        Ok(project) => project,
        Err(_) => {
            return IncidentAutoRunPolicy {
                allowed: false,
                detail: "target repo is missing a readable .punk/project.json identity packet"
                    .to_string(),
            }
        }
    };
    if let Some(expected_project_id) = expected_project_id {
        if project.id != expected_project_id {
            return IncidentAutoRunPolicy {
                allowed: false,
                detail: format!(
                    "target repo identity packet does not match the promoted target id {}",
                    expected_project_id
                ),
            };
        }
    }
    let packet_repo_root =
        fs::canonicalize(&project.path).unwrap_or_else(|_| PathBuf::from(&project.path));
    if packet_repo_root != canonical_target_repo_root {
        return IncidentAutoRunPolicy {
            allowed: false,
            detail: "target repo identity packet points at a different repo root".to_string(),
        };
    }
    let agents_path = canonical_target_repo_root.join("AGENTS.md");
    let agents_contents = match fs::read_to_string(&agents_path) {
        Ok(contents) => contents,
        Err(_) => {
            return IncidentAutoRunPolicy {
                allowed: false,
                detail: "target repo is missing a readable AGENTS.md identity guide".to_string(),
            }
        }
    };
    if !agents_contents.contains("for specpunk") {
        return IncidentAutoRunPolicy {
            allowed: false,
            detail: "target repo AGENTS.md does not identify specpunk upstream".to_string(),
        };
    }
    let markers = [
        "Cargo.toml",
        "crates/specpunk/src/main.rs",
        "crates/punk-orch/src/lib.rs",
        "docs/product/CLI.md",
    ];
    let missing = markers
        .iter()
        .filter_map(|marker| {
            (!canonical_target_repo_root.join(marker).exists()).then(|| marker.to_string())
        })
        .collect::<Vec<_>>();
    if missing.is_empty() {
        IncidentAutoRunPolicy {
            allowed: true,
            detail: "target repo identity matches specpunk upstream markers".to_string(),
        }
    } else {
        IncidentAutoRunPolicy {
            allowed: false,
            detail: format!(
                "target repo identity matches, but specpunk upstream markers are missing: {}",
                missing.join(", ")
            ),
        }
    }
}

fn derive_incident_next_actions(
    orch: &OrchService,
    incident: &punk_domain::IncidentRecord,
) -> Result<IncidentNextActions> {
    let defaults = orch.inspect_effective_incident_defaults()?;
    let promote_target = defaults.promote_repo_root.clone();
    let promote_command = promote_target
        .as_ref()
        .map(|_| format!("punk incident promote {}", incident.id));
    let (auto_run_policy, auto_run_command) =
        if let Some(target_repo_root) = promote_target.as_deref() {
            let policy = inspect_incident_auto_run_policy(Path::new(target_repo_root), None);
            let policy_line = if policy.allowed {
                format!("allowed: {}", policy.detail)
            } else {
                format!("blocked: {}", policy.detail)
            };
            let command = policy
                .allowed
                .then(|| format!("punk incident promote {} --auto-run", incident.id));
            (Some(policy_line), command)
        } else {
            (None, None)
        };
    let setup_hint = if promote_target.is_none() {
        Some(
            "configure a promote target with `punk incident defaults --repo /path/to/specpunk` or `punk incident defaults --global --repo /path/to/specpunk`".to_string(),
        )
    } else {
        None
    };
    Ok(IncidentNextActions {
        promote_target,
        promote_command,
        auto_run_policy,
        auto_run_command,
        setup_hint,
    })
}

fn format_contract_status_label(status: &punk_domain::ContractStatus) -> &'static str {
    match status {
        punk_domain::ContractStatus::Draft => "draft",
        punk_domain::ContractStatus::Approved => "approved",
        punk_domain::ContractStatus::Superseded => "superseded",
        punk_domain::ContractStatus::Cancelled => "cancelled",
    }
}

fn format_project_overlay_summary(overlay: &punk_orch::ProjectOverlay) -> String {
    let overlay_ref = overlay.overlay_ref.as_str();
    let bootstrap = overlay.bootstrap_ref.as_deref().unwrap_or("missing");
    let guidance = if overlay.agent_guidance_ref.is_empty() {
        "missing".to_string()
    } else {
        overlay.agent_guidance_ref.join(", ")
    };
    let skills = if overlay.project_skill_refs.is_empty() {
        "none".to_string()
    } else {
        overlay.project_skill_refs.join(", ")
    };
    let ambient_skills = if overlay.ambient_project_skill_refs.is_empty() {
        "none".to_string()
    } else {
        overlay.ambient_project_skill_refs.join(", ")
    };
    let checks = if overlay.safe_default_checks.is_empty() {
        "none".to_string()
    } else {
        overlay.safe_default_checks.join(", ")
    };
    let capability_active_ids = if overlay.capability_resolution.active_ids.is_empty() {
        "none".to_string()
    } else {
        overlay.capability_resolution.active_ids.join(", ")
    };
    let constraints = if overlay.local_constraints.is_empty() {
        "none".to_string()
    } else {
        overlay
            .local_constraints
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "Project: {project_id}\nRepo root: {repo_root}\nProject overlay packet: {overlay_ref}\nCapability index packet: {capability_index_ref}\nVCS mode: {vcs_mode}\nStatus scope: {status_scope_mode}\nBootstrap: {bootstrap}\nGuidance: {guidance}\nProject skill resolution: {project_skill_resolution_mode}\nProject skills: {skills}\nAmbient fallback skills: {ambient_skills}\nSafe default checks: {checks}\nCapabilities:\n  bootstrap_ready={bootstrap_ready}\n  project_guidance_ready={guidance_ready}\n  staged_ready={staged_ready}\n  autonomous_ready={autonomous_ready}\n  jj_ready={jj_ready}\n  proof_ready={proof_ready}\n  active_ids={capability_active_ids}\n  suppressed_count={suppressed_count}\n  conflicted_count={conflicted_count}\n  advisory_count={advisory_count}\nHarness:\n  inspect_ready={inspect_ready}\n  bootable_per_workspace={bootable_per_workspace}\n  ui_legible={ui_legible}\n  logs_legible={logs_legible}\n  metrics_legible={metrics_legible}\n  traces_legible={traces_legible}\nHarness packet: {harness_spec_ref}\n  derivation_source={derivation_source}\n  profiles={profiles}\nLocal constraints:\n{constraints}",
        project_id = overlay.project_id,
        repo_root = overlay.repo_root,
        overlay_ref = overlay_ref,
        capability_index_ref = overlay.capability_resolution.capability_index_ref,
        vcs_mode = overlay.vcs_mode,
        status_scope_mode = overlay.status_scope_mode,
        bootstrap = bootstrap,
        guidance = guidance,
        project_skill_resolution_mode = overlay.project_skill_resolution_mode,
        skills = skills,
        ambient_skills = ambient_skills,
        checks = checks,
        bootstrap_ready = overlay.capability_summary.bootstrap_ready,
        guidance_ready = overlay.capability_summary.project_guidance_ready,
        staged_ready = overlay.capability_summary.staged_ready,
        autonomous_ready = overlay.capability_summary.autonomous_ready,
        jj_ready = overlay.capability_summary.jj_ready,
        proof_ready = overlay.capability_summary.proof_ready,
        capability_active_ids = capability_active_ids,
        suppressed_count = overlay.capability_resolution.suppressed.len(),
        conflicted_count = overlay.capability_resolution.conflicted.len(),
        advisory_count = overlay.capability_resolution.advisory.len(),
        inspect_ready = overlay.harness_summary.inspect_ready,
        bootable_per_workspace = overlay.harness_summary.bootable_per_workspace,
        ui_legible = overlay.harness_summary.ui_legible,
        logs_legible = overlay.harness_summary.logs_legible,
        metrics_legible = overlay.harness_summary.metrics_legible,
        traces_legible = overlay.harness_summary.traces_legible,
        harness_spec_ref = overlay.harness_spec_ref,
        derivation_source = overlay.harness_spec.derivation_source,
        profiles = if overlay.harness_spec.profiles.is_empty() {
            "none".to_string()
        } else {
            overlay
                .harness_spec
                .profiles
                .iter()
                .map(|profile| {
                    format!(
                        "{}({})",
                        profile.name,
                        profile.validation_surfaces.join(", ")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
        },
        constraints = constraints,
    )
}

fn format_work_ledger_summary(
    ledger: &punk_orch::WorkLedgerView,
    latest_proof_harness_evidence: Option<&str>,
) -> String {
    let goal = ledger.goal_ref.as_deref().unwrap_or("missing");
    let contract = ledger.active_contract_ref.as_deref().unwrap_or("none");
    let run = ledger.latest_run_ref.as_deref().unwrap_or("none");
    let receipt = ledger.latest_receipt_ref.as_deref().unwrap_or("none");
    let decision = ledger.latest_decision_ref.as_deref().unwrap_or("none");
    let proof = ledger.latest_proof_ref.as_deref().unwrap_or("none");
    let autonomy = ledger.latest_autonomy_ref.as_deref().unwrap_or("none");
    let autonomy_outcome = ledger.autonomy_outcome.as_deref().unwrap_or("none");
    let recovery_contract = ledger.recovery_contract_ref.as_deref().unwrap_or("none");
    let blocked_reason = ledger.blocked_reason.as_deref().unwrap_or("none");
    let next_action = ledger.next_action.as_deref().unwrap_or("none");
    let next_action_ref = ledger.next_action_ref.as_deref().unwrap_or("none");
    let architecture = format_work_ledger_architecture(ledger.architecture.as_ref());
    let suggested_command = suggested_command_from_action(
        ledger.next_action.as_deref(),
        ledger.next_action_ref.as_deref(),
    )
    .unwrap_or_else(|| "none".to_string());
    let latest_proof_evidence = if ledger.latest_proof_command_evidence_summary.is_empty() {
        "none".to_string()
    } else {
        ledger
            .latest_proof_command_evidence_summary
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let latest_proof_harness_evidence = latest_proof_harness_evidence.unwrap_or("none");
    let recovery_status = if ledger.recovery_contract_ref.is_some() {
        "prepared"
    } else {
        "none"
    };

    format!(
        "Work: {work_id}\nProject: {project_id}\nLifecycle: {lifecycle_state}\nGoal: {goal}\nFeature: {feature_ref}\nContract: {contract}\nRun: {run}\nReceipt: {receipt}\nDecision: {decision}\nProof: {proof}\nArchitecture:\n{architecture}\nLatest proof evidence:\n{latest_proof_evidence}\nLatest proof harness evidence:\n{latest_proof_harness_evidence}\nAutonomy: {autonomy}\nAutonomy outcome: {autonomy_outcome}\nRecovery status: {recovery_status}\nRecovery contract: {recovery_contract}\nBlocked reason: {blocked_reason}\nNext action: {next_action}\nNext action ref: {next_action_ref}\nSuggested command: {suggested_command}\nUpdated at: {updated_at}",
        work_id = ledger.work_id,
        project_id = ledger.project_id,
        lifecycle_state = ledger.lifecycle_state,
        goal = goal,
        feature_ref = ledger.feature_ref,
        contract = contract,
        run = run,
        receipt = receipt,
        decision = decision,
        proof = proof,
        architecture = architecture,
        latest_proof_evidence = latest_proof_evidence,
        latest_proof_harness_evidence = latest_proof_harness_evidence,
        autonomy = autonomy,
        autonomy_outcome = autonomy_outcome,
        recovery_status = recovery_status,
        recovery_contract = recovery_contract,
        blocked_reason = blocked_reason,
        next_action = next_action,
        next_action_ref = next_action_ref,
        suggested_command = suggested_command,
        updated_at = ledger.updated_at,
    )
}

fn format_work_ledger_architecture(
    architecture: Option<&punk_orch::WorkLedgerArchitectureView>,
) -> String {
    let Some(architecture) = architecture else {
        return "  none".to_string();
    };
    let severity = architecture
        .severity
        .as_ref()
        .map(architecture_severity_label)
        .unwrap_or("none");
    let signals = architecture.signals_ref.as_deref().unwrap_or("none");
    let brief = architecture.brief_ref.as_deref().unwrap_or("none");
    let assessment = architecture.assessment_ref.as_deref().unwrap_or("none");
    let assessment_outcome = architecture
        .assessment_outcome
        .as_ref()
        .map(architecture_assessment_outcome_label)
        .unwrap_or("none");
    let trigger_summary = summarize_architecture_reasons(&architecture.trigger_reasons);
    let assessment_summary = summarize_architecture_reasons(&architecture.assessment_reasons);
    let integrity = architecture
        .contract_integrity
        .as_ref()
        .map(|integrity| {
            format!(
                "present review_required={} touched_roots_max={} file_loc_budgets={} forbidden_path_dependencies={}",
                integrity.review_required,
                integrity
                    .touched_roots_max
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                integrity.file_loc_budgets.len(),
                integrity.forbidden_path_dependencies.len(),
            )
        })
        .unwrap_or_else(|| "none".to_string());

    format!(
        "  signals: {signals}\n  severity: {severity}\n  signals summary: {trigger_summary}\n  brief: {brief}\n  contract integrity: {integrity}\n  assessment: {assessment}\n  assessment outcome: {assessment_outcome}\n  assessment summary: {assessment_summary}"
    )
}

fn load_latest_proofpack(
    repo_root: &Path,
    ledger: &punk_orch::WorkLedgerView,
) -> Result<Option<punk_domain::Proofpack>> {
    let Some(proof_ref) = ledger.latest_proof_ref.as_deref() else {
        return Ok(None);
    };
    let proof_path = repo_root.join(proof_ref);
    if !proof_path.exists() {
        return Ok(None);
    }
    let proof: punk_domain::Proofpack = punk_orch::read_json(&proof_path)?;
    Ok(Some(proof))
}

fn summarize_proof_harness_evidence(proof: &punk_domain::Proofpack) -> String {
    let mut lines = Vec::new();
    lines.extend(proof.declared_harness_evidence.iter().map(|item| {
        format!(
            "declared {} [{}]: {}{}",
            item.evidence_type,
            item.profile,
            item.summary,
            format_declared_harness_evidence_target(item.source_ref.as_deref())
        )
    }));
    lines.extend(proof.harness_evidence.iter().map(|item| {
        let target = item
            .artifact_ref
            .as_deref()
            .or(item.source_ref.as_deref())
            .unwrap_or(item.summary.as_str());
        format!(
            "{} {} [{}]: {}",
            item.evidence_type,
            check_status_summary_label(&item.status),
            item.profile,
            target
        )
    }));
    if lines.is_empty() {
        "none".to_string()
    } else {
        lines
            .into_iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn format_declared_harness_evidence_target(source_ref: Option<&str>) -> String {
    source_ref
        .map(|source_ref| format!(" (source: {source_ref})"))
        .unwrap_or_default()
}

fn research_next_command(research: &punk_orch::ResearchInspectView) -> String {
    match research.record.state.as_str() {
        "frozen" => format!(
            "punk research artifact {} --kind note --summary \"<summary>\"",
            research.record.id
        ),
        "gathering" => format!(
            "punk research synthesize {} --outcome <outcome> --summary \"<summary>\"",
            research.record.id
        ),
        "synthesized" => {
            if research.record.outcome.as_deref() == Some("escalate") {
                format!("punk research escalate {}", research.record.id)
            } else {
                format!("punk research complete {}", research.record.id)
            }
        }
        "completed" | "escalated" => {
            if research
                .synthesis
                .as_ref()
                .is_some_and(|synthesis| !synthesis.follow_up_refs.is_empty())
            {
                "terminal state reached; review follow-up refs".to_string()
            } else {
                "terminal state reached; no further research mutation".to_string()
            }
        }
        _ => format!("punk inspect {} --json", research.record.id),
    }
}

fn format_research_summary(research: &punk_orch::ResearchInspectView) -> String {
    let snapshot = &research.packet.repo_snapshot_ref;
    let budget = &research.packet.budget;
    let snapshot_summary = format!(
        "vcs={} head_ref={} dirty={}",
        snapshot
            .vcs
            .as_ref()
            .map(|kind| match kind {
                punk_domain::VcsKind::Jj => "jj",
                punk_domain::VcsKind::Git => "git",
            })
            .unwrap_or("none"),
        snapshot.head_ref.as_deref().unwrap_or("missing"),
        snapshot.dirty
    );
    let subject_ref = research.question.subject_ref.as_deref().unwrap_or("none");
    let constraints = if research.question.constraints.is_empty() {
        "none".to_string()
    } else {
        research
            .question
            .constraints
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let success_criteria = if research.question.success_criteria.is_empty() {
        "none".to_string()
    } else {
        research
            .question
            .success_criteria
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let context_refs = if research.packet.context_refs.is_empty() {
        "none".to_string()
    } else {
        research.packet.context_refs.join(", ")
    };
    let stop_rules = if research.packet.stop_rules.is_empty() {
        "none".to_string()
    } else {
        research
            .packet
            .stop_rules
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let artifact_refs = if research.record.artifact_refs.is_empty() {
        "none".to_string()
    } else {
        research
            .record
            .artifact_refs
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let synthesis_ref = research.record.synthesis_ref.as_deref().unwrap_or("none");
    let synthesis_identity_ref = research
        .record
        .synthesis_ref
        .as_ref()
        .and_then(|_| research.record.synthesis_history_refs.last())
        .map(String::as_str)
        .unwrap_or("none");
    let outcome = research.record.outcome.as_deref().unwrap_or("none");
    let synthesis_summary = research
        .synthesis
        .as_ref()
        .map(|synthesis| synthesis.summary.as_str())
        .unwrap_or("none");
    let supersedes_ref = research
        .synthesis
        .as_ref()
        .and_then(|synthesis| synthesis.supersedes_ref.as_deref())
        .unwrap_or("none");
    let synthesis_artifact_refs = research
        .synthesis
        .as_ref()
        .map(|synthesis| {
            if synthesis.artifact_refs.is_empty() {
                "none".to_string()
            } else {
                synthesis
                    .artifact_refs
                    .iter()
                    .map(|item| format!("- {item}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        })
        .unwrap_or_else(|| "none".to_string());
    let synthesis_history_refs = if research.record.synthesis_history_refs.is_empty() {
        "none".to_string()
    } else {
        research
            .record
            .synthesis_history_refs
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let follow_up_refs = research
        .synthesis
        .as_ref()
        .map(|synthesis| {
            if synthesis.follow_up_refs.is_empty() {
                "none".to_string()
            } else {
                synthesis
                    .follow_up_refs
                    .iter()
                    .map(|item| format!("- {item}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        })
        .unwrap_or_else(|| "none".to_string());
    let invalidation_note = if research.record.state == "gathering"
        && research.record.invalidated_synthesis_ref.is_some()
    {
        "current synthesized view was cleared by a newer artifact".to_string()
    } else {
        "none".to_string()
    };
    let invalidated_synthesis_ref = if research.record.state == "gathering" {
        research
            .record
            .invalidated_synthesis_ref
            .as_deref()
            .unwrap_or("none")
    } else {
        "none"
    };
    let invalidating_artifact_ref = if research.record.state == "gathering" {
        research
            .record
            .invalidation_artifact_ref
            .as_deref()
            .unwrap_or("none")
    } else {
        "none"
    };
    let invalidation_history = if research.record.invalidation_history.is_empty() {
        "none".to_string()
    } else {
        research
            .record
            .invalidation_history
            .iter()
            .map(|entry| {
                format!(
                    "- invalidated={} by={} at={}",
                    entry.invalidated_synthesis_ref,
                    entry.invalidating_artifact_ref,
                    entry.invalidated_at
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let next_command = research_next_command(research);

    format!(
        "Research: {id}\nState: {state}\nKind: {kind}\nQuestion: {question}\nGoal: {goal}\nSubject ref: {subject_ref}\nQuestion ref: {question_ref}\nPacket ref: {packet_ref}\nSnapshot: {snapshot}\nBudget: rounds={max_rounds} worker_slots={max_worker_slots} duration_minutes={max_duration_minutes} artifacts={max_artifacts} cost_usd={max_cost_usd}\nArtifact count: {artifact_count}\nArtifact refs:\n{artifact_refs}\nOutcome: {outcome}\nSynthesis ref: {synthesis_ref}\nSynthesis identity ref: {synthesis_identity_ref}\nSupersedes synthesis ref: {supersedes_ref}\nSynthesis history refs:\n{synthesis_history_refs}\nSynthesis summary: {synthesis_summary}\nSynthesis artifact refs:\n{synthesis_artifact_refs}\nFollow-up refs:\n{follow_up_refs}\nInvalidation note: {invalidation_note}\nInvalidated synthesis ref: {invalidated_synthesis_ref}\nInvalidating artifact ref: {invalidating_artifact_ref}\nInvalidation history:\n{invalidation_history}\nContext refs: {context_refs}\nSuccess criteria:\n{success_criteria}\nConstraints:\n{constraints}\nStop rules:\n{stop_rules}\nOutput schema: {output_schema_ref}\nNext: {next_command}",
        id = research.record.id,
        state = research.record.state,
        kind = research.question.kind,
        question = research.question.question,
        goal = research.question.goal,
        subject_ref = subject_ref,
        question_ref = research.record.question_ref,
        packet_ref = research.record.packet_ref,
        snapshot = snapshot_summary,
        max_rounds = budget.max_rounds,
        max_worker_slots = budget.max_worker_slots,
        max_duration_minutes = budget.max_duration_minutes,
        max_artifacts = budget.max_artifacts,
        max_cost_usd = budget
            .max_cost_usd
            .map(|value| format!("{value:.2}"))
            .unwrap_or_else(|| "none".to_string()),
        artifact_count = research.artifacts.len(),
        artifact_refs = artifact_refs,
        outcome = outcome,
        synthesis_ref = synthesis_ref,
        synthesis_identity_ref = synthesis_identity_ref,
        supersedes_ref = supersedes_ref,
        synthesis_history_refs = synthesis_history_refs,
        synthesis_summary = synthesis_summary,
        synthesis_artifact_refs = synthesis_artifact_refs,
        follow_up_refs = follow_up_refs,
        invalidation_note = invalidation_note,
        invalidated_synthesis_ref = invalidated_synthesis_ref,
        invalidating_artifact_ref = invalidating_artifact_ref,
        invalidation_history = invalidation_history,
        context_refs = context_refs,
        success_criteria = success_criteria,
        constraints = constraints,
        stop_rules = stop_rules,
        output_schema_ref = research.packet.output_schema_ref,
        next_command = next_command,
    )
}

fn format_proofpack_summary(proof: &punk_domain::Proofpack) -> String {
    let run_ref = proof.run_ref.as_deref().unwrap_or("missing");
    let workspace_lineage = proof
        .workspace_lineage
        .as_ref()
        .map(|lineage| {
            let backend = match lineage.backend {
                punk_domain::VcsKind::Jj => "jj",
                punk_domain::VcsKind::Git => "git",
            };
            format!(
                "backend={} workspace={} change_ref={} base_ref={}",
                backend,
                lineage.workspace_ref,
                lineage.change_ref,
                lineage.base_ref.as_deref().unwrap_or("none")
            )
        })
        .unwrap_or_else(|| "missing".to_string());
    let executor_identity = proof
        .executor_identity
        .as_ref()
        .map(|identity| match identity.version.as_deref() {
            Some(version) => format!("{}@{}", identity.name, version),
            None => format!("{}@unknown", identity.name),
        })
        .unwrap_or_else(|| "missing".to_string());
    let reproducibility_claim = proof
        .reproducibility_claim
        .as_ref()
        .map(|claim| format!("{}: {}", claim.level, claim.summary))
        .unwrap_or_else(|| "missing".to_string());
    let environment_digest = proof
        .reproducibility_claim
        .as_ref()
        .and_then(|claim| claim.environment_digest_sha256.as_deref())
        .unwrap_or("missing");
    let claim_limits = proof
        .reproducibility_claim
        .as_ref()
        .map(|claim| {
            if claim.limits.is_empty() {
                "none".to_string()
            } else {
                claim
                    .limits
                    .iter()
                    .map(|item| format!("- {item}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        })
        .unwrap_or_else(|| "none".to_string());
    let command_evidence = if proof.command_evidence.is_empty() {
        "none".to_string()
    } else {
        proof
            .command_evidence
            .iter()
            .map(|item| {
                format!(
                    "- {} {}: {}",
                    item.lane,
                    check_status_summary_label(&item.status),
                    item.command
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let declared_harness_evidence = if proof.declared_harness_evidence.is_empty() {
        "none".to_string()
    } else {
        proof
            .declared_harness_evidence
            .iter()
            .map(|item| {
                format!(
                    "- {} [{}]: {}{}",
                    item.evidence_type,
                    item.profile,
                    item.summary,
                    format_declared_harness_evidence_target(item.source_ref.as_deref())
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let harness_evidence = if proof.harness_evidence.is_empty() {
        "none".to_string()
    } else {
        proof
            .harness_evidence
            .iter()
            .map(|item| {
                let target = item
                    .artifact_ref
                    .as_deref()
                    .or(item.source_ref.as_deref())
                    .unwrap_or(item.summary.as_str());
                format!(
                    "- {} {} [{}]: {}",
                    item.evidence_type,
                    check_status_summary_label(&item.status),
                    item.profile,
                    target
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "Proof: {proof_id}\nRun: {run_id}\nRun record: {run_ref}\nDecision: {decision_id}\nContract: {contract_ref}\nReceipt: {receipt_ref}\nExecutor: {executor_identity}\nWorkspace lineage: {workspace_lineage}\nReproducibility claim: {reproducibility_claim}\nEnvironment digest: {environment_digest}\nClaim limits:\n{claim_limits}\nSummary: {summary}\nCommand evidence:\n{command_evidence}\nDeclared harness evidence:\n{declared_harness_evidence}\nHarness evidence:\n{harness_evidence}",
        proof_id = proof.id,
        run_id = proof.run_id,
        run_ref = run_ref,
        decision_id = proof.decision_id,
        contract_ref = proof.contract_ref,
        receipt_ref = proof.receipt_ref,
        executor_identity = executor_identity,
        workspace_lineage = workspace_lineage,
        reproducibility_claim = reproducibility_claim,
        environment_digest = environment_digest,
        claim_limits = claim_limits,
        summary = proof.summary,
        command_evidence = command_evidence,
        declared_harness_evidence = declared_harness_evidence,
        harness_evidence = harness_evidence,
    )
}

fn format_incident_summary(
    incident: &punk_domain::IncidentRecord,
    next_actions: Option<&IncidentNextActions>,
) -> String {
    let autonomy = incident.autonomy_ref.as_deref().unwrap_or("none");
    let blocked_reason = incident.blocked_reason.as_deref().unwrap_or("none");
    let capture_basis = if incident.capture_basis.is_empty() {
        "none".to_string()
    } else {
        incident
            .capture_basis
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let promote_lines = next_actions
        .map(|actions| {
            let mut rendered = String::new();
            if let Some(target) = actions.promote_target.as_deref() {
                rendered.push_str(&format!("\nPromote target: {target}"));
            }
            if let Some(command) = actions.promote_command.as_deref() {
                rendered.push_str(&format!("\nPromote draft: {command}"));
            }
            if let Some(policy) = actions.auto_run_policy.as_deref() {
                rendered.push_str(&format!("\nAuto-run policy: {policy}"));
            }
            if let Some(command) = actions.auto_run_command.as_deref() {
                rendered.push_str(&format!("\nAuto-run next: {command}"));
            }
            if let Some(setup_hint) = actions.setup_hint.as_deref() {
                rendered.push_str(&format!("\nPromote setup: {setup_hint}"));
            }
            rendered
        })
        .unwrap_or_default();
    format!(
        "Incident: {incident_id}\nKind: {incident_kind}\nProject: {project_id}\nWork: {work_id}\nOutcome: {decision_outcome}\nFailure signature: {failure_signature}\nSummary: {summary}\nBlocked reason: {blocked_reason}\nGoal: {goal}\nContract: {contract_ref}\nRun: {run_ref}\nDecision: {decision_ref}\nProof: {proof_ref}\nAutonomy: {autonomy}\nIssue draft: {issue_draft_ref}\nRepro note: {repro_ref}\nCapture basis:\n{capture_basis}{promote_lines}",
        incident_id = incident.id,
        incident_kind = incident.incident_kind,
        project_id = incident.project_id,
        work_id = incident.work_id,
        decision_outcome = incident.decision_outcome,
        failure_signature = incident.failure_signature,
        summary = incident.summary,
        blocked_reason = blocked_reason,
        goal = incident.goal,
        contract_ref = incident.contract_ref,
        run_ref = incident.run_ref,
        decision_ref = incident.decision_ref,
        proof_ref = incident.proof_ref,
        autonomy = autonomy,
        issue_draft_ref = incident.issue_draft_ref,
        repro_ref = incident.repro_ref,
        capture_basis = capture_basis,
        promote_lines = promote_lines,
    )
}

fn format_incident_promotion_summary(
    promotion: &IncidentPromotionRecord,
    policy: Option<&IncidentAutoRunPolicy>,
) -> String {
    let state = if promotion.execution.is_some() {
        "completed"
    } else if promotion.last_failure.is_some() {
        "failed"
    } else {
        "drafted"
    };
    let attempts = promotion.auto_run_attempts;
    let last_attempt_at = promotion.last_attempt_at.as_deref().unwrap_or("none");
    let failure_lines = promotion
        .last_failure
        .as_ref()
        .map(|failure| {
            let contract_status = failure.contract_status.as_deref().unwrap_or("unknown");
            let run_id = failure.run_id.as_deref().unwrap_or("none");
            let receipt_ref = failure.receipt_ref.as_deref().unwrap_or("none");
            let decision_id = failure.decision_id.as_deref().unwrap_or("none");
            format!(
                "\nLast failure: {phase}: {summary}\nLast failure contract status: {contract_status}\nLast failure run: {run_id}\nLast failure receipt: {receipt_ref}\nLast failure decision: {decision_id}",
                phase = failure.phase,
                summary = failure.summary,
                contract_status = contract_status,
                run_id = run_id,
                receipt_ref = receipt_ref,
                decision_id = decision_id,
            )
        })
        .unwrap_or_else(|| "\nLast failure: none".to_string());
    let execution_lines = promotion
        .execution
        .as_ref()
        .map(|execution| {
            format!(
                "\nRun: {run_id}\nReceipt: {receipt_ref}\nDecision: {decision_outcome}\nProof: {proof_id}\nExecution summary: {receipt_summary}",
                run_id = execution.run_id,
                receipt_ref = execution.receipt_ref,
                decision_outcome = execution.decision_outcome,
                proof_id = execution.proof_id,
                receipt_summary = execution.receipt_summary,
            )
        })
        .unwrap_or_default();
    let auto_run_policy = policy
        .map(|policy| {
            let state = if policy.allowed { "allowed" } else { "blocked" };
            format!("\nAuto-run policy: {state}: {}", policy.detail)
        })
        .unwrap_or_default();
    let next = if let Some(execution) = promotion.execution.as_ref() {
        format!(
            "cd {} && punk inspect {}",
            promotion.target_repo_root, execution.proof_id
        )
    } else if promotion.last_failure.is_some() {
        format!("punk incident rerun {} --auto-run", promotion.id)
    } else {
        format!(
            "cd {} && punk inspect {} --json",
            promotion.target_repo_root, promotion.draft_contract_id
        )
    };
    format!(
        "Promotion: {promotion_id}\nIncident: {incident_id}\nSource project: {source_project_id}\nSource repo: {source_repo_root}\nTarget project: {target_project_id}\nTarget repo: {target_repo_root}\nImported incident: {imported_incident_ref}\nImported issue draft: {imported_issue_draft_ref}\nImported repro note: {imported_repro_ref}\nPrepared goal: {prepared_goal}\nDraft feature: {draft_feature_id}\nDraft contract: {draft_contract_id}\nState: {state}\nAuto-run attempts: {attempts}\nLast attempt at: {last_attempt_at}{auto_run_policy}{failure_lines}{execution_lines}\nNext: {next}",
        promotion_id = promotion.id,
        incident_id = promotion.incident_id,
        source_project_id = promotion.source_project_id,
        source_repo_root = promotion.source_repo_root,
        target_project_id = promotion.target_project_id,
        target_repo_root = promotion.target_repo_root,
        imported_incident_ref = promotion.imported_incident_ref,
        imported_issue_draft_ref = promotion.imported_issue_draft_ref,
        imported_repro_ref = promotion.imported_repro_ref,
        prepared_goal = promotion.prepared_goal,
        draft_feature_id = promotion.draft_feature_id,
        draft_contract_id = promotion.draft_contract_id,
        state = state,
        attempts = attempts,
        last_attempt_at = last_attempt_at,
        auto_run_policy = auto_run_policy,
        failure_lines = failure_lines,
        execution_lines = execution_lines,
        next = next,
    )
}

fn format_incident_submission_summary(
    submission: &punk_domain::IncidentSubmissionRecord,
) -> String {
    let issue_url = submission
        .published_issue_url
        .as_deref()
        .unwrap_or("not published");
    let issue_number = submission
        .published_issue_number
        .map(|value| value.to_string())
        .unwrap_or_else(|| "not published".to_string());
    let publish_error = submission.publish_error.as_deref().unwrap_or("none");
    format!(
        "Submission: {submission_id}\nIncident: {incident_id}\nKind: {submission_kind}\nGitHub repo: {github_repo}\nState: {state}\nIssue title: {issue_title}\nBody snapshot: {body_ref}\nPreview command: {preview_command}\nPublished issue: {issue_url}\nPublished number: {issue_number}\nPublish error: {publish_error}",
        submission_id = submission.id,
        incident_id = submission.incident_id,
        submission_kind = submission.submission_kind,
        github_repo = submission.github_repo,
        state = submission.state,
        issue_title = submission.issue_title,
        body_ref = submission.body_ref,
        preview_command = submission.preview_command,
        issue_url = issue_url,
        issue_number = issue_number,
        publish_error = publish_error,
    )
}

fn format_incident_defaults_summary(label: &str, defaults: &punk_orch::IncidentDefaults) -> String {
    let promote_repo_root = defaults
        .promote_repo_root
        .as_deref()
        .unwrap_or("not configured");
    let github_repo = defaults.github_repo.as_deref().unwrap_or("not configured");
    let updated_at = defaults.updated_at.as_deref().unwrap_or("never");
    format!(
        "{label}\nPromote repo: {promote_repo_root}\nGitHub repo: {github_repo}\nUpdated at: {updated_at}",
        label = label,
        promote_repo_root = promote_repo_root,
        github_repo = github_repo,
        updated_at = updated_at,
    )
}

fn resolve_incident_promote_repo(orch: &OrchService, repo: Option<&Path>) -> Result<PathBuf> {
    if let Some(repo) = repo {
        return Ok(repo.to_path_buf());
    }
    let defaults = orch.inspect_effective_incident_defaults()?;
    let configured = defaults.promote_repo_root.ok_or_else(|| {
        anyhow!(
            "missing promote target repo; pass --repo or configure it with `punk incident defaults --repo /path/to/specpunk` or `punk incident defaults --global --repo /path/to/specpunk`"
        )
    })?;
    Ok(PathBuf::from(configured))
}

fn resolve_incident_submit_repo(orch: &OrchService, github_repo: Option<&str>) -> Result<String> {
    if let Some(github_repo) = github_repo {
        return Ok(github_repo.to_string());
    }
    let defaults = orch.inspect_effective_incident_defaults()?;
    defaults.github_repo.ok_or_else(|| {
        anyhow!(
            "missing GitHub repo; pass --github owner/repo or configure it with `punk incident defaults --github owner/repo` or `punk incident defaults --global --github owner/repo`"
        )
    })
}

fn check_status_summary_label(status: &punk_domain::CheckStatus) -> &'static str {
    match status {
        punk_domain::CheckStatus::Pass => "pass",
        punk_domain::CheckStatus::Fail => "fail",
        punk_domain::CheckStatus::Partial => "partial",
        punk_domain::CheckStatus::Unverified => "unverified",
    }
}

fn suggested_command_from_action(
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

fn resolve_init_project_id(project_root: &Path, explicit_project: Option<&str>) -> Result<String> {
    if let Some(project) = explicit_project
        .map(str::trim)
        .filter(|project| !project.is_empty())
    {
        return Ok(project.to_string());
    }
    infer_project_id(project_root).ok_or_else(|| {
        anyhow!("unable to infer project id from repo root; rerun with `punk init --project <id>`")
    })
}

fn format_init_error(project_id: &str, reason: &str) -> String {
    format!(
        "project init failed for `{project_id}`: {reason}. Run `punk init --project {project_id} --enable-jj --verify` after `git init` if needed, then retry."
    )
}

fn maybe_warn_jj_degraded_mode(repo_root: &PathBuf, command: &Command) {
    if matches!(command, Command::Vcs(_) | Command::Init(_)) {
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
    use punk_adapters::{ContractDrafter, ExecuteInput, ExecuteOutput, Executor};
    use punk_domain::{
        CheckStatus, Decision, DecisionObject, DeterministicStatus, DraftInput, DraftProposal,
        Receipt, ReceiptArtifacts, RefineInput, Run, RunStatus, RunVcs, VcsKind,
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

    fn init_git_repo_with_commit(root: &Path) {
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test User",
                "commit",
                "-m",
                "init",
            ])
            .current_dir(root)
            .output()
            .unwrap();
    }

    struct TestDrafter;

    impl ContractDrafter for TestDrafter {
        fn name(&self) -> &'static str {
            "test-drafter"
        }

        fn draft(&self, input: DraftInput) -> Result<DraftProposal> {
            let scope = input
                .scan
                .candidate_file_scope_paths
                .first()
                .cloned()
                .or_else(|| input.scan.candidate_entry_points.first().cloned())
                .unwrap_or_else(|| "demo.txt".into());
            let check = input
                .scan
                .candidate_integrity_checks
                .first()
                .cloned()
                .unwrap_or_else(|| "true".into());
            Ok(DraftProposal {
                title: "demo contract".into(),
                summary: input.prompt,
                entry_points: vec![scope.clone()],
                import_paths: vec![],
                expected_interfaces: vec!["demo interface".into()],
                behavior_requirements: vec!["implement the requested behavior".into()],
                allowed_scope: vec![scope],
                target_checks: vec![check.clone()],
                integrity_checks: vec![check],
                risk_level: "medium".into(),
            })
        }

        fn refine(&self, input: RefineInput) -> Result<DraftProposal> {
            Ok(input.current)
        }
    }

    struct ScopedWriteExecutor;

    impl Executor for ScopedWriteExecutor {
        fn name(&self) -> &'static str {
            "scoped-write"
        }

        fn execute_contract(&self, input: ExecuteInput) -> Result<ExecuteOutput> {
            let rel = input
                .contract
                .allowed_scope
                .iter()
                .find(|path| !path.starts_with(".punk/"))
                .cloned()
                .or_else(|| {
                    input
                        .contract
                        .entry_points
                        .iter()
                        .find(|path| !path.starts_with(".punk/"))
                        .cloned()
                })
                .or_else(|| {
                    input
                        .contract
                        .allowed_scope
                        .first()
                        .cloned()
                        .or_else(|| input.contract.entry_points.first().cloned())
                })
                .unwrap_or_else(|| "demo.txt".into());
            let path = input.repo_root.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let existing = fs::read_to_string(&path).unwrap_or_default();
            let body = match path.extension().and_then(|ext| ext.to_str()) {
                Some("rs") => format!("{existing}\n// updated by test executor\n").into_bytes(),
                Some("toml") => format!("{existing}\n# updated by test executor\n").into_bytes(),
                _ => format!("{existing}\nok\n").into_bytes(),
            };
            fs::write(path, body)?;
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

    fn prepare_captured_incident_fixture(
        source_root: &Path,
        global_root: &Path,
    ) -> (OrchService, punk_domain::IncidentRecord) {
        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(global_root);
        fs::create_dir_all(source_root.join("docs/product")).unwrap();
        fs::write(
            source_root.join(".gitignore"),
            ".punk/\ntarget/\n.playwright-mcp/\n",
        )
        .unwrap();
        fs::write(
            source_root.join("Cargo.toml"),
            "[package]\nname='source-demo'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        init_git_repo_with_commit(source_root);

        let service = OrchService::new(source_root, global_root).unwrap();
        let contract = service
            .draft_contract(&TestDrafter, "capture runtime incident")
            .unwrap();
        let approved = service.approve_contract(&contract.id).unwrap();
        let (run, _receipt) = service.cut_run(&ScopedWriteExecutor, &approved.id).unwrap();

        let run_dir = source_root.join(".punk/runs").join(&run.id);
        let receipt_path = run_dir.join("receipt.json");
        let mut receipt: Receipt = read_json(&receipt_path).unwrap();
        receipt.status = "failure".into();
        receipt.summary = "controller stalled with no-progress and left no product changes".into();
        write_json(&receipt_path, &receipt).unwrap();

        let decision = punk_domain::DecisionObject {
            id: format!("dec_{}", run.id.trim_start_matches("run_")),
            run_id: run.id.clone(),
            contract_id: approved.id.clone(),
            decision: punk_domain::Decision::Block,
            deterministic_status: punk_domain::DeterministicStatus::Fail,
            target_status: punk_domain::CheckStatus::Pass,
            integrity_status: punk_domain::CheckStatus::Fail,
            confidence_estimate: 0.42,
            decision_basis: vec![
                "no-progress failure after bounded execution".into(),
                "controller should surface a durable incident bundle".into(),
            ],
            contract_ref: format!(".punk/contracts/{}/v1.json", run.feature_id),
            receipt_ref: format!(".punk/runs/{}/receipt.json", run.id),
            check_refs: Vec::new(),
            verification_context_ref: run.verification_context_ref.clone(),
            verification_context_identity: None,
            command_evidence: Vec::new(),
            declared_harness_evidence: Vec::new(),
            harness_evidence: Vec::new(),
            created_at: now_rfc3339(),
        };
        let decision_path = source_root
            .join(".punk/decisions")
            .join(format!("{}.json", decision.id));
        write_json(&decision_path, &decision).unwrap();

        let proof = punk_domain::Proofpack {
            id: format!("proof_{}", decision.id.trim_start_matches("dec_")),
            decision_id: decision.id.clone(),
            run_id: run.id.clone(),
            run_ref: Some(format!(".punk/runs/{}/run.json", run.id)),
            contract_ref: decision.contract_ref.clone(),
            receipt_ref: decision.receipt_ref.clone(),
            decision_ref: format!(".punk/decisions/{}.json", decision.id),
            check_refs: Vec::new(),
            workspace_lineage: Some(run.vcs.clone()),
            verification_context_ref: decision.verification_context_ref.clone(),
            verification_context_identity: None,
            executor_identity: None,
            reproducibility_claim: None,
            command_evidence: Vec::new(),
            declared_harness_evidence: Vec::new(),
            harness_evidence: Vec::new(),
            hashes: Default::default(),
            summary: "proof for incident capture".into(),
            created_at: now_rfc3339(),
        };
        let proof_dir = source_root.join(".punk/proofs").join(&decision.id);
        fs::create_dir_all(&proof_dir).unwrap();
        let proof_path = proof_dir.join("proofpack.json");
        write_json(&proof_path, &proof).unwrap();

        service.record_autonomy_outcome(&proof.id, None).unwrap();
        let incident = service.capture_incident(&proof.id).unwrap();
        (service, incident)
    }

    fn write_specpunk_target_markers(root: &Path) {
        fs::create_dir_all(root.join(".punk")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("crates/specpunk/src")).unwrap();
        fs::create_dir_all(root.join("crates/punk-orch/src")).unwrap();
        fs::create_dir_all(root.join("docs/product")).unwrap();
        let canonical_root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let project = Project {
            id: runtime_project_id(root).unwrap(),
            path: canonical_root.display().to_string(),
            vcs_backend: Some(punk_domain::VcsKind::Git),
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/project.json"), &project).unwrap();
        fs::write(
            root.join("AGENTS.md"),
            "# AI Contributor Guide for specpunk\n\nThis file is for AI agents contributing changes to `specpunk` itself.\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='target-demo'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn runtime_lane() {}\n").unwrap();
        fs::write(root.join("crates/specpunk/src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            root.join("crates/punk-orch/src/lib.rs"),
            "pub fn demo() {}\n",
        )
        .unwrap();
        fs::write(root.join("docs/product/CLI.md"), "# CLI\n").unwrap();
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
            verification_context_ref: None,
            architecture_inputs_ref: None,
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
            verification_context_ref: None,
            verification_context_identity: None,
            command_evidence: vec![],
            declared_harness_evidence: vec![],
            harness_evidence: vec![],
            created_at: "now".into(),
        };
        let status = punk_orch::StatusSnapshot {
            project_id: "proj".into(),
            events_count: 1,
            work_id: Some("feat_1".into()),
            lifecycle_state: Some("accepted".into()),
            autonomy_outcome: None,
            recovery_contract_ref: None,
            blocked_reason: None,
            next_action: Some("inspect_proof".into()),
            next_action_ref: Some("proof_1".into()),
            suggested_command: Some("punk inspect proof_1 --json".into()),
            last_contract_id: Some("ct_1".into()),
            last_run_id: Some("run_1".into()),
            last_decision_id: Some("dec_1".into()),
            vcs_backend: Some(VcsKind::Jj),
            vcs_ref: Some("abc123".into()),
            vcs_dirty: true,
            workspace_root: Some("/repo".into()),
        };
        let rendered = format_gate_run_summary(
            &decision,
            &status,
            &serde_json::json!({"check_refs": []}),
            Path::new("/repo"),
        );
        assert!(rendered.contains("decision Accept for run_1"));
        assert!(rendered.contains("vcs=Some(Jj)"));
        assert!(rendered.contains("ref=Some(\"abc123\")"));
        assert!(rendered.contains("dirty=true"));
    }

    #[test]
    fn contract_architecture_summary_mentions_refs_and_integrity_counts() {
        let root = temp_test_dir("contract-architecture-summary");
        let signals_ref = ".punk/contracts/feat_1/architecture-signals.json";
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        punk_orch::write_json(
            &root.join(signals_ref),
            &punk_domain::ArchitectureSignals {
                contract_id: "ct_1".into(),
                feature_id: "feat_1".into(),
                scope_roots: vec!["src".into()],
                oversized_files: vec![punk_domain::ArchitectureOversizedFile {
                    path: "src/lib.rs".into(),
                    loc: 1300,
                }],
                distinct_scope_roots: 1,
                entry_point_count: 1,
                expected_interface_count: 1,
                import_path_count: 0,
                has_cleanup_obligations: false,
                has_docs_obligations: false,
                has_migration_sensitive_surfaces: false,
                severity: punk_domain::ArchitectureSeverity::Critical,
                trigger_reasons: vec!["oversized file src/lib.rs has 1300 LOC".into()],
                thresholds: punk_domain::ArchitectureThresholds {
                    warn_file_loc: 600,
                    critical_file_loc: 1200,
                    critical_scope_roots: 1,
                    warn_expected_interfaces: 2,
                    warn_import_paths: 5,
                },
                computed_at: "2026-04-12T00:00:00Z".into(),
            },
        )
        .unwrap();

        let persisted = serde_json::json!({
            "architecture_signals_ref": signals_ref,
            "architecture_integrity": {
                "review_required": true,
                "brief_ref": ".punk/contracts/feat_1/architecture-brief.md",
                "touched_roots_max": 1,
                "file_loc_budgets": [{"path": "src/lib.rs", "max_after_loc": 1200}],
                "forbidden_path_dependencies": [{"from_glob": "src/**", "to_glob": "tests/**"}]
            }
        });

        let rendered = format_contract_architecture_summary(&persisted, &root).unwrap();
        assert!(rendered.contains("architecture: critical"));
        assert!(rendered
            .contains("architecture signals: .punk/contracts/feat_1/architecture-signals.json"));
        assert!(
            rendered.contains("architecture brief: .punk/contracts/feat_1/architecture-brief.md")
        );
        assert!(rendered.contains("architecture integrity: review_required=true touched_roots_max=1 file_loc_budgets=1 forbidden_path_dependencies=1"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn status_summary_mentions_work_lifecycle_and_next_action() {
        let snapshot = punk_orch::StatusSnapshot {
            project_id: "proj".into(),
            events_count: 3,
            work_id: Some("feat_1".into()),
            lifecycle_state: Some("accepted".into()),
            autonomy_outcome: Some("blocked".into()),
            recovery_contract_ref: Some(".punk/contracts/feat_1/v2.json".into()),
            blocked_reason: Some("missing trace export".into()),
            next_action: Some("approve_contract".into()),
            next_action_ref: Some("ct_2".into()),
            suggested_command: Some("punk plot approve ct_2".into()),
            last_contract_id: Some("ct_1".into()),
            last_run_id: Some("run_1".into()),
            last_decision_id: Some("dec_1".into()),
            vcs_backend: Some(VcsKind::Jj),
            vcs_ref: Some("abc123".into()),
            vcs_dirty: false,
            workspace_root: Some("/repo".into()),
        };
        let rendered = format_status_summary(&snapshot);
        assert!(rendered.contains("work=Some(\"feat_1\")"));
        assert!(rendered.contains("lifecycle=Some(\"accepted\")"));
        assert!(rendered.contains("autonomy_outcome=Some(\"blocked\")"));
        assert!(rendered.contains("recovery_contract_ref=Some(\".punk/contracts/feat_1/v2.json\")"));
        assert!(rendered.contains("next_action=Some(\"approve_contract\")"));
        assert!(rendered.contains("next_action_ref=Some(\"ct_2\")"));
        assert!(rendered.contains("suggested_command=punk plot approve ct_2"));
    }

    #[test]
    fn stale_gc_report_summary_lists_safe_candidates() {
        let report = punk_orch::StaleGcReport {
            project_id: "proj".into(),
            generated_at: "2026-04-09T12:00:00Z".into(),
            safe_to_archive: vec![punk_orch::StaleArtifactCandidate {
                artifact_kind: "run".into(),
                artifact_id: "run_stale".into(),
                work_id: "feat_1".into(),
                artifact_ref: ".punk/runs/run_stale/run.json".into(),
                reason: "status=running but child_pid 999999 is dead".into(),
                last_progress_at: Some("2020-01-01T00:00:00Z".into()),
                executor_pid: Some(999999),
            }],
            manual_review: Vec::new(),
        };
        let rendered = format_stale_gc_report(&report);
        assert!(rendered.contains("Project: proj"));
        assert!(rendered.contains("Safe to archive:"));
        assert!(rendered.contains("run_stale (run)"));
        assert!(rendered.contains(".punk/runs/run_stale/run.json"));
        assert!(rendered.contains("child_pid 999999 is dead"));
        assert!(rendered.contains("Manual review:\nnone"));
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

        ensure_native_project_bootstrap(&root, "interviewcoach", false, false).unwrap();

        assert!(!needs_project_bootstrap(&root, "interviewcoach"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bootstrap_error_mentions_native_recovery_command() {
        let message = format_bootstrap_error("interviewcoach", "failed to write bootstrap files");
        assert!(message.contains("project bootstrap failed"));
        assert!(message.contains("punk init --project interviewcoach --enable-jj --verify"));
    }

    #[test]
    fn resolve_bootstrap_doc_path_reuses_existing_single_bootstrap_doc() {
        let root = temp_test_dir("bootstrap-resolve");
        let bootstrap = project_bootstrap_file_path(&root, "custom-project");
        fs::create_dir_all(bootstrap.parent().unwrap()).unwrap();
        fs::write(&bootstrap, "core rules\n").unwrap();

        let resolved = resolve_bootstrap_doc_path(&root, "interviewcoach").unwrap();
        assert_eq!(resolved, bootstrap);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn native_bootstrap_writes_project_packet_guidance_and_bootstrap_files() {
        let root = temp_test_dir("native-bootstrap");

        let result = ensure_native_project_bootstrap(&root, "interviewcoach", false, true).unwrap();

        assert_eq!(result.project_label, "interviewcoach");
        assert!(root.join(".punk/project.json").exists());
        assert!(root.join("AGENTS.md").exists());
        assert!(root.join(".punk/AGENT_START.md").exists());
        assert!(root.join(".punk/bootstrap/interviewcoach-core.md").exists());
        assert_eq!(
            result.bootstrap_ref,
            ".punk/bootstrap/interviewcoach-core.md"
        );
        assert_eq!(
            result.agent_guidance_refs,
            vec!["AGENTS.md", ".punk/AGENT_START.md"]
        );

        let agents = fs::read_to_string(root.join("AGENTS.md")).unwrap();
        assert!(agents.contains("punk go --fallback-staged"));
        let bootstrap =
            fs::read_to_string(root.join(".punk/bootstrap/interviewcoach-core.md")).unwrap();
        assert!(bootstrap.contains("Fail closed instead of guessing."));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn merge_default_gitignore_entries_adds_runtime_artifact_ignores_when_missing() {
        let merged = merge_default_gitignore_entries("");
        assert_eq!(merged, ".punk/\ntarget/\n.playwright-mcp/\n");
    }

    #[test]
    fn merge_default_gitignore_entries_preserves_existing_lines_without_duplication() {
        let merged = merge_default_gitignore_entries("node_modules/\n.punk/\n");
        assert_eq!(merged, "node_modules/\n.punk/\ntarget/\n.playwright-mcp/\n");

        let already_covered = merge_default_gitignore_entries("target\n.punk\n.playwright-mcp\n");
        assert_eq!(already_covered, "target\n.punk\n.playwright-mcp\n");
    }

    #[test]
    fn ensure_default_gitignore_coverage_writes_missing_defaults() {
        let root = temp_test_dir("gitignore-defaults");
        fs::write(root.join(".gitignore"), "node_modules/\n").unwrap();

        ensure_default_gitignore_coverage(&root).unwrap();

        let gitignore = fs::read_to_string(root.join(".gitignore")).unwrap();
        assert_eq!(
            gitignore,
            "node_modules/\n.punk/\ntarget/\n.playwright-mcp/\n"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bootstrap_json_mode_supports_start_plot_research_and_incident_commands() {
        let go = Command::Go(GoCommand {
            goal: "ship interview summary".into(),
            fallback_staged: false,
            json: true,
        });
        let start = Command::Start(StartCommand {
            goal: "ship interview summary".into(),
            json: true,
        });
        let plot = Command::Plot(PlotCommand {
            action: PlotAction::Contract {
                prompt: "ship interview summary".into(),
                architecture: ArchitectureCliMode::Auto,
                json: false,
            },
        });
        let research_start = Command::Research(ResearchCommand {
            action: ResearchAction::Start {
                question: "freeze research".into(),
                kind: "architecture".into(),
                goal: "capture packet".into(),
                success_criteria: vec!["packet exists".into()],
                constraints: Vec::new(),
                subject_ref: None,
                context_refs: Vec::new(),
                contract_ref: None,
                receipt_ref: None,
                skill_ref: None,
                eval_ref: None,
                max_rounds: 3,
                max_worker_slots: 5,
                max_duration_minutes: 30,
                max_artifacts: 12,
                max_cost_usd: None,
                json: true,
            },
        });
        let research_artifact = Command::Research(ResearchCommand {
            action: ResearchAction::Artifact {
                research_id: "research_123".into(),
                kind: "note".into(),
                summary: "captured".into(),
                source_ref: None,
                json: false,
            },
        });
        let research_synthesis = Command::Research(ResearchCommand {
            action: ResearchAction::Synthesize {
                research_id: "research_123".into(),
                outcome: "risk_memo".into(),
                summary: "synthesized".into(),
                artifact_refs: vec![
                    ".punk/research/research_123/artifacts/artifact_123.json".into()
                ],
                follow_up_refs: vec!["docs/product/RESEARCH.md".into()],
                replace: false,
                json: true,
            },
        });
        let research_complete = Command::Research(ResearchCommand {
            action: ResearchAction::Complete {
                research_id: "research_123".into(),
                json: false,
            },
        });
        let research_escalate = Command::Research(ResearchCommand {
            action: ResearchAction::Escalate {
                research_id: "research_123".into(),
                json: true,
            },
        });
        let incident = Command::Incident(IncidentCommand {
            action: IncidentAction::Capture {
                proof_id: "proof_123".into(),
                json: true,
            },
        });
        let incident_promote = Command::Incident(IncidentCommand {
            action: IncidentAction::Promote {
                incident_id: "inc_123".into(),
                repo: Some(PathBuf::from("/tmp/specpunk")),
                auto_run: true,
                json: false,
            },
        });
        let incident_rerun = Command::Incident(IncidentCommand {
            action: IncidentAction::Rerun {
                promotion_id: "prom_123".into(),
                auto_run: true,
                json: true,
            },
        });
        let incident_submit = Command::Incident(IncidentCommand {
            action: IncidentAction::Submit {
                incident_id: "inc_123".into(),
                github_repo: Some("heurema/specpunk".into()),
                publish: true,
                json: true,
            },
        });
        let incident_resubmit = Command::Incident(IncidentCommand {
            action: IncidentAction::Resubmit {
                submission_id: "sub_123".into(),
                publish: true,
                json: false,
            },
        });
        let incident_defaults = Command::Incident(IncidentCommand {
            action: IncidentAction::Defaults {
                repo: Some(PathBuf::from("/tmp/specpunk")),
                github_repo: Some("heurema/specpunk".into()),
                global: false,
                json: true,
            },
        });
        let status = Command::Status(StatusCommand {
            id: None,
            json: false,
        });

        assert_eq!(bootstrap_json_mode(&go), Some(true));
        assert_eq!(bootstrap_json_mode(&start), Some(true));
        assert_eq!(bootstrap_json_mode(&plot), Some(false));
        assert_eq!(bootstrap_json_mode(&research_start), Some(true));
        assert_eq!(bootstrap_json_mode(&research_artifact), Some(false));
        assert_eq!(bootstrap_json_mode(&research_synthesis), Some(true));
        assert_eq!(bootstrap_json_mode(&research_complete), Some(false));
        assert_eq!(bootstrap_json_mode(&research_escalate), Some(true));
        assert_eq!(bootstrap_json_mode(&incident), Some(true));
        assert_eq!(bootstrap_json_mode(&incident_promote), Some(false));
        assert_eq!(bootstrap_json_mode(&incident_rerun), Some(true));
        assert_eq!(bootstrap_json_mode(&incident_submit), Some(true));
        assert_eq!(bootstrap_json_mode(&incident_resubmit), Some(false));
        assert_eq!(bootstrap_json_mode(&incident_defaults), Some(true));
        assert_eq!(bootstrap_json_mode(&status), None);
    }

    #[test]
    fn no_vcs_goal_intake_bootstraps_git_for_start() {
        let root = temp_test_dir("start-no-vcs");
        let retry = "punk start \"ship demo\"";
        let note = ensure_vcs_ready_for_goal_intake(&root, "demo", "punk start", retry)
            .expect("no-vcs workspace should auto-init git")
            .expect("auto-init note should be returned");
        assert!(note.contains("initialized a Git repo"));
        assert_ne!(detect_vcs_mode(&root), VcsMode::NoVcs);
        let git_dir = root.join(".git");
        assert!(git_dir.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn no_vcs_goal_intake_bootstraps_git_for_go() {
        let root = temp_test_dir("go-no-vcs");
        let retry = "punk go --fallback-staged \"ship demo\"";
        let note = ensure_vcs_ready_for_goal_intake(&root, "demo", "punk go", retry)
            .expect("no-vcs workspace should auto-init git")
            .expect("auto-init note should be returned");
        assert!(note.contains("git-only"));
        assert_ne!(detect_vcs_mode(&root), VcsMode::NoVcs);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn no_vcs_error_still_mentions_manual_recovery_when_git_init_fails() {
        let rendered = format_goal_intake_no_vcs_error(
            Path::new("/tmp/demo"),
            "demo",
            "punk go",
            "punk go --fallback-staged \"ship demo\"",
            Some("permission denied"),
        );
        assert!(rendered.contains("Automatic `git init` failed: permission denied."));
        assert!(rendered.contains("punk init --project demo --enable-jj --verify"));
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

    #[test]
    fn go_summary_mentions_run_and_follow_up() {
        let rendered = format_go_summary(
            "interviewcoach",
            "add interview feedback summary endpoint",
            "ct_123",
            "run_456",
            "success",
            "implemented bounded change",
            "success",
            "accept",
            "target checks passed; integrity checks passed",
            "proof_789",
            "punk inspect proof_789 --json",
            None,
            None,
            None,
            None,
            None,
        );
        assert!(rendered.contains("Goal: add interview feedback summary endpoint"));
        assert!(rendered.contains("Project: interviewcoach"));
        assert!(rendered.contains("Approved contract: ct_123"));
        assert!(rendered.contains("Run: run_456 (success)"));
        assert!(rendered.contains("Summary: implemented bounded change"));
        assert!(rendered.contains("Outcome: success"));
        assert!(rendered.contains("Gate: accept"));
        assert!(rendered.contains("Basis: target checks passed; integrity checks passed"));
        assert!(rendered.contains("Proof: proof_789"));
        assert!(rendered.contains("Next: punk inspect proof_789 --json"));
    }

    #[test]
    fn go_error_mentions_blocking_decision_and_proof() {
        let rendered = format_go_error(
            &punk_domain::Decision::Block,
            "proof_789",
            "punk inspect proof_789 --json",
            Some("punk start \"retry goal\""),
        );
        assert!(rendered.contains("gate decision block"));
        assert!(rendered.contains("proof: proof_789"));
        assert!(rendered.contains("punk inspect proof_789 --json"));
        assert!(rendered.contains("punk start \"retry goal\""));
    }

    #[test]
    fn project_overlay_summary_mentions_capabilities_and_refs() {
        let overlay = punk_orch::ProjectOverlay {
            project_id: "interviewcoach-e5b92bb854".into(),
            repo_root: "/tmp/interviewcoach".into(),
            overlay_ref: ".punk/project/overlay.json".into(),
            vcs_mode: "jj".into(),
            bootstrap_ref: Some(".punk/bootstrap/interviewcoach-core.md".into()),
            agent_guidance_ref: vec!["AGENTS.md".into(), ".punk/AGENT_START.md".into()],
            capability_summary: punk_orch::ProjectCapabilitySummary {
                bootstrap_ready: true,
                autonomous_ready: true,
                staged_ready: true,
                jj_ready: true,
                proof_ready: true,
                project_guidance_ready: true,
            },
            capability_resolution: punk_orch::ProjectCapabilityResolutionSummary {
                capability_index_ref: ".punk/project/capabilities.json".into(),
                resolution_source: "builtin".into(),
                resolution_mode: "builtin_only_v1".into(),
                active_ids: vec!["rust-cargo".into()],
                active: Vec::new(),
                suppressed: Vec::new(),
                conflicted: Vec::new(),
                advisory: Vec::new(),
            },
            harness_summary: punk_orch::ProjectHarnessSummary {
                inspect_ready: true,
                bootable_per_workspace: true,
                ui_legible: false,
                logs_legible: true,
                metrics_legible: false,
                traces_legible: false,
            },
            harness_spec_ref: ".punk/project/harness.json".into(),
            harness_spec: punk_orch::PersistedHarnessSpec {
                project_id: "interviewcoach-e5b92bb854".into(),
                inspect_ready: true,
                bootable_per_workspace: true,
                capabilities: punk_orch::PersistedHarnessCapabilities {
                    ui_legible: false,
                    logs_legible: true,
                    metrics_legible: false,
                    traces_legible: false,
                },
                profiles: vec![punk_orch::PersistedHarnessProfile {
                    name: "default".into(),
                    validation_surfaces: vec!["command".into(), "log_query".into()],
                    validation_recipes: vec![
                        punk_orch::PersistedHarnessRecipe {
                            kind: "artifact_assertion".into(),
                            path: ".punk/bootstrap/interviewcoach-core.md".into(),
                        },
                        punk_orch::PersistedHarnessRecipe {
                            kind: "artifact_assertion".into(),
                            path: "AGENTS.md".into(),
                        },
                    ],
                }],
                derivation_source: "repo_markers_v1".into(),
                updated_at: "2026-04-03T00:00:00Z".into(),
            },
            project_skill_resolution_mode: "repo_local".into(),
            project_skill_refs: vec![
                ".punk/skills/overlays/implementer/interviewcoach-core.md".into()
            ],
            ambient_project_skill_refs: Vec::new(),
            local_constraints: vec!["none".into()],
            safe_default_checks: vec!["make test".into()],
            status_scope_mode: "project:interviewcoach-e5b92bb854".into(),
            updated_at: "2026-04-03T00:00:00Z".into(),
        };
        let rendered = format_project_overlay_summary(&overlay);
        assert!(rendered.contains("Project: interviewcoach-e5b92bb854"));
        assert!(rendered.contains("Project overlay packet: .punk/project/overlay.json"));
        assert!(rendered.contains("Bootstrap: .punk/bootstrap/interviewcoach-core.md"));
        assert!(rendered.contains("Guidance: AGENTS.md, .punk/AGENT_START.md"));
        assert!(rendered.contains("Project skill resolution: repo_local"));
        assert!(rendered
            .contains("Project skills: .punk/skills/overlays/implementer/interviewcoach-core.md"));
        assert!(rendered.contains("Ambient fallback skills: none"));
        assert!(rendered.contains("Safe default checks: make test"));
        assert!(rendered.contains("autonomous_ready=true"));
        assert!(rendered.contains("Harness:"));
        assert!(rendered.contains("bootable_per_workspace=true"));
        assert!(rendered.contains("logs_legible=true"));
        assert!(rendered.contains("Harness packet: .punk/project/harness.json"));
        assert!(rendered.contains("profiles=default(command, log_query)"));
    }

    #[test]
    fn work_ledger_summary_mentions_state_goal_and_next_action() {
        let ledger = punk_orch::WorkLedgerView {
            project_id: "interviewcoach-e5b92bb854".into(),
            work_id: "feat_123".into(),
            goal_ref: Some("add trace export".into()),
            feature_ref: ".punk/features/feat_123.json".into(),
            active_contract_ref: Some(".punk/contracts/feat_123/v1.json".into()),
            latest_run_ref: Some(".punk/runs/run_456/run.json".into()),
            latest_receipt_ref: Some(".punk/runs/run_456/receipt.json".into()),
            latest_decision_ref: Some(".punk/decisions/dec_456.json".into()),
            latest_proof_ref: Some(".punk/proofs/dec_456/proofpack.json".into()),
            latest_autonomy_ref: Some(".punk/autonomy/feat_123/auto_456.json".into()),
            autonomy_outcome: Some("blocked".into()),
            recovery_contract_ref: Some(".punk/contracts/feat_789/v1.json".into()),
            architecture: Some(punk_orch::WorkLedgerArchitectureView {
                signals_ref: Some(".punk/contracts/feat_123/architecture-signals.json".into()),
                brief_ref: Some(".punk/contracts/feat_123/architecture-brief.md".into()),
                assessment_ref: Some(".punk/runs/run_456/architecture-assessment.json".into()),
                severity: Some(punk_domain::ArchitectureSeverity::Critical),
                trigger_reasons: vec![
                    "oversized file src/lib.rs has 1300 LOC".into(),
                    "slice spans multiple scope roots".into(),
                ],
                assessment_outcome: Some(punk_domain::ArchitectureAssessmentOutcome::Block),
                assessment_reasons: vec!["architecture constraint failed".into()],
                contract_integrity: Some(punk_domain::ContractArchitectureIntegrity {
                    review_required: true,
                    brief_ref: ".punk/contracts/feat_123/architecture-brief.md".into(),
                    touched_roots_max: Some(1),
                    file_loc_budgets: vec![punk_domain::ArchitectureFileLocBudget {
                        path: "src/lib.rs".into(),
                        max_after_loc: 1200,
                    }],
                    forbidden_path_dependencies: vec![
                        punk_domain::ArchitectureForbiddenPathDependency {
                            from_glob: "src/**".into(),
                            to_glob: "tests/**".into(),
                        },
                    ],
                }),
            }),
            lifecycle_state: "accepted".into(),
            blocked_reason: None,
            latest_proof_command_evidence_summary: vec![
                "target pass: cargo test -p specpunk".into(),
                "integrity pass: cargo test --workspace".into(),
            ],
            next_action: Some("inspect_proof".into()),
            next_action_ref: Some("proof_456".into()),
            updated_at: "2026-04-03T00:00:00Z".into(),
        };
        let rendered = format_work_ledger_summary(
            &ledger,
            Some(
                "- declared log_query [default]: declared surface from persisted packet\n- artifact_assertion pass [default]: AGENTS.md",
            ),
        );
        assert!(rendered.contains("Work: feat_123"));
        assert!(rendered.contains("Lifecycle: accepted"));
        assert!(rendered.contains("Goal: add trace export"));
        assert!(rendered.contains("Contract: .punk/contracts/feat_123/v1.json"));
        assert!(rendered.contains("Proof: .punk/proofs/dec_456/proofpack.json"));
        assert!(rendered.contains("Architecture:"));
        assert!(rendered.contains("signals: .punk/contracts/feat_123/architecture-signals.json"));
        assert!(rendered.contains("severity: critical"));
        assert!(rendered.contains("brief: .punk/contracts/feat_123/architecture-brief.md"));
        assert!(rendered.contains("contract integrity: present review_required=true"));
        assert!(rendered.contains("assessment: .punk/runs/run_456/architecture-assessment.json"));
        assert!(rendered.contains("assessment outcome: block"));
        assert!(rendered.contains("Latest proof evidence:"));
        assert!(rendered.contains("- target pass: cargo test -p specpunk"));
        assert!(rendered.contains("Latest proof harness evidence:"));
        assert!(rendered
            .contains("- declared log_query [default]: declared surface from persisted packet"));
        assert!(rendered.contains("- artifact_assertion pass [default]: AGENTS.md"));
        assert!(rendered.contains("Autonomy: .punk/autonomy/feat_123/auto_456.json"));
        assert!(rendered.contains("Autonomy outcome: blocked"));
        assert!(rendered.contains("Recovery status: prepared"));
        assert!(rendered.contains("Recovery contract: .punk/contracts/feat_789/v1.json"));
        assert!(rendered.contains("Next action: inspect_proof"));
        assert!(rendered.contains("Next action ref: proof_456"));
        assert!(rendered.contains("Suggested command: punk inspect proof_456 --json"));
    }

    #[test]
    fn plot_architecture_examples_parse_with_documented_flags() {
        let contract = Cli::try_parse_from([
            "punk",
            "plot",
            "contract",
            "--architecture",
            "on",
            "close architecture merge gap",
        ])
        .unwrap();
        let refine = Cli::try_parse_from([
            "punk",
            "plot",
            "refine",
            "ct_123",
            "keep the scope tighter",
            "--architecture",
            "off",
        ])
        .unwrap();

        match contract.command {
            Command::Plot(PlotCommand {
                action:
                    PlotAction::Contract {
                        architecture,
                        prompt,
                        ..
                    },
            }) => {
                assert_eq!(architecture, ArchitectureCliMode::On);
                assert_eq!(prompt, "close architecture merge gap");
            }
            _ => panic!("expected plot contract command"),
        }

        match refine.command {
            Command::Plot(PlotCommand {
                action:
                    PlotAction::Refine {
                        contract_id,
                        guidance,
                        architecture,
                        ..
                    },
            }) => {
                assert_eq!(contract_id, "ct_123");
                assert_eq!(guidance, "keep the scope tighter");
                assert_eq!(architecture, ArchitectureCliMode::Off);
            }
            _ => panic!("expected plot refine command"),
        }
    }

    #[test]
    fn incident_capture_command_parses_with_proof_id() {
        let cli = Cli::try_parse_from(["punk", "incident", "capture", "proof_123"]).unwrap();
        match cli.command {
            Command::Incident(IncidentCommand {
                action: IncidentAction::Capture { proof_id, json },
            }) => {
                assert_eq!(proof_id, "proof_123");
                assert!(!json);
            }
            _ => panic!("expected incident capture command"),
        }
    }

    #[test]
    fn incident_promote_command_parses_with_target_repo() {
        let cli = Cli::try_parse_from([
            "punk",
            "incident",
            "promote",
            "inc_123",
            "--repo",
            "/tmp/specpunk",
            "--auto-run",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Command::Incident(IncidentCommand {
                action:
                    IncidentAction::Promote {
                        incident_id,
                        repo,
                        auto_run,
                        json,
                    },
            }) => {
                assert_eq!(incident_id, "inc_123");
                assert_eq!(repo, Some(PathBuf::from("/tmp/specpunk")));
                assert!(auto_run);
                assert!(json);
            }
            _ => panic!("expected incident promote command"),
        }
    }

    #[test]
    fn incident_submit_command_parses_with_github_repo_and_publish() {
        let cli = Cli::try_parse_from([
            "punk",
            "incident",
            "submit",
            "inc_123",
            "--github",
            "heurema/specpunk",
            "--publish",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Command::Incident(IncidentCommand {
                action:
                    IncidentAction::Submit {
                        incident_id,
                        github_repo,
                        publish,
                        json,
                    },
            }) => {
                assert_eq!(incident_id, "inc_123");
                assert_eq!(github_repo.as_deref(), Some("heurema/specpunk"));
                assert!(publish);
                assert!(json);
            }
            _ => panic!("expected incident submit command"),
        }
    }

    #[test]
    fn incident_rerun_command_parses_with_auto_run_flag() {
        let cli = Cli::try_parse_from([
            "punk",
            "incident",
            "rerun",
            "prom_123",
            "--auto-run",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Command::Incident(IncidentCommand {
                action:
                    IncidentAction::Rerun {
                        promotion_id,
                        auto_run,
                        json,
                    },
            }) => {
                assert_eq!(promotion_id, "prom_123");
                assert!(auto_run);
                assert!(json);
            }
            _ => panic!("expected incident rerun command"),
        }
    }

    #[test]
    fn incident_resubmit_command_parses_with_publish_flag() {
        let cli = Cli::try_parse_from([
            "punk",
            "incident",
            "resubmit",
            "sub_123",
            "--publish",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Command::Incident(IncidentCommand {
                action:
                    IncidentAction::Resubmit {
                        submission_id,
                        publish,
                        json,
                    },
            }) => {
                assert_eq!(submission_id, "sub_123");
                assert!(publish);
                assert!(json);
            }
            _ => panic!("expected incident resubmit command"),
        }
    }

    #[test]
    fn incident_defaults_command_parses_repo_and_github_flags() {
        let cli = Cli::try_parse_from([
            "punk",
            "incident",
            "defaults",
            "--repo",
            "/tmp/specpunk",
            "--github",
            "heurema/specpunk",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Command::Incident(IncidentCommand {
                action:
                    IncidentAction::Defaults {
                        repo,
                        github_repo,
                        global,
                        json,
                    },
            }) => {
                assert_eq!(repo, Some(PathBuf::from("/tmp/specpunk")));
                assert_eq!(github_repo.as_deref(), Some("heurema/specpunk"));
                assert!(!global);
                assert!(json);
            }
            _ => panic!("expected incident defaults command"),
        }
    }

    #[test]
    fn incident_defaults_command_parses_global_repo_and_github_flags() {
        let cli = Cli::try_parse_from([
            "punk",
            "incident",
            "defaults",
            "--global",
            "--repo",
            "/tmp/specpunk",
            "--github",
            "heurema/specpunk",
        ])
        .unwrap();
        match cli.command {
            Command::Incident(IncidentCommand {
                action:
                    IncidentAction::Defaults {
                        repo,
                        github_repo,
                        global,
                        json,
                    },
            }) => {
                assert_eq!(repo, Some(PathBuf::from("/tmp/specpunk")));
                assert_eq!(github_repo.as_deref(), Some("heurema/specpunk"));
                assert!(global);
                assert!(!json);
            }
            _ => panic!("expected global incident defaults command"),
        }
    }

    #[test]
    fn incident_rerun_reuses_existing_promotion_after_partial_auto_run_failure() {
        let source_root = temp_test_dir("incident-rerun-source");
        let target_root = temp_test_dir("incident-rerun-target");
        let global_root = temp_test_dir("incident-rerun-global");

        let (source_service, incident) =
            prepare_captured_incident_fixture(&source_root, &global_root);

        write_specpunk_target_markers(&target_root);
        fs::write(
            target_root.join(".gitignore"),
            ".punk/\ntarget/\n.playwright-mcp/\n",
        )
        .unwrap();
        init_git_repo_with_commit(&target_root);

        let promotion = source_service
            .promote_incident(&TestDrafter, &incident.id, &target_root)
            .unwrap();

        let target_service = OrchService::new(&target_root, &global_root).unwrap();
        let approved = target_service
            .approve_contract(&promotion.draft_contract_id)
            .unwrap();
        assert_eq!(approved.status, punk_domain::ContractStatus::Approved);

        let rerun = auto_run_existing_incident_promotion(
            &source_service,
            &global_root,
            &ScopedWriteExecutor,
            &promotion,
        )
        .unwrap();
        assert!(
            go_decision_succeeds(&rerun.decision.decision),
            "decision={:?} basis={:?}",
            rerun.decision.decision,
            rerun.decision.decision_basis
        );
        assert_eq!(rerun.promotion.id, promotion.id);
        let execution = rerun
            .promotion
            .execution
            .expect("execution should be recorded");
        assert_eq!(execution.decision_id, rerun.decision.id);
        assert_eq!(execution.proof_id, rerun.proof.id);

        let updated_target = target_service
            .inspect_incident_promotion(&promotion.id)
            .unwrap();
        assert_eq!(
            updated_target
                .execution
                .as_ref()
                .map(|value| value.proof_id.as_str()),
            Some(rerun.proof.id.as_str())
        );

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&target_root);
        let _ = fs::remove_dir_all(&global_root);
    }

    #[test]
    fn incident_auto_run_records_failure_metadata_on_contract_readiness_error() {
        let source_root = temp_test_dir("incident-rerun-failure-source");
        let target_root = temp_test_dir("incident-rerun-failure-target");
        let global_root = temp_test_dir("incident-rerun-failure-global");

        let (source_service, incident) =
            prepare_captured_incident_fixture(&source_root, &global_root);

        write_specpunk_target_markers(&target_root);
        fs::write(
            target_root.join(".gitignore"),
            ".punk/\ntarget/\n.playwright-mcp/\n",
        )
        .unwrap();
        init_git_repo_with_commit(&target_root);

        let promotion = source_service
            .promote_incident(&TestDrafter, &incident.id, &target_root)
            .unwrap();

        let contract_path = target_root
            .join(".punk/contracts")
            .join(&promotion.draft_feature_id)
            .join("v1.json");
        fs::remove_file(&contract_path).unwrap();

        let error = auto_run_existing_incident_promotion(
            &source_service,
            &global_root,
            &ScopedWriteExecutor,
            &promotion,
        )
        .unwrap_err();
        let rendered = error.to_string();
        assert!(rendered.contains("punk incident rerun prom_"));

        let updated = source_service
            .inspect_incident_promotion(&promotion.id)
            .unwrap();
        assert_eq!(updated.auto_run_attempts, 1);
        let failure = updated
            .last_failure
            .expect("failure metadata should be persisted");
        assert_eq!(failure.phase, "contract_ready");
        assert_eq!(failure.contract_status.as_deref(), None);
        assert!(failure.summary.contains(&promotion.draft_contract_id));

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&target_root);
        let _ = fs::remove_dir_all(&global_root);
    }

    #[test]
    fn proofpack_summary_mentions_command_declared_and_harness_evidence() {
        let proof = punk_domain::Proofpack {
            id: "proof_789".into(),
            decision_id: "dec_789".into(),
            run_id: "run_789".into(),
            run_ref: Some(".punk/runs/run_789/run.json".into()),
            contract_ref: ".punk/contracts/feat_789/v1.json".into(),
            receipt_ref: ".punk/runs/run_789/receipt.json".into(),
            decision_ref: ".punk/decisions/dec_789.json".into(),
            check_refs: vec![],
            workspace_lineage: Some(punk_domain::RunVcs {
                backend: punk_domain::VcsKind::Git,
                workspace_ref: "/tmp/specpunk-worktree".into(),
                change_ref: "HEAD".into(),
                base_ref: Some("HEAD~1".into()),
            }),
            verification_context_ref: None,
            verification_context_identity: None,
            executor_identity: Some(punk_domain::ProofExecutorIdentity {
                name: "codex-cli".into(),
                version: None,
            }),
            reproducibility_claim: Some(punk_domain::ProofReproducibilityClaim {
                level: "run_record_v0".into(),
                summary: "Proof records run lineage and executor identity, but lacks a frozen verification-context digest.".into(),
                environment_digest_sha256: None,
                limits: vec![
                    "v0 proof records verdict context and evidence but does not guarantee hermetic rebuilds".into(),
                    "executor version is unavailable in the current receipt schema".into(),
                    "frozen verification context identity is unavailable".into(),
                ],
            }),
            command_evidence: vec![
                punk_domain::CommandEvidence {
                    evidence_type: "command".into(),
                    lane: "target".into(),
                    command: "cargo test -p specpunk".into(),
                    status: punk_domain::CheckStatus::Pass,
                    summary: "target check passed".into(),
                    stdout_ref: Some(".punk/runs/run_789/checks/target-01.stdout.log".into()),
                    stderr_ref: Some(".punk/runs/run_789/checks/target-01.stderr.log".into()),
                },
                punk_domain::CommandEvidence {
                    evidence_type: "command".into(),
                    lane: "integrity".into(),
                    command: "cargo test --workspace".into(),
                    status: punk_domain::CheckStatus::Pass,
                    summary: "integrity check passed".into(),
                    stdout_ref: Some(".punk/runs/run_789/checks/integrity-01.stdout.log".into()),
                    stderr_ref: Some(".punk/runs/run_789/checks/integrity-01.stderr.log".into()),
                },
            ],
            declared_harness_evidence: vec![punk_domain::DeclaredHarnessEvidence {
                evidence_type: "log_query".into(),
                profile: "default".into(),
                source_ref: Some(".punk/project/harness.json".into()),
                summary: "declared non-command harness surface from persisted packet".into(),
            }],
            harness_evidence: vec![
                punk_domain::HarnessEvidence {
                    evidence_type: "artifact_assertion".into(),
                    profile: "default".into(),
                    status: punk_domain::CheckStatus::Pass,
                    summary: "artifact exists".into(),
                    source_ref: Some(".punk/project/harness.json".into()),
                    artifact_ref: Some("AGENTS.md".into()),
                },
                punk_domain::HarnessEvidence {
                    evidence_type: "artifact_assertion".into(),
                    profile: "default".into(),
                    status: punk_domain::CheckStatus::Pass,
                    summary: "artifact exists".into(),
                    source_ref: Some(".punk/project/harness.json".into()),
                    artifact_ref: Some(".punk/bootstrap/specpunk-core.md".into()),
                },
            ],
            hashes: Default::default(),
            summary: "proof for dec_789".into(),
            created_at: "2026-04-08T00:00:00Z".into(),
        };

        let rendered = format_proofpack_summary(&proof);
        assert!(rendered.contains("Proof: proof_789"));
        assert!(rendered.contains("Run record: .punk/runs/run_789/run.json"));
        assert!(rendered.contains("Executor: codex-cli@unknown"));
        assert!(rendered.contains(
            "Workspace lineage: backend=git workspace=/tmp/specpunk-worktree change_ref=HEAD base_ref=HEAD~1"
        ));
        assert!(rendered.contains("Reproducibility claim: run_record_v0:"));
        assert!(rendered.contains("Environment digest: missing"));
        assert!(rendered.contains("Claim limits:"));
        assert!(rendered.contains("Command evidence:"));
        assert!(rendered.contains("- target pass: cargo test -p specpunk"));
        assert!(rendered.contains("- integrity pass: cargo test --workspace"));
        assert!(rendered.contains("Declared harness evidence:"));
        assert!(rendered.contains(
            "- log_query [default]: declared non-command harness surface from persisted packet"
        ));
        assert!(rendered.contains("Harness evidence:"));
        assert!(rendered.contains("- artifact_assertion pass [default]: AGENTS.md"));
        assert!(rendered
            .contains("- artifact_assertion pass [default]: .punk/bootstrap/specpunk-core.md"));
    }

    #[test]
    fn incident_summary_mentions_bundle_and_refs() {
        let incident = punk_domain::IncidentRecord {
            id: "inc_123".into(),
            project_id: "specpunk".into(),
            repo_root: "/tmp/specpunk".into(),
            work_id: "feat_123".into(),
            goal: "capture runtime bug".into(),
            contract_ref: ".punk/contracts/feat_123/v1.json".into(),
            run_ref: ".punk/runs/run_123/run.json".into(),
            decision_ref: ".punk/decisions/dec_123.json".into(),
            proof_ref: ".punk/proofs/dec_123/proofpack.json".into(),
            autonomy_ref: Some(".punk/autonomy/feat_123/auto_123.json".into()),
            incident_kind: "suspected_runtime_bug".into(),
            decision_outcome: "blocked".into(),
            summary: "controller stalled with no-progress".into(),
            blocked_reason: Some("no-progress failure".into()),
            failure_signature: "blocked:no-progress".into(),
            capture_basis: vec![
                "decision outcome: blocked".into(),
                "matched runtime marker: no-progress".into(),
            ],
            issue_draft_ref: ".punk/incidents/feat_123/inc_123/issue.md".into(),
            repro_ref: ".punk/incidents/feat_123/inc_123/repro.md".into(),
            created_at: "2026-04-16T00:00:00Z".into(),
        };

        let rendered = format_incident_summary(&incident, None);
        assert!(rendered.contains("Incident: inc_123"));
        assert!(rendered.contains("Kind: suspected_runtime_bug"));
        assert!(rendered.contains("Failure signature: blocked:no-progress"));
        assert!(rendered.contains("Issue draft: .punk/incidents/feat_123/inc_123/issue.md"));
        assert!(rendered.contains("Repro note: .punk/incidents/feat_123/inc_123/repro.md"));
        assert!(rendered.contains("Capture basis:"));
        assert!(rendered.contains("- matched runtime marker: no-progress"));
    }

    #[test]
    fn incident_promotion_summary_mentions_target_contract_and_bundle() {
        let promotion = IncidentPromotionRecord {
            id: "prom_123".into(),
            incident_id: "inc_123".into(),
            source_project_id: "foreign-demo".into(),
            source_repo_root: "/tmp/source".into(),
            source_incident_ref: ".punk/incidents/feat_123/inc_123/incident.json".into(),
            source_issue_draft_ref: ".punk/incidents/feat_123/inc_123/issue.md".into(),
            source_repro_ref: ".punk/incidents/feat_123/inc_123/repro.md".into(),
            target_project_id: "specpunk".into(),
            target_repo_root: "/tmp/specpunk".into(),
            imported_incident_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/incident.json".into(),
            imported_issue_draft_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/issue.md".into(),
            imported_repro_ref: ".punk/imported-incidents/foreign-demo/inc_123/prom_123/repro.md"
                .into(),
            prepared_goal: "Investigate and fix promoted incident inc_123".into(),
            draft_feature_id: "feat_456".into(),
            draft_contract_id: "ct_456_v1".into(),
            auto_run_attempts: 0,
            last_attempt_at: None,
            last_failure: None,
            execution: None,
            created_at: "2026-04-16T00:30:00Z".into(),
        };

        let rendered = format_incident_promotion_summary(&promotion, None);
        assert!(rendered.contains("Promotion: prom_123"));
        assert!(rendered.contains("Target repo: /tmp/specpunk"));
        assert!(rendered.contains("Draft contract: ct_456_v1"));
        assert!(rendered.contains("State: drafted"));
        assert!(rendered.contains("Auto-run attempts: 0"));
        assert!(rendered.contains("Last failure: none"));
        assert!(rendered.contains(
            "Imported incident: .punk/imported-incidents/foreign-demo/inc_123/prom_123/incident.json"
        ));
        assert!(rendered.contains("Next: cd /tmp/specpunk && punk inspect ct_456_v1 --json"));
    }

    #[test]
    fn incident_promotion_summary_mentions_auto_run_execution_when_present() {
        let promotion = IncidentPromotionRecord {
            id: "prom_123".into(),
            incident_id: "inc_123".into(),
            source_project_id: "foreign-demo".into(),
            source_repo_root: "/tmp/source".into(),
            source_incident_ref: ".punk/incidents/feat_123/inc_123/incident.json".into(),
            source_issue_draft_ref: ".punk/incidents/feat_123/inc_123/issue.md".into(),
            source_repro_ref: ".punk/incidents/feat_123/inc_123/repro.md".into(),
            target_project_id: "specpunk".into(),
            target_repo_root: "/tmp/specpunk".into(),
            imported_incident_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/incident.json".into(),
            imported_issue_draft_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/issue.md".into(),
            imported_repro_ref: ".punk/imported-incidents/foreign-demo/inc_123/prom_123/repro.md"
                .into(),
            prepared_goal: "Investigate and fix promoted incident inc_123".into(),
            draft_feature_id: "feat_456".into(),
            draft_contract_id: "ct_456_v1".into(),
            auto_run_attempts: 2,
            last_attempt_at: Some("2026-04-16T00:45:00Z".into()),
            last_failure: None,
            execution: Some(IncidentPromotionExecution {
                run_id: "run_456".into(),
                receipt_ref: ".punk/runs/run_456/receipt.json".into(),
                decision_id: "dec_456".into(),
                proof_id: "proof_456".into(),
                decision_outcome: "accept".into(),
                receipt_summary: "bounded fix applied and checks passed".into(),
                completed_at: "2026-04-16T00:45:00Z".into(),
            }),
            created_at: "2026-04-16T00:30:00Z".into(),
        };

        let rendered = format_incident_promotion_summary(&promotion, None);
        assert!(rendered.contains("State: completed"));
        assert!(rendered.contains("Auto-run attempts: 2"));
        assert!(rendered.contains("Run: run_456"));
        assert!(rendered.contains("Decision: accept"));
        assert!(rendered.contains("Proof: proof_456"));
        assert!(rendered.contains("Next: cd /tmp/specpunk && punk inspect proof_456"));
    }

    #[test]
    fn incident_promotion_summary_mentions_auto_run_policy_when_provided() {
        let promotion = IncidentPromotionRecord {
            id: "prom_123".into(),
            incident_id: "inc_123".into(),
            source_project_id: "foreign-demo".into(),
            source_repo_root: "/tmp/source".into(),
            source_incident_ref: ".punk/incidents/feat_123/inc_123/incident.json".into(),
            source_issue_draft_ref: ".punk/incidents/feat_123/inc_123/issue.md".into(),
            source_repro_ref: ".punk/incidents/feat_123/inc_123/repro.md".into(),
            target_project_id: "specpunk".into(),
            target_repo_root: "/tmp/specpunk".into(),
            imported_incident_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/incident.json".into(),
            imported_issue_draft_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/issue.md".into(),
            imported_repro_ref: ".punk/imported-incidents/foreign-demo/inc_123/prom_123/repro.md"
                .into(),
            prepared_goal: "Investigate and fix promoted incident inc_123".into(),
            draft_feature_id: "feat_456".into(),
            draft_contract_id: "ct_456_v1".into(),
            auto_run_attempts: 0,
            last_attempt_at: None,
            last_failure: None,
            execution: None,
            created_at: "2026-04-16T00:30:00Z".into(),
        };
        let policy = IncidentAutoRunPolicy {
            allowed: true,
            detail: "target repo identity matches specpunk upstream markers".into(),
        };

        let rendered = format_incident_promotion_summary(&promotion, Some(&policy));
        assert!(rendered.contains(
            "Auto-run policy: allowed: target repo identity matches specpunk upstream markers"
        ));
    }

    #[test]
    fn incident_promotion_summary_mentions_last_failure_and_rerun_next_step() {
        let promotion = IncidentPromotionRecord {
            id: "prom_123".into(),
            incident_id: "inc_123".into(),
            source_project_id: "foreign-demo".into(),
            source_repo_root: "/tmp/source".into(),
            source_incident_ref: ".punk/incidents/feat_123/inc_123/incident.json".into(),
            source_issue_draft_ref: ".punk/incidents/feat_123/inc_123/issue.md".into(),
            source_repro_ref: ".punk/incidents/feat_123/inc_123/repro.md".into(),
            target_project_id: "specpunk".into(),
            target_repo_root: "/tmp/specpunk".into(),
            imported_incident_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/incident.json".into(),
            imported_issue_draft_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/issue.md".into(),
            imported_repro_ref: ".punk/imported-incidents/foreign-demo/inc_123/prom_123/repro.md"
                .into(),
            prepared_goal: "Investigate and fix promoted incident inc_123".into(),
            draft_feature_id: "feat_456".into(),
            draft_contract_id: "ct_456_v1".into(),
            auto_run_attempts: 1,
            last_attempt_at: Some("2026-04-16T00:40:00Z".into()),
            last_failure: Some(IncidentPromotionFailure {
                phase: "proof_write".into(),
                summary: "proofpack write failed after gate output".into(),
                contract_status: Some("approved".into()),
                run_id: Some("run_456".into()),
                receipt_ref: Some(".punk/runs/run_456/receipt.json".into()),
                decision_id: Some("dec_456".into()),
                failed_at: "2026-04-16T00:40:00Z".into(),
            }),
            execution: None,
            created_at: "2026-04-16T00:30:00Z".into(),
        };

        let rendered = format_incident_promotion_summary(&promotion, None);
        assert!(rendered.contains("State: failed"));
        assert!(rendered.contains("Auto-run attempts: 1"));
        assert!(rendered
            .contains("Last failure: proof_write: proofpack write failed after gate output"));
        assert!(rendered.contains("Last failure run: run_456"));
        assert!(rendered.contains("Last failure decision: dec_456"));
        assert!(rendered.contains("Next: punk incident rerun prom_123 --auto-run"));
    }

    #[test]
    fn incident_auto_run_policy_allows_specpunk_like_target_repo() {
        let root = temp_test_dir("incident-auto-run-policy-allowed");
        write_specpunk_target_markers(&root);

        let policy = inspect_incident_auto_run_policy(&root, None);
        assert!(policy.allowed);
        assert!(policy
            .detail
            .contains("identity matches specpunk upstream markers"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn incident_auto_run_policy_blocks_mismatched_project_identity() {
        let root = temp_test_dir("incident-auto-run-policy-mismatch");
        write_specpunk_target_markers(&root);

        let policy = inspect_incident_auto_run_policy(&root, Some("specpunk-other"));
        assert!(!policy.allowed);
        assert!(policy
            .detail
            .contains("does not match the promoted target id specpunk-other"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn incident_auto_run_policy_blocks_non_specpunk_target_repo() {
        let root = temp_test_dir("incident-auto-run-policy-blocked");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();

        let policy = inspect_incident_auto_run_policy(&root, None);
        assert!(!policy.allowed);
        assert!(policy
            .detail
            .contains("missing a readable .punk/project.json identity packet"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn incident_summary_mentions_auto_run_next_when_defaults_target_is_eligible() {
        let source_root = temp_test_dir("incident-summary-auto-run-source");
        let global_root = temp_test_dir("incident-summary-auto-run-global");
        let target_root = temp_test_dir("incident-summary-auto-run-target");
        write_specpunk_target_markers(&target_root);
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&source_root)
            .output()
            .unwrap();

        let (orch, incident) = prepare_captured_incident_fixture(&source_root, &global_root);
        orch.set_global_incident_defaults(Some(&target_root), None)
            .unwrap();

        let next_actions = derive_incident_next_actions(&orch, &incident).unwrap();
        let rendered = format_incident_summary(&incident, Some(&next_actions));

        assert!(rendered.contains("Promote draft: punk incident promote inc_"));
        assert!(rendered.contains(
            "Auto-run policy: allowed: target repo identity matches specpunk upstream markers"
        ));
        assert!(rendered.contains("Auto-run next: punk incident promote inc_"));
        assert!(rendered.contains("--auto-run"));

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&global_root);
        let _ = fs::remove_dir_all(&target_root);
    }

    #[test]
    fn incident_summary_mentions_auto_run_policy_block_when_defaults_target_is_not_eligible() {
        let source_root = temp_test_dir("incident-summary-policy-source");
        let global_root = temp_test_dir("incident-summary-policy-global");
        let target_root = temp_test_dir("incident-summary-policy-target");
        fs::create_dir_all(target_root.join("src")).unwrap();
        fs::write(
            target_root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&source_root)
            .output()
            .unwrap();

        let (orch, incident) = prepare_captured_incident_fixture(&source_root, &global_root);
        orch.set_global_incident_defaults(Some(&target_root), None)
            .unwrap();

        let next_actions = derive_incident_next_actions(&orch, &incident).unwrap();
        let rendered = format_incident_summary(&incident, Some(&next_actions));

        assert!(rendered.contains("Promote draft: punk incident promote inc_"));
        assert!(rendered.contains(
            "Auto-run policy: blocked: target repo is missing a readable .punk/project.json identity packet"
        ));
        assert!(!rendered.contains("Auto-run next:"));

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&global_root);
        let _ = fs::remove_dir_all(&target_root);
    }

    #[test]
    fn incident_auto_run_policy_error_mentions_promotion_inspect_and_contract_review() {
        let promotion = IncidentPromotionRecord {
            id: "prom_123".into(),
            incident_id: "inc_123".into(),
            source_project_id: "foreign-demo".into(),
            source_repo_root: "/tmp/source".into(),
            source_incident_ref: ".punk/incidents/feat_123/inc_123/incident.json".into(),
            source_issue_draft_ref: ".punk/incidents/feat_123/inc_123/issue.md".into(),
            source_repro_ref: ".punk/incidents/feat_123/inc_123/repro.md".into(),
            target_project_id: "some-target".into(),
            target_repo_root: "/tmp/not-specpunk".into(),
            imported_incident_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/incident.json".into(),
            imported_issue_draft_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/issue.md".into(),
            imported_repro_ref: ".punk/imported-incidents/foreign-demo/inc_123/prom_123/repro.md"
                .into(),
            prepared_goal: "Investigate and fix promoted incident inc_123".into(),
            draft_feature_id: "feat_456".into(),
            draft_contract_id: "ct_456_v1".into(),
            auto_run_attempts: 0,
            last_attempt_at: None,
            last_failure: None,
            execution: None,
            created_at: "2026-04-16T00:30:00Z".into(),
        };
        let policy = IncidentAutoRunPolicy {
            allowed: false,
            detail: "target repo identity packet does not match the promoted target id some-target"
                .into(),
        };

        let rendered = format_incident_auto_run_policy_error(&promotion, &policy);
        assert!(rendered.contains("auto-run is blocked by policy"));
        assert!(rendered.contains("punk inspect prom_123"));
        assert!(rendered.contains("cd /tmp/not-specpunk && punk inspect ct_456_v1 --json"));
    }

    #[test]
    fn incident_auto_run_failure_message_mentions_rerun_command() {
        let promotion = IncidentPromotionRecord {
            id: "prom_123".into(),
            incident_id: "inc_123".into(),
            source_project_id: "foreign-demo".into(),
            source_repo_root: "/tmp/source".into(),
            source_incident_ref: ".punk/incidents/feat_123/inc_123/incident.json".into(),
            source_issue_draft_ref: ".punk/incidents/feat_123/inc_123/issue.md".into(),
            source_repro_ref: ".punk/incidents/feat_123/inc_123/repro.md".into(),
            target_project_id: "specpunk".into(),
            target_repo_root: "/tmp/specpunk".into(),
            imported_incident_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/incident.json".into(),
            imported_issue_draft_ref:
                ".punk/imported-incidents/foreign-demo/inc_123/prom_123/issue.md".into(),
            imported_repro_ref: ".punk/imported-incidents/foreign-demo/inc_123/prom_123/repro.md"
                .into(),
            prepared_goal: "Investigate and fix promoted incident inc_123".into(),
            draft_feature_id: "feat_456".into(),
            draft_contract_id: "ct_456_v1".into(),
            auto_run_attempts: 0,
            last_attempt_at: None,
            last_failure: None,
            execution: None,
            created_at: "2026-04-16T00:30:00Z".into(),
        };

        let error = anyhow::anyhow!("target repo lost the imported bundle");
        let rendered = format_incident_auto_run_failure(&promotion, error.as_ref());
        assert!(rendered.contains("promotion prom_123 auto-run failed"));
        assert!(rendered.contains("punk inspect prom_123"));
        assert!(rendered.contains("punk incident rerun prom_123 --auto-run"));
    }

    #[test]
    fn incident_submission_summary_mentions_preview_and_publish_state() {
        let submission = punk_domain::IncidentSubmissionRecord {
            id: "sub_123".into(),
            incident_id: "inc_123".into(),
            submission_kind: "github_issue".into(),
            github_repo: "heurema/specpunk".into(),
            issue_title: "punk runtime bug [inc_123]: blocked:no-progress".into(),
            body_ref: ".punk/submissions/inc_123/sub_123/body.md".into(),
            preview_command:
                "gh issue create --repo \"heurema/specpunk\" --title \"punk runtime bug [inc_123]: blocked:no-progress\" --body-file \".punk/submissions/inc_123/sub_123/body.md\""
                    .into(),
            state: "submitted".into(),
            published_issue_url: Some("https://github.com/heurema/specpunk/issues/123".into()),
            published_issue_number: Some(123),
            publish_error: None,
            created_at: "2026-04-16T01:00:00Z".into(),
            updated_at: "2026-04-16T01:01:00Z".into(),
        };

        let rendered = format_incident_submission_summary(&submission);
        assert!(rendered.contains("Submission: sub_123"));
        assert!(rendered.contains("GitHub repo: heurema/specpunk"));
        assert!(rendered.contains("State: submitted"));
        assert!(rendered.contains("Body snapshot: .punk/submissions/inc_123/sub_123/body.md"));
        assert!(
            rendered.contains("Published issue: https://github.com/heurema/specpunk/issues/123")
        );
    }

    #[test]
    fn incident_defaults_summary_mentions_repo_and_github_defaults() {
        let defaults = punk_orch::IncidentDefaults {
            promote_repo_root: Some("/tmp/specpunk".into()),
            github_repo: Some("heurema/specpunk".into()),
            updated_at: Some("2026-04-16T02:00:00Z".into()),
        };

        let rendered = format_incident_defaults_summary("Incident defaults", &defaults);
        assert!(rendered.contains("Promote repo: /tmp/specpunk"));
        assert!(rendered.contains("GitHub repo: heurema/specpunk"));
        assert!(rendered.contains("Updated at: 2026-04-16T02:00:00Z"));
    }

    #[test]
    fn resolve_incident_targets_fall_back_to_repo_local_defaults() {
        let root = temp_test_dir("incident-defaults-cli");
        let global = temp_test_dir("incident-defaults-cli-global");
        let target = temp_test_dir("incident-defaults-cli-target");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='defaults-demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let orch = OrchService::new(&root, &global).unwrap();
        orch.set_incident_defaults(Some(&target), Some("heurema/specpunk"))
            .unwrap();

        let promote_repo = resolve_incident_promote_repo(&orch, None).unwrap();
        let github_repo = resolve_incident_submit_repo(&orch, None).unwrap();

        assert_eq!(promote_repo, target.canonicalize().unwrap());
        assert_eq!(github_repo, "heurema/specpunk");

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
        let _ = fs::remove_dir_all(&target);
    }

    #[test]
    fn resolve_incident_targets_fall_back_to_global_defaults_when_repo_local_is_missing() {
        let root = temp_test_dir("incident-defaults-cli-global-fallback");
        let global = temp_test_dir("incident-defaults-cli-global-root");
        let target = temp_test_dir("incident-defaults-cli-global-target");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='defaults-demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();

        let orch = OrchService::new(&root, &global).unwrap();
        orch.set_global_incident_defaults(Some(&target), Some("heurema/specpunk"))
            .unwrap();

        let promote_repo = resolve_incident_promote_repo(&orch, None).unwrap();
        let github_repo = resolve_incident_submit_repo(&orch, None).unwrap();

        assert_eq!(promote_repo, target.canonicalize().unwrap());
        assert_eq!(github_repo, "heurema/specpunk");

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&global);
        let _ = fs::remove_dir_all(&target);
    }

    #[test]
    fn research_summary_mentions_terminal_follow_up_refs() {
        let research = punk_orch::ResearchInspectView {
            record: punk_domain::ResearchRecord {
                id: "research_123".into(),
                project_id: "specpunk".into(),
                kind: "architecture".into(),
                state: "completed".into(),
                question_ref: ".punk/research/research_123/question.json".into(),
                packet_ref: ".punk/research/research_123/packet.json".into(),
                artifact_refs: vec![
                    ".punk/research/research_123/artifacts/artifact_123.json".into()
                ],
                synthesis_ref: Some(".punk/research/research_123/synthesis.json".into()),
                synthesis_history_refs: vec![
                    ".punk/research/research_123/syntheses/synthesis_001.json".into(),
                    ".punk/research/research_123/syntheses/synthesis_123.json".into(),
                ],
                invalidated_synthesis_ref: None,
                invalidation_artifact_ref: None,
                invalidation_history: Vec::new(),
                outcome: Some("adr_draft".into()),
                created_at: "2026-04-11T00:00:00Z".into(),
                updated_at: "2026-04-11T00:00:00Z".into(),
            },
            question: punk_domain::ResearchQuestion {
                id: "rq_123".into(),
                project_id: "specpunk".into(),
                kind: "architecture".into(),
                subject_ref: Some(".punk/project.json".into()),
                question: "Should research packets become first-class repo artifacts?".into(),
                goal: "Freeze a bounded research packet for later execution.".into(),
                constraints: vec!["Keep it advisory-only.".into()],
                success_criteria: vec![
                    "Packet captures an explicit budget.".into(),
                    "Inspect recovers the frozen packet.".into(),
                ],
                created_at: "2026-04-11T00:00:00Z".into(),
            },
            packet: punk_domain::ResearchPacket {
                id: "rp_123".into(),
                research_id: "research_123".into(),
                question_ref: ".punk/research/research_123/question.json".into(),
                repo_snapshot_ref: punk_domain::council::RepoSnapshotRef {
                    vcs: Some(punk_domain::VcsKind::Git),
                    head_ref: Some("HEAD".into()),
                    dirty: false,
                },
                contract_ref: None,
                receipt_ref: None,
                skill_ref: None,
                eval_ref: None,
                context_refs: vec!["docs/product/RESEARCH.md".into()],
                budget: punk_domain::ResearchBudget {
                    max_rounds: 3,
                    max_worker_slots: 5,
                    max_cost_usd: Some(10.0),
                    max_duration_minutes: 30,
                    max_artifacts: 12,
                },
                stop_rules: vec![
                    "stop_when_budget_exhausted".into(),
                    "stop_when_evidence_is_sufficient".into(),
                    "escalate_on_persistent_ambiguity".into(),
                ],
                output_schema_ref: "docs/product/RESEARCH.md#researchsynthesis".into(),
                created_at: "2026-04-11T00:00:00Z".into(),
            },
            artifacts: vec![punk_domain::ResearchArtifact {
                id: "artifact_123".into(),
                research_id: "research_123".into(),
                kind: "note".into(),
                summary: "Captured the first bounded hypothesis.".into(),
                source_ref: Some("docs/product/RESEARCH.md".into()),
                created_at: "2026-04-11T00:10:00Z".into(),
            }],
            synthesis: Some(punk_domain::ResearchSynthesis {
                id: "synthesis_123".into(),
                research_id: "research_123".into(),
                outcome: "adr_draft".into(),
                summary: "The bounded architecture recommendation is complete.".into(),
                artifact_refs: vec![
                    ".punk/research/research_123/artifacts/artifact_123.json".into()
                ],
                supersedes_ref: Some(
                    ".punk/research/research_123/syntheses/synthesis_001.json".into(),
                ),
                follow_up_refs: vec![
                    "docs/product/ARCHITECTURE.md".into(),
                    "docs/product/CLI.md".into(),
                ],
                created_at: "2026-04-11T00:20:00Z".into(),
            }),
            invalidation: punk_orch::ResearchInvalidationInspectView {
                active: None,
                latest: None,
                history_count: 0,
            },
            synthesis_lineage: punk_orch::ResearchSynthesisLineageInspectView {
                active: Some(punk_orch::ResearchSynthesisLineageEntry {
                    identity_ref: ".punk/research/research_123/syntheses/synthesis_123.json".into(),
                    outcome: "adr_draft".into(),
                    supersedes_ref: Some(
                        ".punk/research/research_123/syntheses/synthesis_001.json".into(),
                    ),
                }),
                latest: Some(punk_orch::ResearchSynthesisLineageEntry {
                    identity_ref: ".punk/research/research_123/syntheses/synthesis_123.json".into(),
                    outcome: "adr_draft".into(),
                    supersedes_ref: Some(
                        ".punk/research/research_123/syntheses/synthesis_001.json".into(),
                    ),
                }),
                history_count: 2,
                history: vec![
                    punk_orch::ResearchSynthesisLineageEntry {
                        identity_ref: ".punk/research/research_123/syntheses/synthesis_001.json"
                            .into(),
                        outcome: "adr_draft".into(),
                        supersedes_ref: None,
                    },
                    punk_orch::ResearchSynthesisLineageEntry {
                        identity_ref: ".punk/research/research_123/syntheses/synthesis_123.json"
                            .into(),
                        outcome: "adr_draft".into(),
                        supersedes_ref: Some(
                            ".punk/research/research_123/syntheses/synthesis_001.json".into(),
                        ),
                    },
                ],
                has_active_current_view: true,
                has_replacements: true,
                latest_is_active: true,
            },
        };

        let rendered = format_research_summary(&research);
        assert!(rendered.contains("Research: research_123"));
        assert!(rendered.contains("State: completed"));
        assert!(rendered.contains("Kind: architecture"));
        assert!(rendered.contains("Subject ref: .punk/project.json"));
        assert!(rendered.contains("Snapshot: vcs=git head_ref=HEAD dirty=false"));
        assert!(rendered.contains(
            "Budget: rounds=3 worker_slots=5 duration_minutes=30 artifacts=12 cost_usd=10.00"
        ));
        assert!(rendered.contains("Artifact count: 1"));
        assert!(rendered.contains("- .punk/research/research_123/artifacts/artifact_123.json"));
        assert!(rendered.contains("Outcome: adr_draft"));
        assert!(rendered.contains("Synthesis ref: .punk/research/research_123/synthesis.json"));
        assert!(rendered.contains(
            "Synthesis identity ref: .punk/research/research_123/syntheses/synthesis_123.json"
        ));
        assert!(rendered.contains(
            "Supersedes synthesis ref: .punk/research/research_123/syntheses/synthesis_001.json"
        ));
        assert!(rendered.contains("Synthesis history refs:"));
        assert!(rendered.contains("- .punk/research/research_123/syntheses/synthesis_001.json"));
        assert!(rendered.contains("- .punk/research/research_123/syntheses/synthesis_123.json"));
        assert!(rendered
            .contains("Synthesis summary: The bounded architecture recommendation is complete."));
        assert!(rendered.contains("Synthesis artifact refs:"));
        assert!(rendered.contains("Follow-up refs:"));
        assert!(rendered.contains("- docs/product/ARCHITECTURE.md"));
        assert!(rendered.contains("- docs/product/CLI.md"));
        assert!(rendered.contains("Invalidation note: none"));
        assert!(rendered.contains("Invalidated synthesis ref: none"));
        assert!(rendered.contains("Invalidating artifact ref: none"));
        assert!(rendered.contains("Invalidation history:\nnone"));
        assert!(rendered.contains("Context refs: docs/product/RESEARCH.md"));
        assert!(rendered.contains("- Packet captures an explicit budget."));
        assert!(rendered.contains("- Keep it advisory-only."));
        assert!(rendered.contains("- stop_when_budget_exhausted"));
        assert!(rendered.contains("Output schema: docs/product/RESEARCH.md#researchsynthesis"));
        assert!(rendered.contains("Next: terminal state reached; review follow-up refs"));
    }

    #[test]
    fn research_summary_mentions_invalidation_note_while_gathering() {
        let research = punk_orch::ResearchInspectView {
            record: punk_domain::ResearchRecord {
                id: "research_456".into(),
                project_id: "specpunk".into(),
                kind: "architecture".into(),
                state: "gathering".into(),
                question_ref: ".punk/research/research_456/question.json".into(),
                packet_ref: ".punk/research/research_456/packet.json".into(),
                artifact_refs: vec![
                    ".punk/research/research_456/artifacts/artifact_001.json".into(),
                    ".punk/research/research_456/artifacts/artifact_002.json".into(),
                ],
                synthesis_ref: None,
                synthesis_history_refs: vec![
                    ".punk/research/research_456/syntheses/synthesis_001.json".into(),
                ],
                invalidated_synthesis_ref: Some(
                    ".punk/research/research_456/syntheses/synthesis_001.json".into(),
                ),
                invalidation_artifact_ref: Some(
                    ".punk/research/research_456/artifacts/artifact_002.json".into(),
                ),
                invalidation_history: vec![punk_domain::ResearchInvalidationEntry {
                    invalidated_synthesis_ref:
                        ".punk/research/research_456/syntheses/synthesis_001.json".into(),
                    invalidating_artifact_ref:
                        ".punk/research/research_456/artifacts/artifact_002.json".into(),
                    invalidated_at: "2026-04-12T00:10:00Z".into(),
                }],
                outcome: None,
                created_at: "2026-04-12T00:00:00Z".into(),
                updated_at: "2026-04-12T00:00:00Z".into(),
            },
            question: punk_domain::ResearchQuestion {
                id: "rq_456".into(),
                project_id: "specpunk".into(),
                kind: "architecture".into(),
                subject_ref: None,
                question: "Why was the previous synthesis cleared?".into(),
                goal: "Show an explicit invalidation note.".into(),
                constraints: vec!["Stay bounded.".into()],
                success_criteria: vec!["Human inspect explains invalidation.".into()],
                created_at: "2026-04-12T00:00:00Z".into(),
            },
            packet: punk_domain::ResearchPacket {
                id: "rp_456".into(),
                research_id: "research_456".into(),
                question_ref: ".punk/research/research_456/question.json".into(),
                repo_snapshot_ref: punk_domain::council::RepoSnapshotRef {
                    vcs: Some(punk_domain::VcsKind::Git),
                    head_ref: Some("HEAD".into()),
                    dirty: true,
                },
                contract_ref: None,
                receipt_ref: None,
                skill_ref: None,
                eval_ref: None,
                context_refs: vec!["docs/product/RESEARCH.md".into()],
                budget: punk_domain::ResearchBudget {
                    max_rounds: 3,
                    max_worker_slots: 5,
                    max_cost_usd: None,
                    max_duration_minutes: 30,
                    max_artifacts: 12,
                },
                stop_rules: vec!["stop_when_evidence_is_sufficient".into()],
                output_schema_ref: "docs/product/RESEARCH.md#researchsynthesis".into(),
                created_at: "2026-04-12T00:00:00Z".into(),
            },
            artifacts: vec![
                punk_domain::ResearchArtifact {
                    id: "artifact_001".into(),
                    research_id: "research_456".into(),
                    kind: "note".into(),
                    summary: "First note.".into(),
                    source_ref: Some("docs/product/RESEARCH.md".into()),
                    created_at: "2026-04-12T00:05:00Z".into(),
                },
                punk_domain::ResearchArtifact {
                    id: "artifact_002".into(),
                    research_id: "research_456".into(),
                    kind: "note".into(),
                    summary: "Second note invalidated the previous synthesis.".into(),
                    source_ref: Some("docs/product/RESEARCH.md".into()),
                    created_at: "2026-04-12T00:10:00Z".into(),
                },
            ],
            synthesis: None,
            invalidation: punk_orch::ResearchInvalidationInspectView {
                active: Some(punk_domain::ResearchInvalidationEntry {
                    invalidated_synthesis_ref:
                        ".punk/research/research_456/syntheses/synthesis_001.json".into(),
                    invalidating_artifact_ref:
                        ".punk/research/research_456/artifacts/artifact_002.json".into(),
                    invalidated_at: "2026-04-12T00:10:00Z".into(),
                }),
                latest: Some(punk_domain::ResearchInvalidationEntry {
                    invalidated_synthesis_ref:
                        ".punk/research/research_456/syntheses/synthesis_001.json".into(),
                    invalidating_artifact_ref:
                        ".punk/research/research_456/artifacts/artifact_002.json".into(),
                    invalidated_at: "2026-04-12T00:10:00Z".into(),
                }),
                history_count: 1,
            },
            synthesis_lineage: punk_orch::ResearchSynthesisLineageInspectView {
                active: None,
                latest: Some(punk_orch::ResearchSynthesisLineageEntry {
                    identity_ref: ".punk/research/research_456/syntheses/synthesis_001.json".into(),
                    outcome: "adr_draft".into(),
                    supersedes_ref: None,
                }),
                history_count: 1,
                history: vec![punk_orch::ResearchSynthesisLineageEntry {
                    identity_ref: ".punk/research/research_456/syntheses/synthesis_001.json".into(),
                    outcome: "adr_draft".into(),
                    supersedes_ref: None,
                }],
                has_active_current_view: false,
                has_replacements: false,
                latest_is_active: false,
            },
        };

        let rendered = format_research_summary(&research);
        assert!(rendered.contains("State: gathering"));
        assert!(rendered.contains("Synthesis ref: none"));
        assert!(rendered.contains(
            "Invalidation note: current synthesized view was cleared by a newer artifact"
        ));
        assert!(rendered.contains(
            "Invalidated synthesis ref: .punk/research/research_456/syntheses/synthesis_001.json"
        ));
        assert!(rendered.contains(
            "Invalidating artifact ref: .punk/research/research_456/artifacts/artifact_002.json"
        ));
        assert!(rendered.contains("Invalidation history:"));
        assert!(rendered.contains(
            "- invalidated=.punk/research/research_456/syntheses/synthesis_001.json by=.punk/research/research_456/artifacts/artifact_002.json at=2026-04-12T00:10:00Z"
        ));
        assert!(rendered.contains(
            "Next: punk research synthesize research_456 --outcome <outcome> --summary \"<summary>\""
        ));
    }

    #[test]
    fn summarize_proof_harness_evidence_mentions_declared_and_executed_items() {
        let proof = punk_domain::Proofpack {
            id: "proof_789".into(),
            decision_id: "dec_789".into(),
            run_id: "run_789".into(),
            run_ref: None,
            contract_ref: ".punk/contracts/feat_789/v1.json".into(),
            receipt_ref: ".punk/runs/run_789/receipt.json".into(),
            decision_ref: ".punk/decisions/dec_789.json".into(),
            check_refs: vec![],
            workspace_lineage: None,
            verification_context_ref: None,
            verification_context_identity: None,
            executor_identity: None,
            reproducibility_claim: None,
            command_evidence: vec![],
            declared_harness_evidence: vec![punk_domain::DeclaredHarnessEvidence {
                evidence_type: "log_query".into(),
                profile: "default".into(),
                source_ref: Some(".punk/project/harness.json".into()),
                summary: "declared non-command harness surface from persisted packet".into(),
            }],
            harness_evidence: vec![punk_domain::HarnessEvidence {
                evidence_type: "artifact_assertion".into(),
                profile: "default".into(),
                status: punk_domain::CheckStatus::Pass,
                summary: "artifact exists".into(),
                source_ref: Some(".punk/project/harness.json".into()),
                artifact_ref: Some("AGENTS.md".into()),
            }],
            hashes: Default::default(),
            summary: "proof for dec_789".into(),
            created_at: "2026-04-09T00:00:00Z".into(),
        };

        let rendered = summarize_proof_harness_evidence(&proof);
        assert!(rendered.contains(
            "- declared log_query [default]: declared non-command harness surface from persisted packet"
        ));
        assert!(rendered.contains("- artifact_assertion pass [default]: AGENTS.md"));
    }

    #[test]
    fn go_decision_only_accepts_accept() {
        assert!(go_decision_succeeds(&punk_domain::Decision::Accept));
        assert!(!go_decision_succeeds(&punk_domain::Decision::Block));
        assert!(!go_decision_succeeds(&punk_domain::Decision::Escalate));
    }

    #[test]
    fn go_outcome_labels_follow_decision() {
        assert_eq!(go_outcome_label(&punk_domain::Decision::Accept), "success");
        assert_eq!(go_outcome_label(&punk_domain::Decision::Block), "blocked");
        assert_eq!(
            go_outcome_label(&punk_domain::Decision::Escalate),
            "escalated"
        );
    }

    #[test]
    fn summarize_decision_basis_is_concise_and_stable() {
        assert_eq!(
            summarize_decision_basis(&[
                " first reason ".into(),
                "second reason".into(),
                "third reason".into(),
            ]),
            "first reason; second reason"
        );
        assert_eq!(
            summarize_decision_basis(&[]),
            "no explicit decision basis recorded"
        );
    }

    #[test]
    fn go_recovery_command_switches_blocked_runs_to_staged_mode() {
        assert_eq!(
            go_recovery_command(&punk_domain::Decision::Accept, "ship feature"),
            None
        );
        assert_eq!(
            go_recovery_command(&punk_domain::Decision::Block, "ship \"feature\""),
            Some("punk start \"ship \\\"feature\\\"\"".to_string())
        );
        assert_eq!(
            go_recommended_mode(&punk_domain::Decision::Escalate),
            "staged_review"
        );
    }

    #[test]
    fn auto_chain_requires_bootstrap_success_and_implementation_goal() {
        let cycle = GoCycleResult {
            contract: punk_domain::Contract {
                id: "ct_123".into(),
                feature_id: "feat_123".into(),
                version: 1,
                status: punk_domain::ContractStatus::Approved,
                prompt_source: "scaffold and implement".into(),
                entry_points: vec!["Cargo.toml".into()],
                import_paths: vec![],
                expected_interfaces: vec![],
                behavior_requirements: vec![],
                allowed_scope: vec!["Cargo.toml".into()],
                target_checks: vec![],
                integrity_checks: vec![],
                risk_level: "low".into(),
                created_at: "now".into(),
                approved_at: Some("now".into()),
            },
            run: punk_domain::Run {
                id: "run_123".into(),
                task_id: "task_123".into(),
                feature_id: "feat_123".into(),
                contract_id: "ct_123".into(),
                attempt: 1,
                status: punk_domain::RunStatus::Finished,
                mode_origin: punk_domain::ModeId::Cut,
                started_at: "now".into(),
                ended_at: Some("now".into()),
                vcs: punk_domain::RunVcs {
                    backend: punk_domain::VcsKind::Git,
                    workspace_ref: ".".into(),
                    change_ref: "change".into(),
                    base_ref: None,
                },
                verification_context_ref: None,
                architecture_inputs_ref: None,
            },
            receipt: punk_domain::Receipt {
                id: "rcpt_123".into(),
                run_id: "run_123".into(),
                task_id: "task_123".into(),
                status: "success".into(),
                executor_name: "codex-cli".into(),
                changed_files: vec![],
                artifacts: punk_domain::ReceiptArtifacts {
                    stdout_ref: "stdout".into(),
                    stderr_ref: "stderr".into(),
                },
                checks_run: vec![],
                duration_ms: 1,
                cost_usd: None,
                summary: "PUNK_EXECUTION_COMPLETE: controller bootstrap scaffold created and checks passed".into(),
                created_at: "now".into(),
            },
            decision: punk_domain::DecisionObject {
                id: "dec_123".into(),
                run_id: "run_123".into(),
                contract_id: "ct_123".into(),
                decision: punk_domain::Decision::Accept,
                deterministic_status: punk_domain::DeterministicStatus::Pass,
                target_status: punk_domain::CheckStatus::Pass,
                integrity_status: punk_domain::CheckStatus::Pass,
                confidence_estimate: 0.99,
                decision_basis: vec![],
                contract_ref: "ct_123".into(),
                receipt_ref: "rcpt_123".into(),
                check_refs: vec![],
                verification_context_ref: None,
                verification_context_identity: None,
                command_evidence: vec![],
                declared_harness_evidence: vec![],
                harness_evidence: vec![],
                created_at: "now".into(),
            },
            proof: punk_domain::Proofpack {
                id: "proof_123".into(),
                decision_id: "dec_123".into(),
                run_id: "run_123".into(),
                run_ref: None,
                contract_ref: "ct_123".into(),
                receipt_ref: "rcpt_123".into(),
                decision_ref: "dec_123".into(),
                check_refs: vec![],
                workspace_lineage: None,
                verification_context_ref: None,
                verification_context_identity: None,
                executor_identity: None,
                reproducibility_claim: None,
                command_evidence: vec![],
                declared_harness_evidence: vec![],
                harness_evidence: vec![],
                hashes: Default::default(),
                summary: "bootstrap".into(),
                created_at: "now".into(),
            },
        };

        assert!(should_auto_chain_after_bootstrap(
            "scaffold Rust workspace and implement pubpunk init command with tests",
            &cycle
        ));
        assert!(!should_auto_chain_after_bootstrap(
            "scaffold Rust workspace for pubpunk",
            &cycle
        ));
    }

    #[test]
    fn synthesize_follow_up_goal_narrows_pubpunk_init_scope_after_bootstrap() {
        let root = std::env::temp_dir().join(format!(
            "punk-cli-auto-chain-follow-up-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/pubpunk-cli")).unwrap();
        fs::create_dir_all(root.join("crates/pubpunk-core")).unwrap();

        let goal = "scaffold Rust workspace and implement pubpunk init command with --json output and tests";
        let follow_up = synthesize_follow_up_goal(&root, goal).unwrap();

        assert_eq!(
            follow_up,
            "implement pubpunk init command touching exactly crates/pubpunk-cli/src/main.rs, crates/pubpunk-core/src/lib.rs, and tests; add --json output, and keep cargo test --workspace green"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn go_summary_includes_prepared_staged_recovery() {
        let rendered = format_go_summary(
            "interviewcoach",
            "ship feature",
            "ct_123",
            "run_456",
            "failure",
            "blocked by checks",
            "blocked",
            "block",
            "target checks failed",
            "proof_789",
            "punk inspect proof_789 --json",
            Some("bootstrap proof proof_111 triggered follow-up implementation cycle proof_789"),
            Some("punk start \"ship feature\""),
            Some("ct_999"),
            Some("punk plot approve ct_999"),
            Some("punk incident capture proof_789"),
        );
        assert!(rendered.contains("Auto-chain: bootstrap proof proof_111 triggered follow-up implementation cycle proof_789"));
        assert!(rendered.contains("Recovery: punk start \"ship feature\""));
        assert!(rendered.contains("Recovery contract: ct_999"));
        assert!(rendered.contains("Recovery next: punk plot approve ct_999"));
        assert!(rendered.contains("Incident capture: punk incident capture proof_789"));
    }

    #[test]
    fn resolve_init_project_id_prefers_explicit_then_repo_basename() {
        let root = PathBuf::from("/tmp/interviewcoach");
        assert_eq!(
            resolve_init_project_id(&root, Some("custom-project")).unwrap(),
            "custom-project"
        );
        assert_eq!(
            resolve_init_project_id(&root, None).unwrap(),
            "interviewcoach"
        );
    }

    #[test]
    fn init_error_mentions_native_init_recovery() {
        let rendered = format_init_error(
            "interviewcoach",
            "no Git or jj repo detected in the current directory",
        );
        assert!(rendered.contains("project init failed"));
        assert!(rendered.contains("no Git or jj repo detected in the current directory"));
        assert!(rendered.contains("punk init --project interviewcoach --enable-jj --verify"));
        assert!(rendered.contains("after `git init` if needed"));
    }
}
