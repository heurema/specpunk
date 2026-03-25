use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use punk_core::check::{self, CheckOptions, render_check};
use punk_core::init::run_init;
use punk_core::plan::{run_plan_headless, save_contract, PlanOptions};
use punk_core::plan::contract::{Feedback, FeedbackOutcome};
use punk_core::receipt::{self, ReceiptOptions, render_receipt_md, render_receipt_short};

#[derive(Parser)]
#[command(
    name = "punk",
    about = "Spec-driven development toolkit — keep contracts and code in sync",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a punk workspace in the current directory
    Init {
        /// Target directory to initialize (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Generate an implementation plan from a task description
    Plan {
        /// Task description (what to build)
        task: String,
        /// Open $EDITOR with contract template instead of calling LLM
        #[arg(long)]
        manual: bool,
    },
    /// Check implementation against the active contract
    Check {
        /// Output JSON instead of human-readable text
        #[arg(long)]
        json: bool,
        /// Strict mode: undeclared files are hard failures (for CI)
        #[arg(long)]
        strict: bool,
    },
    /// Record a task receipt for the completed contract
    Receipt {
        /// Output JSON instead of short summary
        #[arg(long)]
        json: bool,
        /// Output Markdown summary
        #[arg(long)]
        md: bool,
    },
    /// Show current workspace status
    Status,
    /// Close (abandon) the active contract with a reason
    Close {
        /// Reason for closing the contract
        reason: String,
    },
    /// Manage punk configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Set an LLM provider (name, endpoint, API key)
    SetProvider {
        /// Provider name (e.g. "anthropic", "openai")
        name: String,
        /// API endpoint URL
        endpoint: String,
        /// API key (will be stored in ~/.config/punk/providers.toml)
        api_key: String,
        /// Optional model name
        #[arg(long)]
        model: Option<String>,
    },
    /// Remove a configured provider
    RemoveProvider {
        /// Provider name to remove
        name: String,
    },
    /// List configured providers
    List,
    /// Show resolved provider (env vars + config file)
    Show,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { path } => {
            let root = if path == Path::new(".") {
                std::env::current_dir().unwrap_or(path)
            } else {
                path
            };

            match run_init(&root, None) {
                Ok(result) => {
                    println!(
                        "punk init: {} mode — {} artifacts written",
                        format!("{:?}", result.mode).to_lowercase(),
                        result.artifacts_written.len()
                    );
                    for artifact in &result.artifacts_written {
                        println!("  {artifact}");
                    }
                }
                Err(e) => {
                    eprintln!("punk init: error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Plan { task, manual } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

            // Resolve LLM provider: env vars → config file → guided setup
            let resolved = punk_core::config::resolve_provider();
            let http_provider = resolved.map(|p| {
                punk_core::plan::llm::HttpProvider::new(p.endpoint, p.api_key)
            });
            let provider_ref: Option<&dyn punk_core::plan::LlmProvider> =
                http_provider.as_ref().map(|p| p as &dyn punk_core::plan::LlmProvider);

            if !manual && provider_ref.is_none() {
                eprintln!("punk plan: no LLM provider configured");
                eprintln!("  Set up with: punk config set-provider anthropic https://api.anthropic.com/v1/messages <your-key>");
                eprintln!("  Or set env:  PUNK_LLM_ENDPOINT + PUNK_LLM_API_KEY");
                eprintln!("  Or use:      punk plan --manual (offline, no LLM)");
                std::process::exit(1);
            }

            let opts = PlanOptions {
                root: &root,
                task: &task,
                manual,
                provider: provider_ref,
            };

            let (mut contract, quality, summary) = match run_plan_headless(&opts).await {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("punk plan: {e}");
                    std::process::exit(1);
                }
            };

            println!("{summary}");

            // Interactive approval loop
            let punk_dir = root.join(".punk");
            loop {
                print!("\nApprove contract? [y]es / [n]o / [e]dit / [q]uit: ");
                io::stdout().flush().unwrap_or_default();

                let mut input = String::new();
                match io::stdin().read_line(&mut input) {
                    Ok(0) => {
                        // EOF — treat as quit
                        eprintln!("punk plan: aborted (EOF)");
                        std::process::exit(1);
                    }
                    Err(e) => {
                        eprintln!("punk plan: stdin error: {e}");
                        std::process::exit(1);
                    }
                    Ok(_) => {}
                }

                let choice = input.trim().to_lowercase();
                match choice.as_str() {
                    "y" | "yes" => {
                        let feedback = Feedback {
                            outcome: FeedbackOutcome::Approve,
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            note: None,
                        };
                        match save_contract(&punk_dir, &mut contract, &feedback) {
                            Ok((cp, _fp)) => {
                                println!("punk plan: contract saved to {}", cp.display());
                            }
                            Err(e) => {
                                eprintln!("punk plan: save error: {e}");
                                std::process::exit(1);
                            }
                        }
                        break;
                    }
                    "n" | "no" => {
                        let feedback = Feedback {
                            outcome: FeedbackOutcome::Reject,
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            note: None,
                        };
                        // Save feedback only (no contract) — create directory for tracking
                        let dir = punk_dir.join("contracts").join(&contract.change_id);
                        if let Err(e) = std::fs::create_dir_all(&dir) {
                            eprintln!("punk plan: could not create feedback dir: {e}");
                        } else {
                            let fb_json = serde_json::to_string_pretty(&feedback)
                                .unwrap_or_default();
                            let _ = std::fs::write(dir.join("feedback.json"), fb_json);
                        }
                        println!("punk plan: contract rejected and discarded");
                        break;
                    }
                    "e" | "edit" => {
                        // Open $EDITOR with contract JSON (secure temp file, 0600)
                        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
                        let contract_json =
                            serde_json::to_string_pretty(&contract).unwrap_or_default();
                        let tmp_file = match tempfile::Builder::new()
                            .prefix("punk-contract-")
                            .suffix(".json")
                            .tempfile()
                        {
                            Ok(f) => f,
                            Err(e) => {
                                eprintln!("punk plan: could not create temp file: {e}");
                                continue;
                            }
                        };
                        let tmp = tmp_file.path().to_path_buf();
                        if let Err(e) = std::fs::write(&tmp, &contract_json) {
                            eprintln!("punk plan: could not write temp file: {e}");
                            continue;
                        }
                        let status = std::process::Command::new(&editor)
                            .arg(&tmp)
                            .status();
                        match status {
                            Ok(s) if s.success() => {
                                // Re-read and re-validate
                                match std::fs::read_to_string(&tmp) {
                                    Ok(raw) => match serde_json::from_str(&raw) {
                                        Ok(edited) => {
                                            contract = edited;
                                            let new_quality = punk_core::plan::quality::check_quality(
                                                &contract.acceptance_criteria,
                                                &contract.scope.touch,
                                                &contract.scope.dont_touch,
                                            );
                                            let new_summary = punk_core::plan::render::render_summary(
                                                &contract,
                                                &new_quality,
                                            );
                                            println!("{new_summary}");
                                            let feedback = Feedback {
                                                outcome: FeedbackOutcome::ApproveWithEdit,
                                                timestamp: chrono::Utc::now().to_rfc3339(),
                                                note: Some("edited in $EDITOR".to_string()),
                                            };
                                            match save_contract(&punk_dir, &mut contract, &feedback) {
                                                Ok((cp, _)) => {
                                                    println!("punk plan: edited contract saved to {}", cp.display());
                                                    break;
                                                }
                                                Err(e) => {
                                                    eprintln!("punk plan: save error: {e}");
                                                    std::process::exit(1);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("punk plan: invalid JSON after edit: {e}");
                                            continue;
                                        }
                                    },
                                    Err(e) => {
                                        eprintln!("punk plan: could not read edited file: {e}");
                                        continue;
                                    }
                                }
                            }
                            _ => {
                                eprintln!("punk plan: editor exited with error");
                                continue;
                            }
                        }
                    }
                    "q" | "quit" => {
                        let feedback = Feedback {
                            outcome: FeedbackOutcome::Quit,
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            note: None,
                        };
                        // Log quit feedback
                        let dir = punk_dir.join("contracts").join(&contract.change_id);
                        if let Ok(()) = std::fs::create_dir_all(&dir) {
                            let fb_json = serde_json::to_string_pretty(&feedback)
                                .unwrap_or_default();
                            let _ = std::fs::write(dir.join("feedback.json"), fb_json);
                        }
                        println!("punk plan: quit");
                        break;
                    }
                    _ => {
                        eprintln!("punk plan: unknown choice '{choice}' — enter y/n/e/q");
                    }
                }
            }

            // Suppress unused warning on quality in this path
            let _ = quality;
        }
        Commands::Check { json, strict } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let opts = CheckOptions {
                root: &root,
                strict,
                json,
            };

            match check::run_check(&opts) {
                Ok((receipt, exit_code)) => {
                    if json {
                        let j = serde_json::to_string_pretty(&receipt).unwrap_or_default();
                        println!("{j}");
                    } else {
                        print!("{}", render_check(&receipt, strict));
                    }
                    std::process::exit(exit_code);
                }
                Err(check::CheckError::NoContract(msg)) => {
                    if json {
                        println!(r#"{{"status":"ERROR","code":{},"message":{}}}"#,
                            check::EXIT_NO_CONTRACT,
                            serde_json::to_string(&msg).unwrap_or_default());
                    } else {
                        eprintln!("punk check: {msg}");
                    }
                    std::process::exit(check::EXIT_NO_CONTRACT);
                }
                Err(check::CheckError::NotApproved(msg)) => {
                    if json {
                        println!(r#"{{"status":"ERROR","code":{},"message":{}}}"#,
                            check::EXIT_NOT_APPROVED,
                            serde_json::to_string(&msg).unwrap_or_default());
                    } else {
                        eprintln!("punk check: {msg}");
                    }
                    std::process::exit(check::EXIT_NOT_APPROVED);
                }
                Err(e) => {
                    let msg = e.to_string();
                    if json {
                        println!(r#"{{"status":"ERROR","code":{},"message":{}}}"#,
                            check::EXIT_INTERNAL,
                            serde_json::to_string(&msg).unwrap_or_default());
                    } else {
                        eprintln!("punk check: {msg}");
                    }
                    std::process::exit(check::EXIT_INTERNAL);
                }
            }
        }
        Commands::Receipt { json, md } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let opts = ReceiptOptions {
                root: &root,
                json,
                md,
            };

            match receipt::run_receipt(&opts) {
                Ok((task_receipt, exit_code)) => {
                    if json {
                        let j = serde_json::to_string_pretty(&task_receipt).unwrap_or_default();
                        println!("{j}");
                    } else if md {
                        print!("{}", render_receipt_md(&task_receipt));
                    } else {
                        print!("{}", render_receipt_short(&task_receipt));
                    }
                    std::process::exit(exit_code);
                }
                Err(receipt::ReceiptError::NoCheckReceipt(msg)) => {
                    eprintln!("punk receipt: {msg}");
                    std::process::exit(receipt::EXIT_NO_CHECK);
                }
                Err(receipt::ReceiptError::CheckFailed(msg)) => {
                    eprintln!("punk receipt: {msg}");
                    std::process::exit(receipt::EXIT_CHECK_FAILED);
                }
                Err(e) => {
                    eprintln!("punk receipt: {e}");
                    std::process::exit(receipt::EXIT_INTERNAL);
                }
            }
        }
        Commands::Status => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let punk_dir = root.join(".punk");

            if !punk_dir.is_dir() {
                eprintln!("punk status: no .punk/ directory. Run `punk init` first.");
                std::process::exit(1);
            }

            // Find current VCS change
            let change_id = punk_core::vcs::detect(&root)
                .and_then(|v| v.change_id())
                .unwrap_or_else(|_| "(no VCS)".to_string());

            println!("punk status: change {change_id}");

            let contract_dir = punk_dir.join("contracts").join(&change_id);
            let contract_path = contract_dir.join("contract.json");

            if !contract_path.exists() {
                println!("  contract: none");
                println!("  action:   run `punk plan` to create a contract");
            } else {
                // Load contract to show goal
                let goal = std::fs::read_to_string(&contract_path)
                    .ok()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
                    .and_then(|v| v["goal"].as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "(parse error)".to_string());

                let has_approval = std::fs::read_to_string(&contract_path)
                    .ok()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
                    .and_then(|v| v["approval_hash"].as_str().map(|_| true))
                    .unwrap_or(false);

                let closed_exists = contract_dir.join("closed.json").exists();
                let check_exists = contract_dir.join("receipts").join("check.json").exists();
                let task_exists = contract_dir.join("receipts").join("task.json").exists();

                let state = if closed_exists {
                    "CLOSED"
                } else if task_exists {
                    "COMPLETED"
                } else if check_exists {
                    "CHECKED"
                } else if has_approval {
                    "APPROVED"
                } else {
                    "DRAFT"
                };

                println!("  contract: {state}");
                println!("  goal:     {goal}");

                if state == "APPROVED" {
                    println!("  action:   implement, then run `punk check`");
                } else if state == "CHECKED" {
                    println!("  action:   run `punk receipt` to complete");
                } else if state == "DRAFT" {
                    println!("  action:   run `punk plan` to approve");
                }
            }
        }
        Commands::Close { reason } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

            // Resolve contract
            match check::resolve_contract(&root) {
                Ok((contract, contract_dir, _)) => {
                    let feedback = Feedback {
                        outcome: FeedbackOutcome::Reject,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        note: Some(format!("closed: {reason}")),
                    };
                    let fb_json = serde_json::to_string_pretty(&feedback).unwrap_or_default();
                    let _ = std::fs::write(contract_dir.join("closed.json"), &fb_json);
                    println!(
                        "punk close: contract '{}' closed — {}",
                        contract.change_id, reason
                    );
                }
                Err(e) => {
                    eprintln!("punk close: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Config { action } => {
            match action {
                ConfigAction::SetProvider { name, endpoint, api_key, model } => {
                    match punk_core::config::set_provider(&name, &endpoint, &api_key, model.as_deref()) {
                        Ok(()) => {
                            println!("Provider '{name}' saved to {}", punk_core::config::providers_path().display());
                        }
                        Err(e) => {
                            eprintln!("punk config: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                ConfigAction::RemoveProvider { name } => {
                    match punk_core::config::remove_provider(&name) {
                        Ok(()) => println!("Provider '{name}' removed"),
                        Err(e) => {
                            eprintln!("punk config: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                ConfigAction::List => {
                    match punk_core::config::load_config() {
                        Ok(config) => {
                            if config.providers.is_empty() {
                                println!("No providers configured.");
                                println!("  punk config set-provider <name> <endpoint> <api-key>");
                            } else {
                                let default = config.default_provider.as_deref().unwrap_or("");
                                for (name, p) in &config.providers {
                                    let marker = if name == default { " (default)" } else { "" };
                                    let model_str = p.model.as_deref().unwrap_or("-");
                                    println!("  {name}{marker}: {endpoint} model={model_str}",
                                        endpoint = p.endpoint);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("punk config: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                ConfigAction::Show => {
                    match punk_core::config::resolve_provider() {
                        Some(p) => {
                            println!("Active provider:");
                            println!("  Endpoint: {}", p.endpoint);
                            println!("  API key:  {}...{}", &p.api_key[..4.min(p.api_key.len())],
                                &p.api_key[p.api_key.len().saturating_sub(4)..]);
                            if let Some(model) = &p.model {
                                println!("  Model:    {model}");
                            }
                        }
                        None => {
                            println!("No active provider. Resolution order:");
                            println!("  1. PUNK_LLM_ENDPOINT + PUNK_LLM_API_KEY env vars");
                            println!("  2. ANTHROPIC_API_KEY or OPENAI_API_KEY env vars");
                            println!("  3. ~/.config/punk/providers.toml (default provider)");
                        }
                    }
                }
            }
        }
    }
}

// CLI integration tests
#[cfg(test)]
mod tests {
    use punk_core::init::{run_init, GreenFieldAnswers, InitMode};
    use tempfile::TempDir;
    use std::fs;

    #[test]
    fn init_greenfield_cli() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        let answers = GreenFieldAnswers {
            description: "CLI test project".to_string(),
            tech_stack: "Rust".to_string(),
            never_in_scope: "".to_string(),
        };

        let result = run_init(dir, Some(answers)).unwrap();
        assert_eq!(result.mode, InitMode::Greenfield);
        assert!(dir.join(".punk/config.toml").exists());
        assert!(dir.join(".punk/intent.md").exists());
        assert!(dir.join(".punk/conventions.json").exists());
    }

    #[test]
    fn init_brownfield_cli() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        // Create enough source files for brownfield detection
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n").unwrap();
        for i in 0..6 {
            fs::write(dir.join(format!("src/f{i}.rs")), "pub fn f() {}").unwrap();
        }

        let result = run_init(dir, None).unwrap();
        assert_eq!(result.mode, InitMode::Brownfield);
        assert!(dir.join(".punk/scan.json").exists());
    }
}
