use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use punk_core::init::run_init;
use punk_core::plan::{run_plan_headless, save_contract, PlanOptions};
use punk_core::plan::contract::{Feedback, FeedbackOutcome};

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
    Check,
    /// Record a receipt for the completed contract
    Receipt,
    /// Show current workspace status
    Status,
    /// Manage punk configuration
    Config,
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

            let opts = PlanOptions {
                root: &root,
                task: &task,
                manual,
                provider: None, // use HttpProvider::from_env() in real usage
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
                        // Open $EDITOR with contract JSON
                        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
                        let contract_json =
                            serde_json::to_string_pretty(&contract).unwrap_or_default();
                        let tmp = std::env::temp_dir().join("punk-contract-edit.json");
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
                        eprintln!("punk plan: aborted");
                        std::process::exit(1);
                    }
                    _ => {
                        eprintln!("punk plan: unknown choice '{choice}' — enter y/n/e/q");
                    }
                }
            }

            // Suppress unused warning on quality in this path
            let _ = quality;
        }
        Commands::Check => {
            eprintln!("punk check: not yet implemented");
            std::process::exit(1);
        }
        Commands::Receipt => {
            eprintln!("punk receipt: not yet implemented");
            std::process::exit(1);
        }
        Commands::Status => {
            eprintln!("punk status: not yet implemented");
            std::process::exit(1);
        }
        Commands::Config => {
            eprintln!("punk config: not yet implemented");
            std::process::exit(1);
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
