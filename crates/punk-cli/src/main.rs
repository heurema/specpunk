use std::env;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand};
use punk_adapters::{CodexCliContractDrafter, CodexCliExecutor};
use punk_gate::GateService;
use punk_orch::OrchService;
use punk_proof::ProofService;

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
    Plot(PlotCommand),
    Cut(CutCommand),
    Gate(GateCommand),
    Status(StatusCommand),
    Inspect(InspectCommand),
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
    let orch = OrchService::new(&repo_root, &global_root)?;

    match cli.command {
        Command::Plot(plot) => match plot.action {
            PlotAction::Contract { prompt, json } => {
                let drafter = CodexCliContractDrafter::default();
                let contract = orch.draft_contract(&drafter, &prompt)?;
                render(json, &contract, &format!("drafted {}", contract.id))
            }
            PlotAction::Refine {
                contract_id,
                guidance,
                json,
            } => {
                let drafter = CodexCliContractDrafter::default();
                let contract = orch.refine_contract(&drafter, &contract_id, &guidance)?;
                render(json, &contract, &format!("refined {}", contract.id))
            }
            PlotAction::Approve { contract_id, json } => {
                let contract = orch.approve_contract(&contract_id)?;
                render(json, &contract, &format!("approved {}", contract.id))
            }
        },
        Command::Cut(cut) => match cut.action {
            CutAction::Run { contract_id, json } => {
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
                    println!("run {} ({})", run.id, receipt.status);
                    println!("summary: {}", receipt.summary);
                }
                Ok(())
            }
        },
        Command::Gate(gate) => match gate.action {
            GateAction::Run { run_id, json } => {
                let service = GateService::new(&repo_root, &global_root);
                let decision = service.gate_run(&run_id)?;
                render(
                    json,
                    &decision,
                    &format!("decision {:?} for {}", decision.decision, decision.run_id),
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
            let snapshot = orch.status(status.id.as_deref())?;
            render(
                status.json,
                &snapshot,
                &format!(
                    "project={} events={} contract={:?} run={:?} decision={:?}",
                    snapshot.project_id,
                    snapshot.events_count,
                    snapshot.last_contract_id,
                    snapshot.last_run_id,
                    snapshot.last_decision_id
                ),
            )
        }
        Command::Inspect(inspect) => {
            if !inspect.json {
                return Err(anyhow!("inspect currently requires --json"));
            }
            let value = orch.inspect(&inspect.id)?;
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
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

fn default_global_root() -> Result<PathBuf> {
    let home = env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| anyhow!("HOME is not set"))?;
    Ok(home.join(".punk"))
}
