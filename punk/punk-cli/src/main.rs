use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use punk_core::audit::{self, render_audit_short};
use punk_core::check::{self, render_check, CheckOptions};
use punk_core::holdout::{self, render_holdout_short};
use punk_core::init::run_init;
use punk_core::mechanic::{self, render_baseline_short, render_mechanic_short};
use punk_core::pack;
use punk_core::plan::contract::{Feedback, FeedbackOutcome};
use punk_core::plan::{run_plan_headless, save_contract, PlanOptions};
use punk_core::receipt::{self, render_receipt_md, render_receipt_short, ReceiptOptions};
use punk_core::repair;

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
    /// Generate repair brief from audit findings
    Repair {
        /// Output JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Run multi-model audit (mechanic + holdouts + external review)
    Audit {
        /// Output JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Run 4-pass self-critique on active contract
    Critique,
    /// Check dependencies against deps.dev (existence + deprecation)
    DepCheck {
        /// Specific files to scan (default: all changed files)
        files: Vec<String>,
    },
    /// Static test quality analysis (assertions, mocks, tautologies)
    TestQuality {
        /// Specific test files (default: all changed test files)
        files: Vec<String>,
    },
    /// Detect ghost functions via jj predecessor chain
    Supersede,
    /// Verify contract removals and cleanup obligations
    Cleanup,
    /// Generate context pack for AI agent session
    Session,
    /// Show temporal coupling for a file (co-change patterns)
    Coupling {
        /// File to analyze
        file: String,
        /// Minimum confidence threshold (default 0.2)
        #[arg(long, default_value = "0.2")]
        min_confidence: f64,
    },
    /// Create or confirm an explanation artifact
    Explain {
        /// What changed
        #[arg(long)]
        what: Option<String>,
        /// Why this approach
        #[arg(long)]
        why: Option<String>,
        /// What can break
        #[arg(long)]
        risks: Option<String>,
        /// Confirm existing explanation
        #[arg(long)]
        confirm: Option<String>,
    },
    /// Search prior events relevant to a context (pre-action recall)
    Recall {
        /// Search query (file paths, keywords)
        query: String,
        /// Max results (default 5)
        #[arg(long, default_value = "5")]
        limit: usize,
    },
    /// Record a human invariant rule
    Remember {
        /// Rule description
        description: String,
        /// Reason for the rule
        #[arg(long)]
        reason: Option<String>,
    },
    /// Scan project conventions and optionally generate AGENTS.md
    Scan {
        /// Generate AGENTS.md file
        #[arg(long)]
        agents_md: bool,
        /// Output JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Assemble proofpack from all verification artifacts
    Pack,
    /// CI gate: read proofpack, exit 0=promote, 1=reject, 2=hold
    Ci {
        /// Path to proofpack.json (default: auto-detect from active contract)
        #[arg(long)]
        proofpack: Option<std::path::PathBuf>,
        /// Output JSON instead of summary
        #[arg(long)]
        json: bool,
    },
    /// Capture pre-change test baseline
    Baseline,
    /// Run blind holdout tests against implementation
    Holdout {
        /// Output JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Compare post-change tests against baseline (regression detection)
    Mechanic {
        /// Output JSON instead of human-readable text
        #[arg(long)]
        json: bool,
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
            let http_provider =
                resolved.map(|p| punk_core::plan::llm::HttpProvider::new(p.endpoint, p.api_key));
            let provider_ref: Option<&dyn punk_core::plan::LlmProvider> = http_provider
                .as_ref()
                .map(|p| p as &dyn punk_core::plan::LlmProvider);

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
                            let fb_json =
                                serde_json::to_string_pretty(&feedback).unwrap_or_default();
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
                        let status = std::process::Command::new(&editor).arg(&tmp).status();
                        match status {
                            Ok(s) if s.success() => {
                                // Re-read and re-validate
                                match std::fs::read_to_string(&tmp) {
                                    Ok(raw) => match serde_json::from_str(&raw) {
                                        Ok(edited) => {
                                            contract = edited;
                                            let new_quality =
                                                punk_core::plan::quality::check_quality(
                                                    &contract.acceptance_criteria,
                                                    &contract.scope.touch,
                                                    &contract.scope.dont_touch,
                                                );
                                            let new_summary =
                                                punk_core::plan::render::render_summary(
                                                    &contract,
                                                    &new_quality,
                                                );
                                            println!("{new_summary}");
                                            let feedback = Feedback {
                                                outcome: FeedbackOutcome::ApproveWithEdit,
                                                timestamp: chrono::Utc::now().to_rfc3339(),
                                                note: Some("edited in $EDITOR".to_string()),
                                            };
                                            match save_contract(&punk_dir, &mut contract, &feedback)
                                            {
                                                Ok((cp, _)) => {
                                                    println!(
                                                        "punk plan: edited contract saved to {}",
                                                        cp.display()
                                                    );
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
                            let fb_json =
                                serde_json::to_string_pretty(&feedback).unwrap_or_default();
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
                        println!(
                            r#"{{"status":"ERROR","code":{},"message":{}}}"#,
                            check::EXIT_NO_CONTRACT,
                            serde_json::to_string(&msg).unwrap_or_default()
                        );
                    } else {
                        eprintln!("punk check: {msg}");
                    }
                    std::process::exit(check::EXIT_NO_CONTRACT);
                }
                Err(check::CheckError::NotApproved(msg)) => {
                    if json {
                        println!(
                            r#"{{"status":"ERROR","code":{},"message":{}}}"#,
                            check::EXIT_NOT_APPROVED,
                            serde_json::to_string(&msg).unwrap_or_default()
                        );
                    } else {
                        eprintln!("punk check: {msg}");
                    }
                    std::process::exit(check::EXIT_NOT_APPROVED);
                }
                Err(e) => {
                    let msg = e.to_string();
                    if json {
                        println!(
                            r#"{{"status":"ERROR","code":{},"message":{}}}"#,
                            check::EXIT_INTERNAL,
                            serde_json::to_string(&msg).unwrap_or_default()
                        );
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
        Commands::Repair { json } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

            match check::resolve_contract(&root) {
                Ok((contract, contract_dir, _)) => {
                    // Load audit report
                    let audit_path = contract_dir.join("audit.json");
                    if !audit_path.exists() {
                        eprintln!("punk repair: no audit.json found. Run `punk audit` first.");
                        std::process::exit(1);
                    }
                    let audit_raw = std::fs::read_to_string(&audit_path).unwrap_or_default();
                    let audit_report: audit::AuditReport = match serde_json::from_str(&audit_raw) {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!("punk repair: parse audit.json: {e}");
                            std::process::exit(1);
                        }
                    };

                    if audit_report.decision == audit::AuditDecision::AutoOk {
                        println!("punk repair: audit already passed (AutoOk). No repair needed.");
                        std::process::exit(0);
                    }

                    // Generate brief
                    let brief = repair::generate_brief(
                        &contract.change_id,
                        1,
                        &audit_report.all_findings,
                        &contract.scope.touch,
                    );

                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&brief).unwrap_or_default()
                        );
                    } else {
                        print!("{}", repair::render_brief(&brief));
                    }

                    // Save brief
                    let brief_json = serde_json::to_string_pretty(&brief).unwrap_or_default();
                    let _ = std::fs::write(contract_dir.join("repair-brief.json"), &brief_json);
                }
                Err(e) => {
                    eprintln!("punk repair: {e}");
                    std::process::exit(2);
                }
            }
        }
        Commands::Audit { json } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

            match check::resolve_contract(&root) {
                Ok((contract, _, _)) => {
                    // Get diff
                    let diff = punk_core::vcs::detect(&root)
                        .and_then(|v| v.diff())
                        .unwrap_or_default();

                    // Assess risk
                    let assessment = punk_core::risk::assess(&contract.goal, &contract.scope);

                    // Run audit
                    let audit_input = audit::AuditInput {
                        goal: &contract.goal,
                        diff: &diff,
                        contract_id: &contract.change_id,
                        tier: &assessment.tier,
                        mechanic_regressions: 0,
                        holdout_pass_rate: 1.0,
                        ac_verified: contract.acceptance_criteria.len(),
                        ac_total: contract.acceptance_criteria.len(),
                        root: &root,
                    };
                    match audit::run_audit(&audit_input) {
                        Ok(report) => {
                            if json {
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&report).unwrap_or_default()
                                );
                            } else {
                                print!("{}", render_audit_short(&report));
                            }
                            let exit_code = match report.decision {
                                audit::AuditDecision::AutoOk => 0,
                                audit::AuditDecision::AutoBlock => 1,
                                audit::AuditDecision::HumanReview => 2,
                            };
                            std::process::exit(exit_code);
                        }
                        Err(e) => {
                            eprintln!("punk audit: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("punk audit: {e}");
                    std::process::exit(2);
                }
            }
        }
        Commands::Holdout { json } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

            match check::resolve_contract(&root) {
                Ok((contract, _, _)) => {
                    match holdout::run_holdouts(&contract, &root) {
                        Ok(report) => {
                            if json {
                                let j = serde_json::to_string_pretty(&report).unwrap_or_default();
                                println!("{j}");
                            } else {
                                print!("{}", render_holdout_short(&report));
                            }
                            let exit_code = if report.meets_threshold { 0 } else { 1 };
                            std::process::exit(exit_code);
                        }
                        Err(holdout::HoldoutError::NoHoldouts) => {
                            eprintln!("punk holdout: contract has no holdout scenarios (risk={:?}, min={})",
                                contract.risk_level,
                                holdout::min_holdouts_for_risk(&contract.risk_level));
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("punk holdout: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("punk holdout: {e}");
                    std::process::exit(2);
                }
            }
        }
        Commands::Critique => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            match check::resolve_contract(&root) {
                Ok((contract, _, _)) => {
                    let report = punk_core::critique::critique(&contract);
                    print!("{}", punk_core::critique::render_critique(&report));
                    let exit_code = match report.readiness {
                        punk_core::critique::Readiness::Go => 0,
                        punk_core::critique::Readiness::GoWithWarnings => 0,
                        punk_core::critique::Readiness::NeedsRevision => 1,
                    };
                    std::process::exit(exit_code);
                }
                Err(e) => {
                    eprintln!("punk critique: {e}");
                    std::process::exit(2);
                }
            }
        }
        Commands::DepCheck { files } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let scan_files = if files.is_empty() {
                // Get changed files from VCS
                punk_core::vcs::detect(&root)
                    .and_then(|v| v.changed_files())
                    .unwrap_or_default()
            } else {
                files
            };

            // Extract imports from all files
            let mut all_imports = Vec::new();
            for file in &scan_files {
                let path = root.join(file);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let lang = if file.ends_with(".rs") {
                        "rust"
                    } else if file.ends_with(".py") {
                        "python"
                    } else if file.ends_with(".ts") || file.ends_with(".tsx") {
                        "typescript"
                    } else if file.ends_with(".js") || file.ends_with(".jsx") {
                        "javascript"
                    } else if file.ends_with(".go") {
                        "go"
                    } else {
                        continue;
                    };
                    all_imports.extend(punk_core::depcheck::extract_imports(&content, lang));
                }
            }
            all_imports.sort();
            all_imports.dedup();

            let report = punk_core::depcheck::check_imports(&all_imports);
            print!("{}", punk_core::depcheck::render_dep_report(&report));
            std::process::exit(if report.hard_fail_count > 0 { 1 } else { 0 });
        }
        Commands::TestQuality { files } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let scan_files = if files.is_empty() {
                punk_core::vcs::detect(&root)
                    .and_then(|v| v.changed_files())
                    .unwrap_or_default()
            } else {
                files
            };

            let report = punk_core::testquality::scan_test_files(&root, &scan_files);
            print!("{}", punk_core::testquality::render_test_quality(&report));
            std::process::exit(if report.zero_assertion_count > 0 {
                1
            } else {
                0
            });
        }
        Commands::Supersede => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let report = punk_core::supersede::detect_ghosts(&root);
            print!("{}", punk_core::supersede::render_ghosts(&report));
        }
        Commands::Cleanup => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            match check::resolve_contract(&root) {
                Ok((contract, _, _)) => {
                    let report = punk_core::cleanup::run_cleanup(
                        &root,
                        &contract.removals,
                        &contract.cleanup_obligations,
                    );
                    print!("{}", punk_core::cleanup::render_cleanup(&report));
                    std::process::exit(if report.all_passed { 0 } else { 1 });
                }
                Err(e) => {
                    eprintln!("punk cleanup: {e}");
                    std::process::exit(2);
                }
            }
        }
        Commands::Session => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let pack = punk_core::session::build_context_pack(&root);
            print!("{}", punk_core::session::render_context_pack(&pack));
        }
        Commands::Coupling {
            file,
            min_confidence,
        } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let result = punk_core::coupling::find_coupling(&root, &file, min_confidence);
            print!("{}", punk_core::coupling::render_coupling(&result));
        }
        Commands::Explain {
            what,
            why,
            risks,
            confirm,
        } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            match check::resolve_contract(&root) {
                Ok((contract, contract_dir, _)) => {
                    if let Some(by) = confirm {
                        // Confirm existing explanation
                        match punk_core::explain::load(&contract_dir) {
                            Some(mut e) => {
                                punk_core::explain::confirm(&mut e, &by);
                                if let Err(err) = punk_core::explain::save(&e, &contract_dir) {
                                    eprintln!("punk explain: {err}");
                                    std::process::exit(1);
                                }
                                println!("punk explain: confirmed by {by}");
                            }
                            None => {
                                eprintln!("punk explain: no explanation found. Create one first.");
                                std::process::exit(1);
                            }
                        }
                    } else {
                        let what = what.unwrap_or_else(|| "(describe what changed)".into());
                        let why = why.unwrap_or_else(|| "(explain why this approach)".into());
                        let risks = risks.unwrap_or_else(|| "(what can break)".into());
                        let e = punk_core::explain::create_draft(
                            &contract.change_id,
                            &what,
                            &why,
                            &risks,
                        );
                        if let Err(err) = punk_core::explain::save(&e, &contract_dir) {
                            eprintln!("punk explain: {err}");
                            std::process::exit(1);
                        }
                        print!("{}", punk_core::explain::render_explanation(&e));
                    }
                }
                Err(e) => {
                    eprintln!("punk explain: {e}");
                    std::process::exit(2);
                }
            }
        }
        Commands::Recall { query, limit } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let results = punk_core::recall::recall(&root, &query, limit);
            let output = punk_core::recall::render_recall(&results);
            if output.is_empty() {
                // Silent when nothing found (by design)
            } else {
                print!("{output}");
            }
        }
        Commands::Remember {
            description,
            reason,
        } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            match punk_core::recall::remember(&root, &description, reason.as_deref()) {
                Ok(event) => {
                    println!(
                        "punk remember: saved invariant '{}' ({})",
                        event.context, event.id
                    );
                }
                Err(e) => {
                    eprintln!("punk remember: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Scan { agents_md, json } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

            // Detect language from .punk/config.toml or scan.json
            let language = {
                let config_path = root.join(".punk").join("config.toml");
                if let Ok(raw) = std::fs::read_to_string(&config_path) {
                    #[derive(serde::Deserialize)]
                    struct C {
                        #[serde(default)]
                        project: P,
                    }
                    #[derive(serde::Deserialize, Default)]
                    struct P {
                        #[serde(default)]
                        primary_language: Option<String>,
                    }
                    toml::from_str::<C>(&raw)
                        .ok()
                        .and_then(|c| c.project.primary_language)
                        .unwrap_or_else(|| "rust".to_string())
                } else {
                    "rust".to_string()
                }
            };

            let report = punk_core::scan::scan_conventions(&root, &language);

            if agents_md {
                let project_name = root
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "project".to_string());
                let md = punk_core::scan::generate_agents_md(&report, &project_name);
                let agents_path = root.join("AGENTS.md");
                if let Err(e) = std::fs::write(&agents_path, &md) {
                    eprintln!("punk scan: write AGENTS.md: {e}");
                    std::process::exit(1);
                }
                println!("punk scan: AGENTS.md written ({} bytes)", md.len());
            } else if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).unwrap_or_default()
                );
            } else {
                println!(
                    "punk scan: {} ({} findings)",
                    report.language,
                    report.findings.len()
                );
                println!(
                    "  naming:  fn={}, types={}",
                    report.naming.functions, report.naming.types
                );
                println!("  imports: {}", report.imports.style);
                println!("  errors:  {}", report.errors.style);
                println!(
                    "  tests:   {} ({})",
                    report.tests.framework, report.tests.style
                );
                if !report.imports.top_imports.is_empty() {
                    println!("  top deps: {}", report.imports.top_imports.join(", "));
                }
            }

            // Save to .punk/conventions-scan.json
            let punk_dir = root.join(".punk");
            if punk_dir.is_dir() {
                let scan_json = serde_json::to_string_pretty(&report).unwrap_or_default();
                let _ = std::fs::write(punk_dir.join("conventions-scan.json"), &scan_json);
            }
        }
        Commands::Pack => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

            match check::resolve_contract(&root) {
                Ok((contract, contract_dir, contract_raw)) => {
                    // Load audit report
                    let audit_path = contract_dir.join("audit.json");
                    if !audit_path.exists() {
                        eprintln!("punk pack: no audit.json found. Run `punk audit` first.");
                        std::process::exit(1);
                    }
                    let audit_raw = std::fs::read_to_string(&audit_path).unwrap_or_default();
                    let audit_report: punk_core::audit::AuditReport =
                        match serde_json::from_str(&audit_raw) {
                            Ok(r) => r,
                            Err(e) => {
                                eprintln!("punk pack: invalid audit.json: {e}");
                                std::process::exit(1);
                            }
                        };

                    // Get diff
                    let diff = punk_core::vcs::detect(&root)
                        .and_then(|v| v.diff())
                        .unwrap_or_default();

                    // Strip holdouts for embedded contract
                    let stripped = punk_core::holdout::strip_holdouts(&contract);
                    let stripped_json = serde_json::to_string_pretty(&stripped).unwrap_or_default();

                    // Load holdout results
                    let holdout_path = contract_dir.join("holdout.json");
                    let (ho_total, ho_passed) = if holdout_path.exists() {
                        let raw = std::fs::read_to_string(&holdout_path).unwrap_or_default();
                        if let Ok(r) =
                            serde_json::from_str::<punk_core::holdout::HoldoutReport>(&raw)
                        {
                            (r.total, r.passed)
                        } else {
                            (0, 0)
                        }
                    } else {
                        (0, 0)
                    };

                    let proofpack = pack::assemble(
                        &contract_dir,
                        &contract_raw,
                        &stripped_json,
                        &diff,
                        &audit_report,
                        ho_total,
                        ho_passed,
                    );

                    match pack::save(&proofpack, &contract_dir) {
                        Ok(path) => {
                            print!("{}", pack::render_pack_short(&proofpack));
                            println!("  saved: {}", path.display());
                        }
                        Err(e) => {
                            eprintln!("punk pack: save error: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("punk pack: {e}");
                    std::process::exit(2);
                }
            }
        }
        Commands::Ci { proofpack, json } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

            // Find proofpack path
            let pack_path = if let Some(p) = proofpack {
                p
            } else {
                // Auto-detect from active contract
                let change_id = punk_core::vcs::detect(&root)
                    .and_then(|v| v.change_id())
                    .unwrap_or_default();
                root.join(".punk")
                    .join("contracts")
                    .join(&change_id)
                    .join("proofpack.json")
            };

            match pack::ci_gate(&pack_path) {
                Ok((code, summary)) => {
                    if json {
                        println!(
                            r#"{{"code":{},"verdict":"{}","summary":{}}}"#,
                            code,
                            match code {
                                0 => "PROMOTE",
                                1 => "REJECT",
                                _ => "HOLD",
                            },
                            serde_json::to_string(&summary).unwrap_or_default()
                        );
                    } else {
                        println!("{summary}");
                    }
                    std::process::exit(code);
                }
                Err(e) => {
                    eprintln!("punk ci: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Baseline => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

            // Get contract ID from VCS
            let change_id = punk_core::vcs::detect(&root)
                .and_then(|v| v.change_id())
                .unwrap_or_else(|_| String::new());

            if change_id.is_empty() {
                eprintln!("punk baseline: could not determine VCS change id");
                std::process::exit(1);
            }

            match mechanic::capture_baseline(&root, &change_id) {
                Ok(baseline) => {
                    print!("{}", render_baseline_short(&baseline));
                    for check in &baseline.checks {
                        println!(
                            "  {} (exit {}): {} failures, {}ms",
                            check.name,
                            check.exit_code,
                            check.failures.len(),
                            check.duration_ms
                        );
                    }
                }
                Err(e) => {
                    eprintln!("punk baseline: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Mechanic { json } => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

            let change_id = punk_core::vcs::detect(&root)
                .and_then(|v| v.change_id())
                .unwrap_or_else(|_| String::new());

            if change_id.is_empty() {
                eprintln!("punk mechanic: could not determine VCS change id");
                std::process::exit(1);
            }

            match mechanic::run_mechanic(&root, &change_id) {
                Ok(report) => {
                    if json {
                        let j = serde_json::to_string_pretty(&report).unwrap_or_default();
                        println!("{j}");
                    } else {
                        print!("{}", render_mechanic_short(&report));
                    }
                    let exit_code = match report.status {
                        mechanic::MechanicStatus::Pass => 0,
                        mechanic::MechanicStatus::Regression => 1,
                        mechanic::MechanicStatus::Error => 2,
                    };
                    std::process::exit(exit_code);
                }
                Err(e) => {
                    eprintln!("punk mechanic: {e}");
                    std::process::exit(1);
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
        Commands::Config { action } => match action {
            ConfigAction::SetProvider {
                name,
                endpoint,
                api_key,
                model,
            } => {
                match punk_core::config::set_provider(&name, &endpoint, &api_key, model.as_deref())
                {
                    Ok(()) => {
                        println!(
                            "Provider '{name}' saved to {}",
                            punk_core::config::providers_path().display()
                        );
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
            ConfigAction::List => match punk_core::config::load_config() {
                Ok(config) => {
                    if config.providers.is_empty() {
                        println!("No providers configured.");
                        println!("  punk config set-provider <name> <endpoint> <api-key>");
                    } else {
                        let default = config.default_provider.as_deref().unwrap_or("");
                        for (name, p) in &config.providers {
                            let marker = if name == default { " (default)" } else { "" };
                            let model_str = p.model.as_deref().unwrap_or("-");
                            println!(
                                "  {name}{marker}: {endpoint} model={model_str}",
                                endpoint = p.endpoint
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!("punk config: {e}");
                    std::process::exit(1);
                }
            },
            ConfigAction::Show => match punk_core::config::resolve_provider() {
                Some(p) => {
                    println!("Active provider:");
                    println!("  Endpoint: {}", p.endpoint);
                    println!(
                        "  API key:  {}...{}",
                        &p.api_key[..4.min(p.api_key.len())],
                        &p.api_key[p.api_key.len().saturating_sub(4)..]
                    );
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
            },
        },
    }
}

// CLI integration tests
#[cfg(test)]
mod tests {
    use punk_core::init::{run_init, GreenFieldAnswers, InitMode};
    use std::fs;
    use tempfile::TempDir;

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
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        for i in 0..6 {
            fs::write(dir.join(format!("src/f{i}.rs")), "pub fn f() {}").unwrap();
        }

        let result = run_init(dir, None).unwrap();
        assert_eq!(result.mode, InitMode::Brownfield);
        assert!(dir.join(".punk/scan.json").exists());
    }
}
