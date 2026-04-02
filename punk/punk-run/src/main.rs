use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use punk_core::vcs::{VcsMode, detect_mode as detect_vcs_mode, enable_jj as enable_jj_for_repo};
use punk_orch::{
    benchmark, bus, config, context, daemon, diverge, doctor, eval, goal, graph, morning, ops,
    panel, pipeline, ratchet, recall, research, resolver, sanitize, skill,
};

#[derive(Parser)]
#[command(name = "punk-run", version, about = "Specpunk agent orchestration")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show current tasks, slots, and receipt summary
    Status {
        /// Number of recent completed tasks to show
        #[arg(short = 'n', long, default_value_t = 10)]
        recent: usize,
        /// Filter by project
        #[arg(long)]
        project: Option<String>,
    },
    /// Show loaded configuration
    Config,
    /// Start the daemon
    Daemon {
        /// Shadow mode: log decisions without dispatching
        #[arg(long)]
        shadow: bool,
        /// Max concurrent slots
        #[arg(long, default_value_t = 5)]
        slots: u32,
        /// Run as background service (daemonize)
        #[arg(long)]
        background: bool,
    },
    /// Daily briefing: receipts, queue, checkpoints, budget
    Morning,
    /// List failed/dead tasks for triage
    Triage,
    /// Retry a failed or dead-letter task
    Retry {
        /// Task ID to retry
        task_id: String,
    },
    /// Cancel a queued or running task
    Cancel {
        /// Task ID to cancel
        task_id: String,
    },
    /// Health check: providers, bus, config
    Doctor,
    /// Test routing rules against a task (dry run)
    PolicyCheck {
        /// Project slug
        project: String,
        /// Task category
        #[arg(long, default_value = "codegen")]
        category: String,
        /// Priority
        #[arg(long, default_value = "p1")]
        priority: String,
    },
    /// Queue a one-off task for agent dispatch
    Queue {
        /// Project slug
        project: String,
        /// Task prompt
        prompt: String,
        /// Agent/model (claude, codex, gemini)
        #[arg(long)]
        agent: Option<String>,
        /// Task category
        #[arg(long, default_value = "codegen")]
        category: String,
        /// Priority (p0, p1, p2)
        #[arg(long, default_value = "p1")]
        priority: String,
        /// Timeout in seconds
        #[arg(long, default_value_t = 600)]
        timeout: u64,
        /// Max budget in USD
        #[arg(long)]
        budget: Option<f64>,
        /// Run in isolated git worktree
        #[arg(long)]
        worktree: bool,
        /// Run after this task completes
        #[arg(long)]
        after: Option<String>,
    },
    /// Query receipt history
    Receipts {
        /// Filter by project
        #[arg(long)]
        project: Option<String>,
        /// Look back N days
        #[arg(long, default_value_t = 7)]
        since: i64,
    },
    /// Goal management (create, list, approve, pause, resume, cancel)
    Goal {
        #[command(subcommand)]
        action: GoalAction,
    },
    /// AI-powered query over state (uses Claude haiku)
    Ask {
        /// Question about tasks, goals, or project state
        question: String,
    },
    /// Pipeline CRM management
    Pipeline {
        #[command(subcommand)]
        action: PipelineAction,
    },
    /// 3-provider parallel implementation, compare and select
    Diverge {
        /// Project slug
        project: String,
        /// Implementation spec
        spec: String,
        /// Timeout per provider in seconds
        #[arg(long, default_value_t = 300)]
        timeout: u64,
    },
    /// Ask all providers the same question, compare answers
    Panel {
        /// Question to ask all providers
        question: String,
        /// Timeout per provider in seconds
        #[arg(long, default_value_t = 120)]
        timeout: u64,
    },
    /// List or create skills
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    /// Weekly performance comparison (metric ratchet)
    Ratchet,
    /// Show unified project context (guidance + skills + recall + session + stats)
    Context {
        /// Project slug
        project: String,
    },
    /// Pre-action knowledge recall — find past failures relevant to a task
    Recall {
        /// Search query (project, topic, or task description)
        query: String,
        /// Filter by project
        #[arg(long)]
        project: Option<String>,
        /// Max results
        #[arg(short = 'n', long, default_value_t = 5)]
        limit: usize,
    },
    /// Add a manual knowledge event (lesson, invariant)
    Remember {
        /// Project slug
        project: String,
        /// What happened or what to remember
        context: String,
        /// Why this matters
        #[arg(long)]
        why: String,
        /// Event type: failure, lesson, invariant
        #[arg(long, default_value = "lesson")]
        kind: String,
    },
    /// On-demand charts (cost, project distribution)
    Graph {
        /// Chart type: cost or project
        #[arg(default_value = "cost")]
        chart_type: String,
        /// Number of days to look back
        #[arg(long, default_value_t = 14)]
        since: i64,
    },
    /// Bounded deep-research packet management
    Research {
        #[command(subcommand)]
        action: ResearchAction,
    },
    /// Offline task eval records
    Eval {
        #[command(subcommand)]
        action: EvalAction,
    },
    /// Repo-local benchmark result storage
    Benchmark {
        #[command(subcommand)]
        action: BenchmarkAction,
    },
    /// Pin a project alias to a local path
    Use {
        /// Project slug
        name: String,
        /// Path to project root
        path: String,
    },
    /// Show how a project name resolves (resolution chain)
    Resolve {
        /// Project name
        name: String,
        /// Explicit path override
        #[arg(long)]
        path: Option<String>,
    },
    /// Remove a pinned project alias from cache
    Forget {
        /// Project name to unpin
        name: String,
    },
    /// List all known projects (TOML + cache + discovered)
    Projects,
    /// Generate config files from detected environment
    Init {
        /// Bootstrap the current repo as a project after normal init
        #[arg(long)]
        project: Option<String>,
    },
    /// Version-control integration helpers
    Vcs {
        #[command(subcommand)]
        action: VcsAction,
    },
}

#[derive(Subcommand)]
enum SkillAction {
    /// List all skills
    List,
    /// Create a new skill
    Create {
        name: String,
        #[arg(long)]
        description: String,
        /// Path to skill content file
        #[arg(long)]
        file: String,
        #[arg(long = "project")]
        project: Vec<String>,
        #[arg(long = "category")]
        category: Vec<String>,
    },
    /// Register a repo-local candidate skill patch with evidence refs
    Candidate {
        name: String,
        #[arg(long)]
        description: String,
        /// Path to skill content file
        #[arg(long)]
        file: String,
        /// Evidence refs such as run ids, receipts, or incident ids
        #[arg(long = "evidence", required = true)]
        evidence: Vec<String>,
        #[arg(long = "project")]
        project: Vec<String>,
        #[arg(long = "category")]
        category: Vec<String>,
    },
    /// Draft a candidate skill from an existing task receipt
    Propose {
        /// Task id to mine as evidence
        task_id: String,
        /// Optional explicit candidate skill name
        #[arg(long)]
        name: Option<String>,
    },
    /// Promote a repo-local candidate skill into active skills
    Promote {
        /// Candidate skill name
        name: String,
    },
}

#[derive(Subcommand)]
enum VcsAction {
    /// Show the current VCS mode for this repo
    Status,
    /// Enable jj for this Git repo explicitly
    EnableJj,
}

#[derive(Subcommand)]
enum ResearchAction {
    /// Freeze a new bounded research packet
    Start {
        /// Research kind
        #[arg(long)]
        kind: String,
        /// Project id
        #[arg(long)]
        project: String,
        /// Frozen research question
        question: String,
        /// Goal of the research run
        #[arg(long)]
        goal: String,
        /// Optional subject reference
        #[arg(long)]
        subject_ref: Option<String>,
        /// Constraint, can be repeated
        #[arg(long = "constraint")]
        constraint: Vec<String>,
        /// Success criterion, can be repeated
        #[arg(long = "success", required = true)]
        success: Vec<String>,
        /// Context reference, can be repeated
        #[arg(long = "context-ref")]
        context_ref: Vec<String>,
        #[arg(long)]
        max_rounds: Option<u32>,
        #[arg(long)]
        max_worker_slots: Option<u32>,
        #[arg(long)]
        max_duration_minutes: Option<u32>,
        #[arg(long)]
        max_artifacts: Option<u32>,
        #[arg(long)]
        max_cost_usd: Option<f64>,
        #[arg(long)]
        output_schema_ref: Option<String>,
    },
    /// Write structured synthesis for a frozen research run
    Synthesize {
        /// Research id
        research_id: String,
        /// Structured outcome kind
        #[arg(long)]
        outcome: String,
        /// Short synthesis title
        #[arg(long)]
        title: String,
        /// Finding, can be repeated
        #[arg(long = "finding", required = true)]
        finding: Vec<String>,
        /// Recommendation, can be repeated
        #[arg(long = "recommendation")]
        recommendation: Vec<String>,
        /// Evidence ref, can be repeated
        #[arg(long = "evidence-ref")]
        evidence_ref: Vec<String>,
        /// Unresolved question, can be repeated
        #[arg(long = "unresolved")]
        unresolved: Vec<String>,
    },
    /// Write one structured research artifact before synthesis
    Artifact {
        /// Research id
        research_id: String,
        /// Artifact kind
        #[arg(long)]
        kind: String,
        /// Artifact title
        #[arg(long)]
        title: String,
        /// Path to artifact content file
        #[arg(long)]
        file: String,
        /// Evidence ref, can be repeated
        #[arg(long = "evidence-ref")]
        evidence_ref: Vec<String>,
    },
    /// Show one research run with packet/artifacts/synthesis details
    Show {
        /// Research id
        research_id: String,
    },
    /// List frozen research runs for the current repo
    List,
}

#[derive(Subcommand)]
enum EvalAction {
    /// Evaluate one task from its latest receipt
    Task {
        /// Task id
        task_id: String,
    },
    /// Evaluate one candidate skill patch against a baseline
    Skill {
        /// Candidate skill name
        name: String,
        /// Project id
        #[arg(long)]
        project: String,
        /// Eval suite id
        #[arg(long)]
        suite: String,
        /// Optional target role
        #[arg(long)]
        role: Option<String>,
        /// Baseline contract satisfaction in [0.0, 1.0]
        #[arg(long = "baseline-contract-satisfaction")]
        baseline_contract_satisfaction: f64,
        /// Candidate contract satisfaction in [0.0, 1.0]
        #[arg(long = "candidate-contract-satisfaction")]
        candidate_contract_satisfaction: f64,
        /// Baseline target pass rate in [0.0, 1.0]
        #[arg(long = "baseline-target-pass-rate")]
        baseline_target_pass_rate: f64,
        /// Candidate target pass rate in [0.0, 1.0]
        #[arg(long = "candidate-target-pass-rate")]
        candidate_target_pass_rate: f64,
        /// Baseline blocked-run rate in [0.0, 1.0]
        #[arg(long = "baseline-blocked-run-rate")]
        baseline_blocked_run_rate: f64,
        /// Candidate blocked-run rate in [0.0, 1.0]
        #[arg(long = "candidate-blocked-run-rate")]
        candidate_blocked_run_rate: f64,
        /// Baseline escalation rate in [0.0, 1.0]
        #[arg(long = "baseline-escalation-rate")]
        baseline_escalation_rate: f64,
        /// Candidate escalation rate in [0.0, 1.0]
        #[arg(long = "candidate-escalation-rate")]
        candidate_escalation_rate: f64,
        /// Baseline scope discipline in [0.0, 1.0]
        #[arg(long = "baseline-scope-discipline")]
        baseline_scope_discipline: f64,
        /// Candidate scope discipline in [0.0, 1.0]
        #[arg(long = "candidate-scope-discipline")]
        candidate_scope_discipline: f64,
        /// Baseline integrity pass rate in [0.0, 1.0]
        #[arg(long = "baseline-integrity-pass-rate")]
        baseline_integrity_pass_rate: f64,
        /// Candidate integrity pass rate in [0.0, 1.0]
        #[arg(long = "candidate-integrity-pass-rate")]
        candidate_integrity_pass_rate: f64,
        /// Baseline cleanup completion in [0.0, 1.0]
        #[arg(long = "baseline-cleanup-completion")]
        baseline_cleanup_completion: f64,
        /// Candidate cleanup completion in [0.0, 1.0]
        #[arg(long = "candidate-cleanup-completion")]
        candidate_cleanup_completion: f64,
        /// Baseline docs parity in [0.0, 1.0]
        #[arg(long = "baseline-docs-parity")]
        baseline_docs_parity: f64,
        /// Candidate docs parity in [0.0, 1.0]
        #[arg(long = "candidate-docs-parity")]
        candidate_docs_parity: f64,
        /// Baseline drift penalty in [0.0, 1.0]
        #[arg(long = "baseline-drift-penalty")]
        baseline_drift_penalty: f64,
        /// Candidate drift penalty in [0.0, 1.0]
        #[arg(long = "candidate-drift-penalty")]
        candidate_drift_penalty: f64,
        /// Number of weighted suite cases
        #[arg(long = "suite-size")]
        suite_size: usize,
        /// Evidence ref, can be repeated
        #[arg(long = "evidence-ref")]
        evidence_ref: Vec<String>,
        /// Free-form note, can be repeated
        #[arg(long = "note")]
        note: Vec<String>,
    },
    /// List stored task eval results
    List,
    /// List stored skill eval results
    SkillList,
    /// Aggregate stored skill eval results
    SkillSummary {
        /// Optional project filter
        #[arg(long)]
        project: Option<String>,
        /// Optional skill filter
        #[arg(long)]
        skill: Option<String>,
        /// Limit to newest N skill eval records
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Aggregate stored task eval results
    Summary {
        /// Optional project filter
        #[arg(long)]
        project: Option<String>,
        /// Limit to newest N eval records
        #[arg(long)]
        limit: Option<usize>,
    },
}

#[derive(Subcommand)]
enum BenchmarkAction {
    /// Record one benchmark result
    Record {
        /// Benchmark suite name
        #[arg(long)]
        suite: String,
        /// Project id
        #[arg(long)]
        project: String,
        /// Outcome: pass, fail, flaky
        #[arg(long)]
        outcome: String,
        /// Score in [0.0, 1.0]
        #[arg(long)]
        score: f64,
        /// Optional subject reference
        #[arg(long)]
        subject_ref: Option<String>,
        /// Metric in `name=value` form, can be repeated
        #[arg(long = "metric")]
        metric: Vec<String>,
        /// Free-form note, can be repeated
        #[arg(long = "note")]
        note: Vec<String>,
    },
    /// List stored benchmark results
    List,
    /// Show one benchmark result
    Show {
        /// Benchmark id
        benchmark_id: String,
    },
    /// Aggregate stored benchmark results
    Summary {
        /// Optional project filter
        #[arg(long)]
        project: Option<String>,
        /// Optional suite filter
        #[arg(long)]
        suite: Option<String>,
        /// Limit to newest N benchmark results
        #[arg(long)]
        limit: Option<usize>,
    },
}

#[derive(Subcommand)]
enum PipelineAction {
    /// List current opportunities
    List,
    /// Add a new opportunity
    Add {
        project: String,
        contact: String,
        #[arg(long)]
        next_step: String,
        #[arg(long)]
        due: String,
        #[arg(long)]
        value: Option<u32>,
    },
    /// Advance opportunity to next stage
    Advance { id: u32 },
    /// Mark opportunity as won
    Win { id: u32 },
    /// Mark opportunity as lost
    Lose { id: u32 },
    /// Show overdue opportunities
    Stale,
}

#[derive(Subcommand)]
enum GoalAction {
    /// Create a new goal with planner
    Create {
        project: String,
        objective: String,
        #[arg(long, default_value_t = 5.0)]
        budget: f64,
        #[arg(long)]
        deadline: Option<String>,
        #[arg(long)]
        approve: bool,
    },
    /// List all goals
    List,
    /// Show detailed goal status
    Status { goal_id: String },
    /// Approve a pending plan
    Approve { goal_id: String },
    /// Pause an active goal
    Pause { goal_id: String },
    /// Resume a paused goal
    Resume { goal_id: String },
    /// Adjust goal budget
    Budget { goal_id: String, usd: f64 },
    /// Force re-plan (generates new plan, requires re-approval)
    Replan { goal_id: String },
    /// Cancel a goal
    Cancel { goal_id: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Status { recent, project } => cmd_status(recent, project.as_deref()),
        Command::Config => cmd_config(),
        Command::Daemon {
            shadow,
            slots,
            background,
        } => {
            if background {
                // Fork to background
                let exe = std::env::current_exe().unwrap();
                let mut cmd = std::process::Command::new(exe);
                cmd.args(["daemon"]);
                if shadow {
                    cmd.arg("--shadow");
                }
                if slots != 5 {
                    cmd.args(["--slots", &slots.to_string()]);
                }
                cmd.stdin(std::process::Stdio::null());
                cmd.stdout(std::process::Stdio::null());
                cmd.stderr(
                    std::fs::File::create(
                        bus::bus_dir()
                            .parent()
                            .unwrap_or(&bus::bus_dir())
                            .join("daemon.log"),
                    )
                    .map(std::process::Stdio::from)
                    .unwrap_or(std::process::Stdio::null()),
                );
                match cmd.spawn() {
                    Ok(child) => println!("Daemon started (PID {})", child.id()),
                    Err(e) => {
                        eprintln!("Failed to start daemon: {e}");
                        std::process::exit(1);
                    }
                }
                return Ok(());
            }
            // Wire policy max_slots if CLI didn't override
            let effective_slots = if slots != 5 {
                slots
            } else {
                load_config_or_exit(&config::config_dir())
                    .policy
                    .defaults
                    .max_slots
            };
            let dcfg = daemon::DaemonConfig {
                shadow,
                max_slots: effective_slots,
                ..Default::default()
            };
            daemon::run(dcfg).await;
        }
        Command::Morning => {
            let bus_path = bus::bus_dir();
            let config_dir = config::config_dir();
            print!("{}", morning::briefing(&bus_path, &config_dir));
        }
        Command::Triage => cmd_triage(),
        Command::Retry { task_id } => {
            let bus_path = bus::bus_dir();
            match ops::retry_task(&bus_path, &task_id) {
                Ok(outcome) => println!("{}", format_retry_outcome(&outcome)),
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Cancel { task_id } => {
            let bus_path = bus::bus_dir();
            match ops::cancel_task(&bus_path, &task_id) {
                Ok(outcome) => println!("{}", format_cancel_outcome(&outcome)),
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Doctor => {
            let bus_path = bus::bus_dir();
            let config_dir = config::config_dir();
            let repo_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let report = doctor::check_all(&bus_path, &config_dir, &repo_path);
            print!("{}", report.display());
        }
        Command::PolicyCheck {
            project,
            category,
            priority,
        } => {
            cmd_policy_check(&project, &category, &priority);
        }
        Command::Queue {
            project,
            prompt,
            agent,
            category,
            priority,
            timeout,
            budget,
            worktree,
            after,
        } => {
            cmd_queue(
                &project,
                &prompt,
                agent.as_deref(),
                &category,
                &priority,
                timeout,
                budget,
                worktree,
                after.as_deref(),
            );
        }
        Command::Receipts { project, since } => {
            cmd_receipts(project.as_deref(), since);
        }
        Command::Ask { question } => cmd_ask(&question).await,
        Command::Pipeline { action } => match action {
            PipelineAction::List => cmd_pipeline_list(),
            PipelineAction::Add {
                project,
                contact,
                next_step,
                due,
                value,
            } => {
                let bus_path = bus::bus_dir();
                match pipeline::add(&bus_path, &project, &contact, &next_step, &due, value) {
                    Ok(opp) => println!(
                        "Added: #{} {} ({}) -> {}",
                        opp.id, opp.contact, opp.project, opp.next_step
                    ),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            PipelineAction::Advance { id } => {
                let bus_path = bus::bus_dir();
                match pipeline::advance(&bus_path, id) {
                    Ok(opp) => println!("#{}: -> {:?}", opp.id, opp.stage),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            PipelineAction::Win { id } => {
                let bus_path = bus::bus_dir();
                match pipeline::set_stage(&bus_path, id, pipeline::Stage::Won) {
                    Ok(opp) => println!("#{}: WON", opp.id),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            PipelineAction::Stale => {
                let bus_path = bus::bus_dir();
                let opps = pipeline::load_pipeline(&bus_path);
                let today = punk_orch::chrono::Utc::now().format("%Y-%m-%d").to_string();
                let stale: Vec<_> = opps
                    .iter()
                    .filter(|o| {
                        o.due < today
                            && o.stage != pipeline::Stage::Won
                            && o.stage != pipeline::Stage::Lost
                    })
                    .collect();
                if stale.is_empty() {
                    println!("No stale opportunities.");
                } else {
                    println!("Stale opportunities ({}):\n", stale.len());
                    for o in &stale {
                        println!(
                            "  #{} {} ({}) — due {} — {:?}",
                            o.id, o.contact, o.project, o.due, o.stage
                        );
                    }
                }
            }
            PipelineAction::Lose { id } => {
                let bus_path = bus::bus_dir();
                match pipeline::set_stage(&bus_path, id, pipeline::Stage::Lost) {
                    Ok(opp) => println!("#{}: LOST", opp.id),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
        },
        Command::Diverge {
            project,
            spec,
            timeout,
        } => {
            cmd_diverge(&project, &spec, timeout).await;
        }
        Command::Panel { question, timeout } => {
            cmd_panel(&question, timeout).await;
        }
        Command::Skill { action } => match action {
            SkillAction::List => {
                let bus_path = bus::bus_dir();
                let cwd = std::env::current_dir().ok();
                let skills = skill::list_skills(&bus_path, cwd.as_deref());
                if skills.is_empty() {
                    println!("No skills.");
                } else {
                    println!("Skills ({})\n", skills.len());
                    for s in &skills {
                        let evidence = if s.evidence_refs.is_empty() {
                            String::new()
                        } else {
                            format!(" [{} evidence]", s.evidence_refs.len())
                        };
                        println!(
                            "  {:<20} {:<10} {}{}",
                            s.name,
                            s.state.as_str(),
                            s.description,
                            evidence
                        );
                    }
                }
            }
            SkillAction::Create {
                name,
                description,
                file,
                project,
                category,
            } => {
                let bus_path = bus::bus_dir();
                let content = std::fs::read_to_string(&file).unwrap_or_else(|e| {
                    eprintln!("Error reading {file}: {e}");
                    std::process::exit(1);
                });
                match skill::create_skill_with_triggers(
                    &bus_path,
                    &name,
                    &description,
                    &content,
                    &project,
                    &category,
                ) {
                    Ok(path) => println!("Created: {}", path.display()),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            SkillAction::Candidate {
                name,
                description,
                file,
                evidence,
                project,
                category,
            } => {
                let cwd = std::env::current_dir().unwrap_or_else(|e| {
                    eprintln!("Error reading current directory: {e}");
                    std::process::exit(1);
                });
                let content = std::fs::read_to_string(&file).unwrap_or_else(|e| {
                    eprintln!("Error reading {file}: {e}");
                    std::process::exit(1);
                });
                match skill::create_candidate_skill_with_triggers(
                    &cwd,
                    &name,
                    &description,
                    &content,
                    &evidence,
                    &project,
                    &category,
                ) {
                    Ok(path) => println!("Created candidate: {}", path.display()),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            SkillAction::Propose { task_id, name } => {
                let bus_path = bus::bus_dir();
                let cwd = std::env::current_dir().unwrap_or_else(|e| {
                    eprintln!("Error reading current directory: {e}");
                    std::process::exit(1);
                });
                match skill::propose_candidate_from_task(&bus_path, &cwd, &task_id, name.as_deref())
                {
                    Ok(path) => println!("Created candidate proposal: {}", path.display()),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            SkillAction::Promote { name } => {
                let bus_path = bus::bus_dir();
                let cwd = std::env::current_dir().unwrap_or_else(|e| {
                    eprintln!("Error reading current directory: {e}");
                    std::process::exit(1);
                });
                match skill::promote_candidate_skill(&bus_path, &cwd, &name) {
                    Ok(path) => println!("Promoted candidate to active skill: {}", path.display()),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
        },
        Command::Context { project } => {
            let bus_path = bus::bus_dir();
            let config_dir = config::config_dir();
            print!(
                "{}",
                context::format_context_report(&bus_path, &project, &config_dir)
            );
        }
        Command::Recall {
            query,
            project,
            limit,
        } => {
            let bus_path = bus::bus_dir();
            let events = recall::recall(&bus_path, &query, project.as_deref(), limit);
            if events.is_empty() {
                println!("No relevant knowledge found for: {query}");
            } else {
                print!("{}", recall::format_recall(&events));
            }
        }
        Command::Remember {
            project,
            context,
            why,
            kind,
        } => {
            let bus_path = bus::bus_dir();
            let event_kind = match kind.as_str() {
                "invariant" => recall::EventKind::Invariant,
                "failure" => recall::EventKind::Failure,
                "lesson" => recall::EventKind::Lesson,
                _ => recall::EventKind::Lesson,
            };
            match recall::add_manual(&bus_path, &project, event_kind, &context, &why) {
                Ok(()) => println!("Remembered: [{kind}] {context}"),
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Ratchet => cmd_ratchet(),
        Command::Graph { chart_type, since } => {
            let bus_path = bus::bus_dir();
            match chart_type.as_str() {
                "cost" => print!("{}", graph::cost_chart(&bus_path, since)),
                "project" => print!("{}", graph::project_chart(&bus_path, since)),
                "gantt" => print!("{}", graph::gantt_chart(&bus_path, since)),
                _ => eprintln!("Unknown chart type: {chart_type}. Available: cost, project, gantt"),
            }
        }
        Command::Research { action } => match action {
            ResearchAction::Start {
                kind,
                project,
                question,
                goal,
                subject_ref,
                constraint,
                success,
                context_ref,
                max_rounds,
                max_worker_slots,
                max_duration_minutes,
                max_artifacts,
                max_cost_usd,
                output_schema_ref,
            } => {
                cmd_research_start(
                    &kind,
                    &project,
                    &question,
                    &goal,
                    subject_ref.as_deref(),
                    &constraint,
                    &success,
                    &context_ref,
                    max_rounds,
                    max_worker_slots,
                    max_duration_minutes,
                    max_artifacts,
                    max_cost_usd,
                    output_schema_ref.as_deref(),
                );
            }
            ResearchAction::Synthesize {
                research_id,
                outcome,
                title,
                finding,
                recommendation,
                evidence_ref,
                unresolved,
            } => cmd_research_synthesize(
                &research_id,
                &outcome,
                &title,
                &finding,
                &recommendation,
                &evidence_ref,
                &unresolved,
            ),
            ResearchAction::Artifact {
                research_id,
                kind,
                title,
                file,
                evidence_ref,
            } => cmd_research_artifact(&research_id, &kind, &title, &file, &evidence_ref),
            ResearchAction::Show { research_id } => cmd_research_show(&research_id),
            ResearchAction::List => cmd_research_list(),
        },
        Command::Eval { action } => match action {
            EvalAction::Task { task_id } => cmd_eval_task(&task_id),
            EvalAction::List => cmd_eval_list(),
            EvalAction::Skill {
                name,
                project,
                suite,
                role,
                baseline_contract_satisfaction,
                candidate_contract_satisfaction,
                baseline_target_pass_rate,
                candidate_target_pass_rate,
                baseline_blocked_run_rate,
                candidate_blocked_run_rate,
                baseline_escalation_rate,
                candidate_escalation_rate,
                baseline_scope_discipline,
                candidate_scope_discipline,
                baseline_integrity_pass_rate,
                candidate_integrity_pass_rate,
                baseline_cleanup_completion,
                candidate_cleanup_completion,
                baseline_docs_parity,
                candidate_docs_parity,
                baseline_drift_penalty,
                candidate_drift_penalty,
                suite_size,
                evidence_ref,
                note,
            } => cmd_eval_skill(
                &name,
                &project,
                &suite,
                role.as_deref(),
                baseline_contract_satisfaction,
                candidate_contract_satisfaction,
                baseline_target_pass_rate,
                candidate_target_pass_rate,
                baseline_blocked_run_rate,
                candidate_blocked_run_rate,
                baseline_escalation_rate,
                candidate_escalation_rate,
                baseline_scope_discipline,
                candidate_scope_discipline,
                baseline_integrity_pass_rate,
                candidate_integrity_pass_rate,
                baseline_cleanup_completion,
                candidate_cleanup_completion,
                baseline_docs_parity,
                candidate_docs_parity,
                baseline_drift_penalty,
                candidate_drift_penalty,
                suite_size,
                &evidence_ref,
                &note,
            ),
            EvalAction::SkillList => cmd_eval_skill_list(),
            EvalAction::SkillSummary {
                project,
                skill,
                limit,
            } => cmd_eval_skill_summary(project.as_deref(), skill.as_deref(), limit),
            EvalAction::Summary { project, limit } => cmd_eval_summary(project.as_deref(), limit),
        },
        Command::Benchmark { action } => match action {
            BenchmarkAction::Record {
                suite,
                project,
                outcome,
                score,
                subject_ref,
                metric,
                note,
            } => cmd_benchmark_record(
                &suite,
                &project,
                &outcome,
                score,
                subject_ref.as_deref(),
                &metric,
                &note,
            ),
            BenchmarkAction::List => cmd_benchmark_list(),
            BenchmarkAction::Show { benchmark_id } => cmd_benchmark_show(&benchmark_id),
            BenchmarkAction::Summary {
                project,
                suite,
                limit,
            } => cmd_benchmark_summary(project.as_deref(), suite.as_deref(), limit),
        },
        Command::Goal { action } => match action {
            GoalAction::Create {
                project,
                objective,
                budget,
                deadline,
                approve,
            } => cmd_goal(&project, &objective, budget, deadline.as_deref(), approve),
            GoalAction::List => cmd_goals(),
            GoalAction::Status { goal_id } => cmd_goal_status(&goal_id),
            GoalAction::Approve { goal_id } => cmd_approve(&goal_id),
            GoalAction::Pause { goal_id } => {
                cmd_goal_set_status(&goal_id, goal::GoalStatus::Paused)
            }
            GoalAction::Resume { goal_id } => {
                cmd_goal_set_status(&goal_id, goal::GoalStatus::Active)
            }
            GoalAction::Cancel { goal_id } => {
                cmd_goal_set_status(&goal_id, goal::GoalStatus::Failed)
            }
            GoalAction::Budget { goal_id, usd } => {
                let bus_path = bus::bus_dir();
                let mut g = match goal::load_goal(&bus_path, &goal_id) {
                    Some(g) => g,
                    None => {
                        eprintln!("Goal not found: {goal_id}");
                        std::process::exit(1);
                    }
                };
                if usd < 0.0 {
                    eprintln!("Budget must be non-negative.");
                    std::process::exit(1);
                }
                if usd < g.spent_usd {
                    eprintln!(
                        "Budget ${usd:.2} cannot be lower than already spent ${:.2}.",
                        g.spent_usd
                    );
                    std::process::exit(1);
                }
                let old_budget_usd = g.budget_usd;
                g.budget_usd = usd;
                goal::save_goal(&bus_path, &g).ok();
                print!(
                    "{}",
                    format_goal_budget_outcome(GoalBudgetOutcome {
                        goal_id: &goal_id,
                        project: &g.project,
                        old_budget_usd,
                        new_budget_usd: usd,
                        spent_usd: g.spent_usd,
                    })
                );
            }
            GoalAction::Replan { goal_id } => {
                let bus_path = bus::bus_dir();
                let mut g = match goal::load_goal(&bus_path, &goal_id) {
                    Some(g) => g,
                    None => {
                        eprintln!("Goal not found: {goal_id}");
                        std::process::exit(1);
                    }
                };
                if goal_has_inflight_steps(&g) {
                    eprintln!("Cannot replan while queued or running goal steps still exist.");
                    std::process::exit(1);
                }
                g.plan = None;
                g.status = goal::GoalStatus::Planning;
                g.status_reason = None;
                g.completed_at = None;
                goal::save_goal(&bus_path, &g).ok();
                print!(
                    "{}",
                    format_goal_replan_outcome(GoalReplanOutcome {
                        goal_id: &goal_id,
                        project: &g.project,
                        objective: &g.objective,
                    })
                );
            }
        },
        Command::Use { name, path } => cmd_use(&name, &path),
        Command::Resolve { name, path } => cmd_resolve(&name, path.as_deref()),
        Command::Forget { name } => cmd_forget(&name),
        Command::Projects => cmd_projects(),
        Command::Init { project } => cmd_init(project.as_deref()),
        Command::Vcs { action } => match action {
            VcsAction::Status => cmd_vcs_status(),
            VcsAction::EnableJj => cmd_vcs_enable_jj(),
        },
    }

    Ok(())
}

fn cmd_status(recent_limit: usize, project_filter: Option<&str>) {
    let bus_path = bus::bus_dir();
    let known_projects = match config::load_or_default(&config::config_dir()) {
        Ok(cfg) => resolver::list_known(Some(&cfg)),
        Err(_) => resolver::list_known(None),
    };
    let cwd = std::env::current_dir().ok();
    let resolved_project_filter = finalize_status_project_filter(
        resolve_status_project_filter(project_filter, cwd.as_deref(), &known_projects),
        cwd.as_deref(),
    );
    let bus_recent_limit = if resolved_project_filter.is_some() {
        usize::MAX
    } else {
        recent_limit
    };
    let mut state = bus::read_state(&bus_path, bus_recent_limit);

    // Apply project filter
    if let Some(proj) = resolved_project_filter.as_deref() {
        state.queued.retain(|t| t.project == proj);
        state.running.retain(|t| t.project == proj);
        state.done.retain(|t| t.project == proj);
        state.failed.retain(|t| t.project == proj);
        state.done.truncate(recent_limit);
    }

    println!(
        "Scope: {}",
        format_status_scope_label(resolved_project_filter.as_deref())
    );
    println!();

    println!(
        "Running ({} task{})",
        state.running.len(),
        if state.running.len() == 1 { "" } else { "s" }
    );
    if state.running.is_empty() {
        println!("  (none)");
    } else {
        println!("  {:<40} {:<12} {:<8} CATEGORY", "ID", "PROJECT", "MODEL");
        for t in &state.running {
            println!(
                "  {:<40} {:<12} {:<8} {}",
                truncate(&t.id, 40),
                t.project,
                t.model,
                t.category
            );
        }
    }
    println!();

    println!(
        "Queued ({} task{})",
        state.queued.len(),
        if state.queued.len() == 1 { "" } else { "s" }
    );
    if state.queued.is_empty() {
        println!("  (none)");
    } else {
        println!(
            "  {:<40} {:<12} {:<8} {:<8} CATEGORY",
            "ID", "PROJECT", "MODEL", "PRI"
        );
        for t in &state.queued {
            println!(
                "  {:<40} {:<12} {:<8} {:<8} {}",
                truncate(&t.id, 40),
                t.project,
                t.model,
                t.priority,
                t.category
            );
        }
    }
    println!();

    println!("Recent ({} shown)", state.done.len());
    if state.done.is_empty() {
        println!("  (none)");
    } else {
        println!(
            "  {:<40} {:<12} {:<8} {:<9} {:>7} {:>6}",
            "ID", "PROJECT", "MODEL", "STATUS", "COST", "TIME"
        );
        for t in &state.done {
            println!(
                "  {:<40} {:<12} {:<8} {:<9} {:>7} {:>5}s",
                truncate(&t.id, 40),
                t.project,
                t.model,
                t.status,
                format_cost(t.cost_usd),
                t.duration_s
            );
        }
    }
    println!();

    if !state.failed.is_empty() {
        println!("Failed ({} pending triage)", state.failed.len());
        println!("  {:<40} {:<12} {:<8} CATEGORY", "ID", "PROJECT", "MODEL");
        for t in &state.failed {
            println!(
                "  {:<40} {:<12} {:<8} {}",
                truncate(&t.id, 40),
                t.project,
                t.model,
                t.category
            );
        }
        println!();
    }

    let total_cost: f64 = state.done.iter().map(|t| t.cost_usd).sum();
    let success_count = state.done.iter().filter(|t| t.status == "success").count();
    let fail_count = state.done.iter().filter(|t| t.status != "success").count();
    println!(
        "Summary: {} done ({} ok, {} fail), ${:.2} total cost",
        state.done.len(),
        success_count,
        fail_count,
        total_cost
    );
    println!(
        "{}",
        goal::format_goal_summary_line_for_project(&bus_path, resolved_project_filter.as_deref())
    );
    if let Some(attention) =
        goal::format_goal_attention_line_for_project(&bus_path, resolved_project_filter.as_deref())
    {
        println!("{attention}");
    }
    if let Ok(skill_eval_summary) = eval::summarize_skill_evals(
        Path::new("."),
        Some(recent_limit),
        resolved_project_filter.as_deref(),
        None,
    ) {
        println!(
            "{}",
            eval::format_skill_eval_summary_line(&skill_eval_summary)
        );
    }
    if let Ok(benchmark_summary) = benchmark::summarize_benchmarks(
        Path::new("."),
        Some(recent_limit),
        resolved_project_filter.as_deref(),
        None,
    ) {
        println!(
            "{}",
            benchmark::format_benchmark_summary_line(&benchmark_summary)
        );
    }
}

fn resolve_status_project_filter(
    explicit_project: Option<&str>,
    cwd: Option<&Path>,
    known_projects: &[resolver::ResolvedProject],
) -> Option<String> {
    explicit_project
        .map(str::to_string)
        .or_else(|| cwd.and_then(|cwd| infer_project_from_cwd(cwd, known_projects)))
}

fn infer_project_from_cwd(
    cwd: &Path,
    known_projects: &[resolver::ResolvedProject],
) -> Option<String> {
    known_projects
        .iter()
        .filter(|project| cwd.starts_with(&project.path))
        .max_by_key(|project| project.path.components().count())
        .map(|project| project.id.clone())
}

fn format_status_scope_label(project_filter: Option<&str>) -> String {
    match project_filter {
        Some(project) => format!("project:{project}"),
        None => "global".to_string(),
    }
}

fn finalize_status_project_filter(
    resolved_project_filter: Option<String>,
    cwd: Option<&Path>,
) -> Option<String> {
    resolved_project_filter.or_else(|| infer_git_repo_root_basename(cwd))
}

fn infer_git_repo_root_basename(cwd: Option<&Path>) -> Option<String> {
    detect_git_repo_root(cwd?).and_then(|root| {
        root.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
    })
}

#[cfg(test)]
mod status_scope_tests {
    use super::{finalize_status_project_filter, infer_git_repo_root_basename};
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("punk-run-{label}-{nanos}"))
    }

    fn create_dir(path: &Path) {
        fs::create_dir_all(path).expect("create temp dir");
    }

    #[test]
    fn explicit_project_scope_keeps_precedence_over_git_repo_fallback() {
        let repo_root = unique_temp_dir("explicit-precedence");
        let nested = repo_root.join("nested").join("deeper");
        create_dir(&nested);
        create_dir(&repo_root.join(".git"));

        let resolved = finalize_status_project_filter(
            Some("known-project".to_string()),
            Some(nested.as_path()),
        );

        assert_eq!(resolved.as_deref(), Some("known-project"));

        fs::remove_dir_all(&repo_root).expect("remove temp dir");
    }

    #[test]
    fn unknown_nested_git_repo_falls_back_to_repo_root_basename() {
        let repo_root = unique_temp_dir("nested-repo");
        let nested = repo_root.join("a").join("b");
        create_dir(&nested);
        create_dir(&repo_root.join(".git"));

        let inferred = infer_git_repo_root_basename(Some(nested.as_path()));
        let resolved = finalize_status_project_filter(None, Some(nested.as_path()));
        let expected = repo_root
            .file_name()
            .and_then(|name| name.to_str())
            .expect("repo root basename");

        assert_eq!(inferred.as_deref(), Some(expected));
        assert_eq!(resolved.as_deref(), Some(expected));

        fs::remove_dir_all(&repo_root).expect("remove temp dir");
    }

    #[test]
    fn non_git_cwd_falls_back_to_global_scope() {
        let outside = unique_temp_dir("non-git");
        let nested = outside.join("workspace").join("leaf");
        create_dir(&nested);

        assert_eq!(infer_git_repo_root_basename(Some(nested.as_path())), None);
        assert_eq!(
            finalize_status_project_filter(None, Some(nested.as_path())),
            None
        );

        fs::remove_dir_all(&outside).expect("remove temp dir");
    }
}

fn cmd_config() {
    maybe_warn_jj_degraded_mode();
    let dir = config::config_dir();
    let status = config::config_status(&dir);
    let label = if status.is_complete() {
        "complete"
    } else if status.is_empty() {
        "using defaults"
    } else {
        "partial"
    };
    println!("Config dir: {} ({})\n", dir.display(), label);

    let cfg = load_config_or_exit(&dir);

    let active: Vec<_> = cfg.projects.projects.iter().filter(|p| p.active).collect();
    if !active.is_empty() {
        println!("Projects ({} active)", active.len());
        println!(
            "  {:<15} {:<40} {:<8} {:>8}",
            "ID", "PATH", "STACK", "BUDGET"
        );
        for p in &active {
            println!(
                "  {:<15} {:<40} {:<8} {:>7}",
                p.id,
                truncate(&p.path, 40),
                truncate(&p.stack, 8),
                format_cost(p.budget_usd)
            );
        }
        println!();
    }

    let mut agents: Vec<_> = cfg.agents.agents.iter().collect();
    agents.sort_by_key(|(k, _)| (*k).clone());
    let agents_label = if status.has_agents {
        ""
    } else {
        " (autodetected)"
    };
    println!("Agents ({}){}", agents.len(), agents_label);
    println!(
        "  {:<22} {:<10} {:<16} {:<10} {:>8}",
        "ID", "PROVIDER", "MODEL", "ROLE", "BUDGET"
    );
    for (id, a) in &agents {
        println!(
            "  {:<22} {:<10} {:<16} {:<10} {:>7}",
            id,
            a.provider,
            a.model,
            a.role,
            format_cost(a.budget_usd)
        );
    }
    println!();

    let d = &cfg.policy.defaults;
    let policy_label = if status.has_policy { "" } else { " (defaults)" };
    println!("Policy{policy_label}");
    println!(
        "  defaults: model={}, budget=${:.2}, timeout={}s, slots={}",
        d.model, d.budget_usd, d.timeout_s, d.max_slots
    );
    let b = &cfg.policy.budget;
    println!(
        "  budget: ${:.0}/mo ceiling, {}% soft, {}% hard, 95% stop",
        b.monthly_ceiling_usd, b.soft_alert_pct, b.hard_stop_pct
    );
    println!("  rules: {}", cfg.policy.rules.len());
    for r in &cfg.policy.rules {
        let m: Vec<_> = r
            .match_criteria
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let s: Vec<_> = r.set.iter().map(|(k, v)| format!("{k}={v}")).collect();
        println!("    {} -> {}", m.join(", "), s.join(", "));
    }

    if !cfg.policy.features.is_empty() {
        let enabled: Vec<_> = cfg
            .policy
            .features
            .iter()
            .filter(|(_, v)| v.as_bool() == Some(true))
            .map(|(k, _)| k.as_str())
            .collect();
        let disabled: Vec<_> = cfg
            .policy
            .features
            .iter()
            .filter(|(_, v)| v.as_bool() == Some(false))
            .map(|(k, _)| k.as_str())
            .collect();
        if !enabled.is_empty() {
            println!("  features ON: {}", enabled.join(", "));
        }
        if !disabled.is_empty() {
            println!("  features OFF: {}", disabled.join(", "));
        }
    }

    if !status.is_complete() {
        println!("\nHint: punk-run init  (generate config from detected environment)");
    }
}

fn cmd_triage() {
    let bus_path = bus::bus_dir();
    let entries = ops::list_triage(&bus_path);
    print!("{}", format_triage_report(&entries));
}

fn format_retry_outcome(outcome: &ops::RetryOutcome) -> String {
    format!(
        "Requeued: {} (project={}, model={}) from {} -> {}",
        outcome.task_id, outcome.project, outcome.model, outcome.source, outcome.destination
    )
}

fn format_cancel_outcome(outcome: &ops::CancelOutcome) -> String {
    match outcome {
        ops::CancelOutcome::Queued {
            task_id,
            queue_lane,
        } => format!("Cancelled queued task: {task_id} ({queue_lane})"),
        ops::CancelOutcome::Running {
            task_id,
            signal_path,
        } => format!("Cancellation signaled for running task: {task_id}\n  signal: {signal_path}"),
    }
}

fn format_triage_report(entries: &[ops::TriageEntry]) -> String {
    if entries.is_empty() {
        return "No tasks pending triage.\n".to_string();
    }

    let dead_count = entries
        .iter()
        .filter(|entry| entry.source == "dead")
        .count();
    let failed_count = entries
        .iter()
        .filter(|entry| entry.source == "failed")
        .count();

    let mut out = format!("Tasks pending triage ({})\n", entries.len());
    out.push_str(&format!(
        "Sources: dead-letter={} failed={}\n",
        dead_count, failed_count
    ));

    append_triage_group(
        &mut out,
        entries,
        "dead",
        "Dead-letter",
        "inspect the dead-letter receipt, then retry if the task is still valid or cancel it if obsolete",
    );
    append_triage_group(
        &mut out,
        entries,
        "failed",
        "Failed",
        "review the error excerpt, fix the root cause, then retry or cancel the task",
    );

    out.push_str("\nActions:\n");
    out.push_str("  punk-run retry <id>\n");
    out.push_str("  punk-run cancel <id>\n");
    out
}

fn append_triage_group(
    out: &mut String,
    entries: &[ops::TriageEntry],
    source: &str,
    title: &str,
    hint: &str,
) {
    let group: Vec<_> = entries
        .iter()
        .filter(|entry| entry.source == source)
        .collect();
    if group.is_empty() {
        return;
    }

    out.push_str(&format!("\n{} ({})\n", title, group.len()));
    out.push_str(&format!(
        "  {:<40} {:<12} {:<8} ERROR\n",
        "ID", "PROJECT", "MODEL"
    ));

    for entry in group {
        out.push_str(&format!(
            "  {:<40} {:<12} {:<8} {}\n",
            truncate(&entry.task_id, 40),
            entry.project,
            entry.model,
            truncate(&entry.error_excerpt, 48)
        ));
    }

    out.push_str(&format!("  Hint: {hint}\n"));
}

fn cmd_goal(project: &str, objective: &str, budget: f64, deadline: Option<&str>, approve: bool) {
    let bus_path = bus::bus_dir();
    let latest = punk_orch::run::latest_run_triage(&bus_path, project);
    if latest.verdict == punk_orch::run::TriageVerdict::StillAlive {
        eprintln!(
            "{}",
            format_still_alive_guard(&latest, project, "goal planning")
        );
        std::process::exit(1);
    }
    let cfg = load_config_or_exit(&config::config_dir());

    let mut g = match goal::create_goal(&bus_path, project, objective, deadline, budget) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error creating goal: {e}");
            std::process::exit(1);
        }
    };

    println!("Goal created: {}", g.id);
    println!("  project:   {}", g.project);
    println!("  objective: {}", g.objective);
    println!("  budget:    ${:.2}", g.budget_usd);
    if let Some(ref d) = g.deadline {
        println!("  deadline:  {d}");
    }
    println!();

    // Resolve project path via resolver chain
    let project_path = match resolver::resolve(project, None, Some(&cfg)) {
        Ok(r) => r.path.to_string_lossy().to_string(),
        Err(_) => {
            eprintln!("Warning: project '{project}' not found, skipping planner");
            eprintln!("Hint: punk-run use {project} /path/to/project");
            return;
        }
    };

    println!("Generating plan...");
    let prompt = goal::build_planner_prompt(&g, std::path::Path::new(&project_path));
    let report = run_goal_planner_with_fallback(&prompt);

    match (
        report.selected_provider.as_deref(),
        report.plan_text.as_deref(),
    ) {
        (Some(provider), Some(text)) => match goal::parse_plan(text, provider) {
            Some(plan) => {
                let step_count = plan.steps.len();
                let est_cost: f64 = plan.steps.iter().map(|s| s.est_cost_usd).sum();
                g.plan = Some(plan);
                g.status = goal::GoalStatus::AwaitingApproval;
                goal::save_goal(&bus_path, &g).ok();

                println!(
                    "Plan generated via {provider}: {} steps, ${:.2} estimated\n",
                    step_count, est_cost
                );
                if approve {
                    match activate_goal(&bus_path, &mut g) {
                        Ok(queued) => {
                            print!("{}", format_goal_approval_report(&g, &queued));
                        }
                        Err(err) => {
                            eprintln!("Auto-approve failed: {err}");
                            eprintln!("Review and approve manually:");
                            eprintln!("  punk-run goal approve {}", g.id);
                            std::process::exit(1);
                        }
                    }
                } else {
                    println!("Review and approve:");
                    println!("  punk-run goal approve {}", g.id);
                }
            }
            None => {
                eprintln!("Failed to parse planner output from {provider}.");
                eprintln!(
                    "Try: punk-run goal approve {} (after manual plan edit)",
                    g.id
                );
            }
        },
        _ => {
            eprintln!("{}", format_goal_planner_failure(&report));
            eprintln!("Try: install claude, codex, or gemini, or add the plan manually.");
        }
    }
}

fn activate_goal(bus_path: &Path, goal: &mut goal::Goal) -> Result<Vec<String>, String> {
    if goal.plan.is_none() {
        return Err("Goal has no plan yet. Run planner first.".to_string());
    }
    if goal.status != goal::GoalStatus::AwaitingApproval
        && goal.status != goal::GoalStatus::Planning
    {
        return Err(format!("Goal status is {:?}, cannot approve.", goal.status));
    }

    let queued = goal::queue_ready_steps(bus_path, goal)
        .map_err(|e| format!("Error queueing initial goal steps: {e}"))?;

    if let Some(ref mut plan) = goal.plan {
        plan.approved_at = Some(punk_orch::chrono::Utc::now());
    }
    if queued.is_empty() && !goal_has_inflight_steps(goal) {
        return Err("No runnable goal steps were queued; refusing activation.".to_string());
    }
    goal.status = goal::GoalStatus::Active;

    goal::save_goal(bus_path, goal).map_err(|e| format!("Error saving goal: {e}"))?;
    Ok(queued)
}

struct GoalPlannerAttempt {
    provider: String,
    error: Option<String>,
}

struct GoalPlannerReport {
    available_providers: Vec<String>,
    selected_provider: Option<String>,
    plan_text: Option<String>,
    attempts: Vec<GoalPlannerAttempt>,
}

fn run_goal_planner_with_fallback(prompt: &str) -> GoalPlannerReport {
    let providers = panel::detect_available_providers();
    let mut attempts = Vec::new();

    for provider in &providers {
        match run_goal_planner_provider(provider, prompt) {
            Ok(plan_text) => {
                attempts.push(GoalPlannerAttempt {
                    provider: provider.clone(),
                    error: None,
                });
                return GoalPlannerReport {
                    available_providers: providers.clone(),
                    selected_provider: Some(provider.clone()),
                    plan_text: Some(plan_text),
                    attempts,
                };
            }
            Err(error) => attempts.push(GoalPlannerAttempt {
                provider: provider.clone(),
                error: Some(error),
            }),
        }
    }

    GoalPlannerReport {
        available_providers: providers,
        selected_provider: None,
        plan_text: None,
        attempts,
    }
}

fn run_goal_planner_provider(provider: &str, prompt: &str) -> Result<String, String> {
    match provider {
        "claude" => {
            let out = std::process::Command::new("claude")
                .args(["-p", prompt, "--output-format", "text", "--model", "sonnet"])
                .env_remove("CLAUDECODE")
                .env_remove("ANTHROPIC_API_KEY")
                .output()
                .map_err(|e| e.to_string())?;
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).into_owned())
            } else {
                Err(format!(
                    "exit {}: {}",
                    out.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&out.stderr).trim()
                ))
            }
        }
        "codex" => {
            let out_file = std::env::temp_dir()
                .join(format!("punk-goal-plan-codex-{}.txt", std::process::id()));
            let out = std::process::Command::new("codex")
                .args(["exec", "--ephemeral", "-p", "fast", "--output-last-message"])
                .arg(&out_file)
                .arg(prompt)
                .output()
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let _ = std::fs::remove_file(&out_file);
                return Err(format!(
                    "exit {}: {stderr}",
                    out.status.code().unwrap_or(-1)
                ));
            }
            let text = std::fs::read_to_string(&out_file).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&out_file);
            Ok(text)
        }
        "gemini" => {
            let out = std::process::Command::new("gemini")
                .args(["-p", prompt, "-o", "text"])
                .output()
                .map_err(|e| e.to_string())?;
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).into_owned())
            } else {
                Err(format!(
                    "exit {}: {}",
                    out.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&out.stderr).trim()
                ))
            }
        }
        _ => Err(format!("unsupported planner provider: {provider}")),
    }
}

fn format_goal_planner_failure(report: &GoalPlannerReport) -> String {
    let mut out = String::from("Planner unavailable.\n");
    if report.available_providers.is_empty() {
        out.push_str("Reason: no supported providers detected.\n");
        return out;
    }
    out.push_str(&format!(
        "Providers tried: {}\n",
        report.available_providers.join(", ")
    ));
    if !report.attempts.is_empty() {
        out.push_str("Attempts:\n");
        for attempt in &report.attempts {
            let status = attempt.error.as_deref().unwrap_or("ok");
            out.push_str(&format!("  - {}: {}\n", attempt.provider, status));
        }
    }
    out
}

fn cmd_goals() {
    let bus_path = bus::bus_dir();
    let goals = goal::list_goals(&bus_path);
    print!("{}", format_goals_report(&goals));
}

fn cmd_approve(goal_id: &str) {
    let bus_path = bus::bus_dir();

    let mut g = match goal::load_goal(&bus_path, goal_id) {
        Some(g) => g,
        None => {
            eprintln!("Goal not found: {goal_id}");
            std::process::exit(1);
        }
    };

    let latest = punk_orch::run::latest_run_triage(&bus_path, &g.project);
    if latest.verdict == punk_orch::run::TriageVerdict::StillAlive {
        eprintln!(
            "{}",
            format_still_alive_guard(&latest, &g.project, "goal approval")
        );
        std::process::exit(1);
    }

    // Show plan
    if let Some(ref plan) = g.plan {
        println!("Plan v{} ({} steps):\n", plan.version, plan.steps.len());
        let mut total_cost = 0.0;
        for step in &plan.steps {
            let deps = if step.depends_on.is_empty() {
                String::new()
            } else {
                format!(
                    " (after {})",
                    step.depends_on
                        .iter()
                        .map(|d| d.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            println!(
                "  {}. [{}] {} ${:.2}{}",
                step.step, step.category, step.prompt, step.est_cost_usd, deps
            );
            total_cost += step.est_cost_usd;
        }
        println!(
            "\n  Total estimated: ${total_cost:.2} / ${:.2} budget",
            g.budget_usd
        );
    }

    let queued = match activate_goal(&bus_path, &mut g) {
        Ok(queued) => queued,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    print!("{}", format_goal_approval_report(&g, &queued));
}

fn format_still_alive_guard(
    triage: &punk_orch::run::RunTriage,
    project: &str,
    action: &str,
) -> String {
    let mut out = format!(
        "Latest run for project '{project}' is still alive; refusing {action}.\nrun: {}",
        triage.run_id
    );
    if let Some(age_s) = triage.age_s {
        out.push_str(&format!(", age={}s", age_s));
    }
    if let Some(heartbeat_age_s) = triage.heartbeat_age_s {
        out.push_str(&format!(", heartbeat={}s", heartbeat_age_s));
    }
    if !triage.stderr_tail.is_empty() {
        out.push_str(&format!(
            "\nstderr: {}",
            triage.stderr_tail.replace('\n', " ")
        ));
    }
    out
}

async fn cmd_diverge(project: &str, spec: &str, timeout: u64) {
    let cfg = load_config_or_exit(&config::config_dir());
    let path = match resolver::resolve(project, None, Some(&cfg)) {
        Ok(r) => r.path,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let strategies = diverge::Strategy::defaults();
    println!(
        "Diverge: dispatching to up to {} providers...\n",
        strategies.len()
    );

    match diverge::run_diverge(&path, spec, &strategies, timeout).await {
        Ok(report) => {
            println!("Run dir:   {}", report.run_dir.display());
            println!("Base ref:  {}\n", report.base_commit);
            println!(
                "{:<6} {:<10} {:<9} {:<6} {:>6} {:>6} FILES",
                "LABEL", "PROVIDER", "STATUS", "EXIT", "+LINES", "-LINES"
            );
            for s in &report.solutions {
                let status = if s.timed_out {
                    "timeout"
                } else if s.exit_code == 0 {
                    "ok"
                } else {
                    "failed"
                };
                println!(
                    "{:<6} {:<10} {:<9} {:<6} {:>6} {:>6} {}",
                    s.label,
                    s.provider,
                    status,
                    s.exit_code,
                    s.lines_added,
                    s.lines_removed,
                    s.files_changed.len()
                );
            }
            println!("\nWorktrees preserved:");
            for s in &report.solutions {
                println!(
                    "  {} [{}] {}",
                    s.label,
                    s.provider,
                    s.worktree_path.display()
                );
            }
            println!("\nInspect with: git -C <worktree> diff HEAD");
        }
        Err(e) => {
            eprintln!("Diverge failed: {e}");
            std::process::exit(1);
        }
    }
}

async fn cmd_panel(question: &str, timeout: u64) {
    println!("Panel: asking all providers...\n");

    let report = panel::ask_all(question, timeout).await;
    if report.available_providers.is_empty() {
        eprintln!("Panel failed: no supported providers detected");
        std::process::exit(1);
    }

    println!("Providers: {}\n", report.available_providers.join(", "));

    for r in &report.responses {
        println!(
            "### {} {} ({} ms, {} chars)",
            r.provider,
            if r.exit_code == 0 { "" } else { "(FAILED)" },
            r.duration_ms,
            r.answer.chars().count()
        );
        if let Some(ref err) = r.error {
            println!("  Error: {err}");
        } else {
            // Show first 500 chars
            let preview: String = r.answer.chars().take(500).collect();
            println!("{preview}");
        }
        println!();
    }

    let summary = panel::summarize(&report);
    println!(
        "Panel: {}/{} providers responded, {} failed, {} timed out",
        summary.responded, summary.available, summary.failed, summary.timed_out
    );
}

fn cmd_ratchet() {
    let bus_path = bus::bus_dir();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let current = ratchet::compute_metrics_window(&bus_path, 0, 7);
    let previous = ratchet::compute_metrics_window(&bus_path, 7, 14);

    println!("Metric Ratchet\n");
    println!("  This week:  {}", ratchet::format_metrics(&current));
    println!("  Last week:  {}", ratchet::format_metrics(&previous));
    println!();

    let mut directives = ratchet::compare(&current, &previous);
    let eval_summary = eval::summarize_task_evals(&cwd, Some(20), None).ok();
    if let Some(summary) = &eval_summary {
        directives.extend(ratchet::eval_directives(summary));
    }
    let skill_eval_summary = eval::summarize_skill_evals(&cwd, Some(20), None, None).ok();
    if let Some(summary) = &skill_eval_summary {
        directives.extend(ratchet::skill_eval_directives(summary));
    }
    let benchmark_summary = benchmark::summarize_benchmarks(&cwd, Some(20), None, None).ok();
    if let Some(summary) = &benchmark_summary {
        directives.extend(ratchet::benchmark_directives(summary));
    }
    let verdict = ratchet::verdict(&directives);
    println!("  Verdict:   {:?}\n", verdict);

    if directives.is_empty() {
        println!("  No significant changes.");
    } else {
        for d in &directives {
            println!("  {}", ratchet::format_directive(d));
        }
    }

    if let Some(summary) = eval_summary {
        println!();
        println!(
            "  Eval window: last {} stored evals, avg score {:.2}, drift {:.2}",
            summary.total, summary.avg_score, summary.avg_drift_penalty
        );
    }
    if let Some(summary) = skill_eval_summary {
        println!(
            "  Skill eval window: {}",
            ratchet::format_skill_eval_window(&summary)
        );
    }
    if let Some(summary) = benchmark_summary {
        println!(
            "  Benchmark window: last {} results, avg score {:.2}, pass/fail/flaky = {}/{}/{}",
            summary.total,
            summary.avg_score,
            summary.pass_count,
            summary.fail_count,
            summary.flaky_count
        );
    }
}

fn cmd_policy_check(project: &str, category: &str, priority: &str) {
    let cfg = load_config_or_exit(&config::config_dir());
    let d = &cfg.policy.defaults;
    let mut model = d.model.clone();
    let mut budget = d.budget_usd;
    let mut timeout = d.timeout_s;

    // Apply matching rules
    for rule in &cfg.policy.rules {
        let matches = rule.match_criteria.iter().all(|(k, v)| match k.as_str() {
            "project" => v == project,
            "category" => v == category,
            "priority" => v == priority,
            _ => false,
        });
        if matches {
            if let Some(m) = rule.set.get("model").and_then(|v| v.as_str()) {
                model = m.to_string();
            }
            if let Some(b) = rule.set.get("budget_usd").and_then(|v| v.as_float()) {
                budget = b;
            }
            if let Some(t) = rule.set.get("timeout_s").and_then(|v| v.as_integer()) {
                timeout = t as u64;
            }
        }
    }

    println!("Policy check (dry run)\n");
    println!("  Input:    project={project}, category={category}, priority={priority}");
    println!("  Resolved: model={model}, budget=${budget:.2}, timeout={timeout}s");
    println!("  Slots:    {}/{}", 0, d.max_slots);

    // Budget pressure
    let bus_path = bus::bus_dir();
    let (pressure, spent) = punk_orch::budget::check_pressure(
        &bus_path,
        cfg.policy.budget.monthly_ceiling_usd,
        cfg.policy.budget.soft_alert_pct,
        cfg.policy.budget.hard_stop_pct,
    );
    println!(
        "  Budget:   ${spent:.2} / ${:.0} ({pressure:?})",
        cfg.policy.budget.monthly_ceiling_usd
    );

    if !punk_orch::budget::priority_allowed(&pressure, priority) {
        println!("\n  BLOCKED: priority {priority} not allowed at {pressure:?} pressure level");
    } else {
        println!("\n  OK: task would be dispatched");
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_queue(
    project: &str,
    prompt: &str,
    agent: Option<&str>,
    category: &str,
    priority: &str,
    timeout: u64,
    budget: Option<f64>,
    worktree: bool,
    after: Option<&str>,
) {
    let bus_path = bus::bus_dir();
    let cfg = load_config_or_exit(&config::config_dir());
    let agent = match resolve_queue_agent(&cfg, agent) {
        Ok(agent) => agent,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    // Resolve project path via resolution chain
    let resolved = match resolver::resolve(project, None, Some(&cfg)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}", format_project_resolution_error(project, &e));
            std::process::exit(1);
        }
    };
    let project_path = resolved.path.to_string_lossy().to_string();

    let task_id = format!(
        "{}-{}",
        project,
        punk_orch::chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );

    let mut task_json = serde_json::json!({
        "project": project,
        "project_path": project_path,
        "model": agent,
        "prompt": prompt,
        "category": category,
        "timeout_seconds": timeout,
        "worktree": worktree,
    });

    if let Some(b) = budget {
        task_json["max_budget_usd"] = serde_json::json!(b);
    }
    if let Some(dep) = after {
        task_json["depends_on"] = serde_json::json!([dep]);
    }

    let queue_dir = bus_path.join(format!("new/{priority}"));
    std::fs::create_dir_all(&queue_dir).ok();
    let task_path = queue_dir.join(format!("{task_id}.json"));

    match serde_json::to_string_pretty(&task_json) {
        Ok(data) => {
            std::fs::write(&task_path, data).unwrap_or_else(|e| {
                eprintln!("Error writing task: {e}");
                std::process::exit(1);
            });
            println!("Queued: {task_id}");
            println!("  project:  {project}");
            println!("  agent:    {agent}");
            println!("  category: {category}");
            println!("  priority: {priority}");
            println!("  timeout:  {timeout}s");
            if let Some(b) = budget {
                println!("  budget:   ${b:.2}");
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

fn resolve_queue_agent(
    cfg: &config::Config,
    explicit_agent: Option<&str>,
) -> Result<String, String> {
    if let Some(agent) = explicit_agent {
        return Ok(agent.to_string());
    }

    preferred_queue_agent(cfg).ok_or_else(|| {
        "No supported agents detected.\n\nHint: install claude, codex, or gemini, or pass --agent <provider-or-alias> explicitly.".to_string()
    })
}

fn preferred_queue_agent(cfg: &config::Config) -> Option<String> {
    let agents = &cfg.agents.agents;

    for preferred in ["claude", "codex", "gemini"] {
        if agents.contains_key(preferred) {
            return Some(preferred.to_string());
        }
    }

    for preferred_provider in ["claude", "codex", "gemini"] {
        let mut matching: Vec<_> = agents
            .iter()
            .filter(|(_, agent)| agent.provider == preferred_provider)
            .map(|(id, _)| id.clone())
            .collect();
        matching.sort();
        if let Some(id) = matching.into_iter().next() {
            return Some(id);
        }
    }

    let mut fallback_ids: Vec<_> = agents.keys().cloned().collect();
    fallback_ids.sort();
    fallback_ids.into_iter().next()
}

fn cmd_receipts(project_filter: Option<&str>, since_days: i64) {
    let bus_path = bus::bus_dir();
    let index = bus_path
        .parent()
        .unwrap_or(&bus_path)
        .join("receipts/index.jsonl");

    let content = match std::fs::read_to_string(&index) {
        Ok(c) => c,
        Err(_) => {
            println!("No receipts found.");
            return;
        }
    };

    let cutoff = (punk_orch::chrono::Utc::now() - punk_orch::chrono::Duration::days(since_days))
        .to_rfc3339();

    println!(
        "{:<40} {:<12} {:<8} {:<9} {:>7} {:>6}",
        "TASK", "PROJECT", "MODEL", "STATUS", "COST", "TIME"
    );

    let mut count = 0u32;
    for line in content.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let ts = v
            .get("created_at")
            .or_else(|| v.get("completed_at"))
            .and_then(|t| t.as_str())
            .unwrap_or("");
        if ts < cutoff.as_str() {
            continue;
        }

        let proj = v.get("project").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(filter) = project_filter {
            if proj != filter {
                continue;
            }
        }

        let task_id = v.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        let model = v.get("model").and_then(|v| v.as_str()).unwrap_or("");
        let status = v.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let cost = v.get("cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let dur = v
            .get("duration_ms")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                v.get("duration_seconds")
                    .and_then(|v| v.as_u64())
                    .map(|s| s * 1000)
            })
            .unwrap_or(0)
            / 1000;

        println!(
            "{:<40} {:<12} {:<8} {:<9} {:>7} {:>5}s",
            truncate(task_id, 40),
            proj,
            model,
            status,
            format_cost(cost),
            dur
        );
        count += 1;
    }
    println!("\n{count} receipts (last {since_days}d)");
}

#[derive(Debug)]
struct AskSnapshot {
    captured_at: String,
    recent_total: usize,
    recent_success: usize,
    running: Vec<String>,
    queued: Vec<String>,
    failed: Vec<String>,
    goals: Vec<String>,
}

async fn cmd_ask(question: &str) {
    let bus_path = bus::bus_dir();
    let state = bus::read_state(&bus_path, 20);
    let goals = goal::list_goals(&bus_path);
    let snapshot = AskSnapshot {
        captured_at: punk_orch::chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string(),
        recent_total: state.done.len(),
        recent_success: state.done.iter().filter(|t| t.status == "success").count(),
        running: state
            .running
            .iter()
            .map(|task| format!("{} ({}, {})", task.id, task.project, task.model))
            .collect(),
        queued: state
            .queued
            .iter()
            .map(|task| format!("{} ({}, {})", task.id, task.project, task.model))
            .collect(),
        failed: state
            .failed
            .iter()
            .map(|task| format!("{} ({}, {})", task.id, task.project, task.model))
            .collect(),
        goals: goals
            .iter()
            .map(|goal| {
                format!(
                    "{} ({:?}, ${:.2}/${:.2})",
                    goal.id, goal.status, goal.spent_usd, goal.budget_usd
                )
            })
            .collect(),
    };
    let prompt = format_ask_prompt(question, &snapshot);
    let report = panel::ask_with_fallback(&prompt, 30).await;

    if let (Some(provider), Some(answer)) = (
        report.selected_provider.as_deref(),
        report.answer.as_deref(),
    ) {
        println!("Provider: {provider}\n");
        println!("{}", answer.trim());
        println!("\n{}", format_ask_provenance(&snapshot));
        return;
    }

    println!(
        "{}",
        format_deterministic_ask_fallback(question, &snapshot, &report)
    );
}

fn format_ask_prompt(question: &str, snapshot: &AskSnapshot) -> String {
    let mut context = format!("Data snapshot ({}):\n", snapshot.captured_at);
    context.push_str(&format!(
        "- Recent: {} tasks ({} ok)\n",
        snapshot.recent_total, snapshot.recent_success
    ));
    if !snapshot.running.is_empty() {
        context.push_str(&format!("- Running: {}\n", snapshot.running.len()));
        for item in &snapshot.running {
            context.push_str(&format!("  - {item}\n"));
        }
    }
    if !snapshot.queued.is_empty() {
        context.push_str(&format!("- Queued: {}\n", snapshot.queued.len()));
        for item in &snapshot.queued {
            context.push_str(&format!("  - {item}\n"));
        }
    }
    if !snapshot.failed.is_empty() {
        context.push_str(&format!(
            "- Failed: {} tasks pending triage\n",
            snapshot.failed.len()
        ));
        for item in &snapshot.failed {
            context.push_str(&format!("  - {item}\n"));
        }
    }
    if !snapshot.goals.is_empty() {
        context.push_str(&format!("- Goals: {}\n", snapshot.goals.len()));
        for item in &snapshot.goals {
            context.push_str(&format!("  - {item}\n"));
        }
    }
    format!(
        "{context}\n\nBased ONLY on the data above, answer: {question}\nRules: cite task/goal IDs, do not invent data outside the snapshot, and say 'unknown' if the snapshot is insufficient."
    )
}

fn format_ask_provenance(snapshot: &AskSnapshot) -> String {
    let mut out = String::from("Provenance:\n");
    out.push_str(&format!(
        "- Snapshot: {} recent / {} ok\n",
        snapshot.recent_total, snapshot.recent_success
    ));
    if !snapshot.running.is_empty() {
        out.push_str(&format!("- Running IDs: {}\n", snapshot.running.join(", ")));
    }
    if !snapshot.queued.is_empty() {
        out.push_str(&format!("- Queued IDs: {}\n", snapshot.queued.join(", ")));
    }
    if !snapshot.failed.is_empty() {
        out.push_str(&format!("- Failed IDs: {}\n", snapshot.failed.join(", ")));
    }
    if !snapshot.goals.is_empty() {
        out.push_str(&format!("- Goal IDs: {}\n", snapshot.goals.join(", ")));
    }
    out
}

fn format_deterministic_ask_fallback(
    question: &str,
    snapshot: &AskSnapshot,
    report: &panel::FallbackAskReport,
) -> String {
    let mut out = String::from("AI answer unavailable.\n");
    out.push_str(&format!("Question: {question}\n"));
    out.push_str("Answer: unknown\n");
    if report.available_providers.is_empty() {
        out.push_str("Reason: no supported providers detected.\n");
    } else {
        out.push_str(&format!(
            "Providers tried: {}\n",
            report.available_providers.join(", ")
        ));
        if !report.attempts.is_empty() {
            out.push_str("Attempt results:\n");
            for attempt in &report.attempts {
                let status = if attempt.timed_out {
                    "timeout".to_string()
                } else if attempt.exit_code == 0 && attempt.answer.trim().is_empty() {
                    "empty answer".to_string()
                } else if attempt.exit_code == 0 {
                    "ok".to_string()
                } else {
                    attempt
                        .error
                        .clone()
                        .unwrap_or_else(|| format!("exit {}", attempt.exit_code))
                };
                out.push_str(&format!("  - {}: {}\n", attempt.provider, status));
            }
        }
    }
    out.push('\n');
    out.push_str(&format_ask_provenance(snapshot));
    out
}

fn cmd_pipeline_list() {
    let bus_path = bus::bus_dir();
    let opps = pipeline::load_pipeline(&bus_path);
    let today = punk_orch::chrono::Utc::now().format("%Y-%m-%d").to_string();
    print!("{}", format_pipeline_report(&opps, &today));
}

fn format_pipeline_report(opps: &[pipeline::Opportunity], today: &str) -> String {
    if opps.is_empty() {
        return "Pipeline empty.\n".to_string();
    }

    let summary = pipeline::summarize(opps, today);
    let ordered = pipeline::ordered_for_review(opps, today);
    let active: Vec<_> = ordered
        .iter()
        .filter(|opp| !pipeline::is_terminal_stage(&opp.stage))
        .collect();
    let closed: Vec<_> = ordered
        .iter()
        .filter(|opp| pipeline::is_terminal_stage(&opp.stage))
        .collect();

    let mut out = format!("Pipeline ({} opportunities)\n", summary.total);
    out.push_str(&format!(
        "Summary: active={} stale={} won={} lost={}\n",
        summary.active, summary.stale, summary.won, summary.lost
    ));

    append_pipeline_group(&mut out, "Active", &active, today);
    append_pipeline_group(&mut out, "Closed", &closed, today);

    out.push_str("\nActions:\n");
    out.push_str("  punk-run pipeline add <project> <contact> <next-step> <due>\n");
    out.push_str("  punk-run pipeline advance <id>\n");
    out.push_str("  punk-run pipeline win <id> | lose <id>\n");
    if summary.stale > 0 {
        out.push_str("  Hint: punk-run pipeline stale\n");
    }
    out
}

fn append_pipeline_group(
    out: &mut String,
    title: &str,
    entries: &[&pipeline::Opportunity],
    today: &str,
) {
    if entries.is_empty() {
        return;
    }

    out.push_str(&format!("\n{} ({})\n", title, entries.len()));
    out.push_str(&format!(
        "  {:<4} {:<12} {:<15} {:<14} {:<20} {:<12} {:>8}\n",
        "ID", "PROJECT", "CONTACT", "STAGE", "NEXT STEP", "DUE", "VALUE"
    ));
    for opp in entries {
        let val = opp.value_usd.map(|v| format!("${v}")).unwrap_or_default();
        let due = if pipeline::is_stale(opp, today) {
            format!("{} !", opp.due)
        } else {
            opp.due.clone()
        };
        out.push_str(&format!(
            "  {:<4} {:<12} {:<15} {:<14} {:<20} {:<12} {:>8}\n",
            opp.id,
            truncate(&opp.project, 12),
            truncate(&opp.contact, 15),
            format!("{:?}", opp.stage).to_lowercase(),
            truncate(&opp.next_step, 20),
            truncate(&due, 12),
            val
        ));
    }
}

fn cmd_goal_status(goal_id: &str) {
    let bus_path = bus::bus_dir();
    let g = match goal::load_goal(&bus_path, goal_id) {
        Some(g) => g,
        None => {
            eprintln!("Goal not found: {goal_id}");
            std::process::exit(1);
        }
    };
    print!("{}", format_goal_status_report(&g));
}

#[derive(Default)]
struct GoalStatusSummary {
    planning: usize,
    awaiting_approval: usize,
    active: usize,
    paused: usize,
    done: usize,
    failed: usize,
}

#[derive(Default)]
struct StepStatusSummary {
    pending: usize,
    queued: usize,
    running: usize,
    done: usize,
    blocked: usize,
    failed: usize,
    skipped: usize,
}

struct GoalApprovalSummary<'a> {
    goal: &'a goal::Goal,
    queued: &'a [String],
}

struct GoalBudgetOutcome<'a> {
    goal_id: &'a str,
    project: &'a str,
    old_budget_usd: f64,
    new_budget_usd: f64,
    spent_usd: f64,
}

struct GoalReplanOutcome<'a> {
    goal_id: &'a str,
    project: &'a str,
    objective: &'a str,
}

struct GoalTransitionOutcome<'a> {
    goal_id: &'a str,
    project: &'a str,
    old_status: goal::GoalStatus,
    new_status: goal::GoalStatus,
    inflight_task_count: usize,
}

fn format_goals_report(goals: &[goal::Goal]) -> String {
    if goals.is_empty() {
        return "No goals.\n".to_string();
    }

    let summary = summarize_goal_statuses(goals);
    let mut out = format!("Goals ({})\n", goals.len());
    out.push_str(&format!(
        "Summary: planning={} awaiting_approval={} active={} paused={} done={} failed={}\n\n",
        summary.planning,
        summary.awaiting_approval,
        summary.active,
        summary.paused,
        summary.done,
        summary.failed
    ));
    out.push_str(&format!(
        "  {:<35} {:<12} {:<17} {:>8} {:>8} OBJECTIVE\n",
        "ID", "PROJECT", "STATUS", "SPENT", "BUDGET"
    ));
    for g in goals {
        out.push_str(&format!(
            "  {:<35} {:<12} {:<17} {:>8} {:>8} {}\n",
            truncate(&g.id, 35),
            g.project,
            format!("{:?}", g.status).to_lowercase(),
            format_cost(g.spent_usd),
            format_cost(g.budget_usd),
            truncate(&g.objective, 40)
        ));
        if let Some(ref plan) = g.plan {
            let done = plan
                .steps
                .iter()
                .filter(|s| s.status == goal::StepStatus::Done)
                .count();
            let total = plan.steps.len();
            out.push_str(&format!(
                "    plan v{}: {done}/{total} steps done\n",
                plan.version
            ));
        } else {
            out.push_str("    no plan yet\n");
        }
    }
    out
}

fn format_goal_status_report(goal: &goal::Goal) -> String {
    let mut out = String::new();
    out.push_str(&format!("Goal: {}\n", goal.id));
    out.push_str(&format!("  project:   {}\n", goal.project));
    out.push_str(&format!("  objective: {}\n", goal.objective));
    out.push_str(&format!(
        "  status:    {}\n",
        format!("{:?}", goal.status).to_lowercase()
    ));
    out.push_str(&format!(
        "  budget:    ${:.2} (spent: ${:.2})\n",
        goal.budget_usd, goal.spent_usd
    ));
    if let Some(ref d) = goal.deadline {
        out.push_str(&format!("  deadline:  {d}\n"));
    }
    out.push('\n');

    if let Some(ref plan) = goal.plan {
        let summary = summarize_step_statuses(plan);
        out.push_str(&format!(
            "Plan v{} ({} steps)\n",
            plan.version,
            plan.steps.len()
        ));
        out.push_str(&format!(
            "Step summary: pending={} queued={} running={} done={} blocked={} failed={} skipped={}\n\n",
            summary.pending,
            summary.queued,
            summary.running,
            summary.done,
            summary.blocked,
            summary.failed,
            summary.skipped
        ));
        for step in &plan.steps {
            let status_icon = match step.status {
                goal::StepStatus::Done => "[x]",
                goal::StepStatus::Queued => "[~]",
                goal::StepStatus::Running => "[>]",
                goal::StepStatus::Blocked => "[!]",
                goal::StepStatus::Failed => "[f]",
                goal::StepStatus::Pending => "[ ]",
                goal::StepStatus::Skipped => "[-]",
            };
            out.push_str(&format!(
                "  {} {}. [{}] {} (${:.2})\n",
                status_icon, step.step, step.category, step.prompt, step.est_cost_usd
            ));
            if let Some(ref tid) = step.task_id {
                out.push_str(&format!("      task: {tid}\n"));
            }
        }
    } else {
        out.push_str("No plan yet.\n");
    }

    if let Some(reason) = goal.status_reason.as_deref() {
        out.push_str(&format!("\nreason:    {reason}\n"));
        if reason == "replan_needed_dead_end" {
            out.push_str("replan needed: blocked or failed steps left no runnable work\n");
        }
    }

    out.push('\n');
    out.push_str("Actions:\n");
    match goal.status {
        goal::GoalStatus::Planning => {
            out.push_str("  waiting for planner output or manual plan edit\n");
        }
        goal::GoalStatus::AwaitingApproval => {
            out.push_str(&format!("  punk-run goal approve {}\n", goal.id));
        }
        goal::GoalStatus::Active => {
            out.push_str(&format!("  punk-run status --project {}\n", goal.project));
            out.push_str(&format!("  punk-run goal pause {}\n", goal.id));
        }
        goal::GoalStatus::Paused => {
            out.push_str(&format!("  punk-run goal resume {}\n", goal.id));
            out.push_str(&format!("  punk-run goal replan {}\n", goal.id));
        }
        goal::GoalStatus::Done => {
            out.push_str("  completed; no further action required\n");
        }
        goal::GoalStatus::Failed => {
            out.push_str(&format!("  punk-run goal replan {}\n", goal.id));
            out.push_str(&format!("  punk-run goal budget {} <usd>\n", goal.id));
        }
    }
    out
}

fn format_goal_approval_report(goal: &goal::Goal, queued: &[String]) -> String {
    let summary = GoalApprovalSummary { goal, queued };
    let mut out = String::new();
    out.push_str(&format!("Approved: {}\n", summary.goal.id));
    out.push_str(&format!("  project: {}\n", summary.goal.project));
    out.push_str(&format!(
        "  status:  {}\n",
        goal_status_label(&summary.goal.status)
    ));
    out.push_str(&format!("  queued:  {} step(s)\n", summary.queued.len()));
    if !summary.queued.is_empty() {
        out.push_str(&format!("  tasks:   {}\n", summary.queued.join(", ")));
    }
    out.push_str(&format!(
        "  budget:  ${:.2} spent ${:.2}\n",
        summary.goal.budget_usd, summary.goal.spent_usd
    ));
    out.push_str("\nActions:\n");
    out.push_str(&format!("  punk-run goal status {}\n", summary.goal.id));
    out.push_str(&format!(
        "  punk-run status --project {}\n",
        summary.goal.project
    ));
    out
}

fn format_goal_budget_outcome(outcome: GoalBudgetOutcome<'_>) -> String {
    format!(
        "Budget updated: {} (project={}) ${:.2} -> ${:.2}, spent ${:.2}\n",
        outcome.goal_id,
        outcome.project,
        outcome.old_budget_usd,
        outcome.new_budget_usd,
        outcome.spent_usd
    )
}

fn format_goal_replan_outcome(outcome: GoalReplanOutcome<'_>) -> String {
    format!(
        "Replan ready: {} (project={})\n  status: planning\n  plan:   cleared\n  next:   punk-run goal create {} \"{}\"\n",
        outcome.goal_id, outcome.project, outcome.project, outcome.objective
    )
}

fn format_goal_transition_outcome(outcome: GoalTransitionOutcome<'_>) -> String {
    let mut out = format!(
        "Goal updated: {} (project={}) {} -> {}\n",
        outcome.goal_id,
        outcome.project,
        goal_status_label(&outcome.old_status),
        goal_status_label(&outcome.new_status)
    );
    if outcome.new_status == goal::GoalStatus::Failed {
        out.push_str(&format!(
            "  inflight tasks signaled: {}\n",
            outcome.inflight_task_count
        ));
    }
    out.push_str("Actions:\n");
    match outcome.new_status {
        goal::GoalStatus::Paused => {
            out.push_str(&format!("  punk-run goal resume {}\n", outcome.goal_id));
            out.push_str(&format!("  punk-run goal status {}\n", outcome.goal_id));
        }
        goal::GoalStatus::Active => {
            out.push_str(&format!("  punk-run goal status {}\n", outcome.goal_id));
            out.push_str(&format!(
                "  punk-run status --project {}\n",
                outcome.project
            ));
        }
        goal::GoalStatus::Failed => {
            out.push_str(&format!("  punk-run goal status {}\n", outcome.goal_id));
            out.push_str(&format!("  punk-run goal replan {}\n", outcome.goal_id));
        }
        _ => {}
    }
    out
}

fn goal_status_label(status: &goal::GoalStatus) -> &'static str {
    match status {
        goal::GoalStatus::Planning => "planning",
        goal::GoalStatus::AwaitingApproval => "awaiting_approval",
        goal::GoalStatus::Active => "active",
        goal::GoalStatus::Paused => "paused",
        goal::GoalStatus::Done => "done",
        goal::GoalStatus::Failed => "failed",
    }
}

fn summarize_goal_statuses(goals: &[goal::Goal]) -> GoalStatusSummary {
    let mut summary = GoalStatusSummary::default();
    for goal in goals {
        match goal.status {
            goal::GoalStatus::Planning => summary.planning += 1,
            goal::GoalStatus::AwaitingApproval => summary.awaiting_approval += 1,
            goal::GoalStatus::Active => summary.active += 1,
            goal::GoalStatus::Paused => summary.paused += 1,
            goal::GoalStatus::Done => summary.done += 1,
            goal::GoalStatus::Failed => summary.failed += 1,
        }
    }
    summary
}

fn summarize_step_statuses(plan: &goal::Plan) -> StepStatusSummary {
    let mut summary = StepStatusSummary::default();
    for step in &plan.steps {
        match step.status {
            goal::StepStatus::Pending => summary.pending += 1,
            goal::StepStatus::Queued => summary.queued += 1,
            goal::StepStatus::Running => summary.running += 1,
            goal::StepStatus::Done => summary.done += 1,
            goal::StepStatus::Blocked => summary.blocked += 1,
            goal::StepStatus::Failed => summary.failed += 1,
            goal::StepStatus::Skipped => summary.skipped += 1,
        }
    }
    summary
}

fn cmd_goal_set_status(goal_id: &str, new_status: goal::GoalStatus) {
    let bus_path = bus::bus_dir();
    let mut g = match goal::load_goal(&bus_path, goal_id) {
        Some(g) => g,
        None => {
            eprintln!("Goal not found: {goal_id}");
            std::process::exit(1);
        }
    };

    if let Err(message) = validate_goal_status_transition(&g, &new_status) {
        eprintln!("{message}");
        std::process::exit(1);
    }

    let old_status = g.status.clone();
    let inflight_task_ids = goal_inflight_task_ids(&g);

    if new_status == goal::GoalStatus::Failed {
        for task_id in &inflight_task_ids {
            if let Err(err) = ops::cancel_task(&bus_path, &task_id) {
                eprintln!("Failed to cancel goal task {task_id}: {err}");
                std::process::exit(1);
            }
        }
        g.completed_at = Some(punk_orch::chrono::Utc::now());
    } else if matches!(
        new_status,
        goal::GoalStatus::Active | goal::GoalStatus::Paused | goal::GoalStatus::Planning
    ) {
        g.completed_at = None;
    }

    g.status = new_status;

    if let Err(e) = goal::save_goal(&bus_path, &g) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    print!(
        "{}",
        format_goal_transition_outcome(GoalTransitionOutcome {
            goal_id,
            project: &g.project,
            old_status,
            new_status: g.status,
            inflight_task_count: inflight_task_ids.len(),
        })
    );
}

fn goal_has_inflight_steps(goal: &goal::Goal) -> bool {
    goal.plan.as_ref().is_some_and(|plan| {
        plan.steps.iter().any(|step| {
            matches!(
                step.status,
                goal::StepStatus::Queued | goal::StepStatus::Running
            )
        })
    })
}

fn goal_inflight_task_ids(goal: &goal::Goal) -> Vec<String> {
    goal.plan
        .as_ref()
        .map(|plan| {
            plan.steps
                .iter()
                .filter(|step| {
                    matches!(
                        step.status,
                        goal::StepStatus::Queued | goal::StepStatus::Running
                    )
                })
                .filter_map(|step| step.task_id.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn validate_goal_status_transition(
    goal: &goal::Goal,
    new_status: &goal::GoalStatus,
) -> Result<(), &'static str> {
    use goal::GoalStatus::*;

    if goal.status == *new_status {
        return Err("Goal is already in that status.");
    }

    match (&goal.status, new_status) {
        (Done, _) | (Failed, Active) | (Failed, Paused) | (Failed, AwaitingApproval) => {
            Err("Cannot transition a terminal goal to that status.")
        }
        (Planning, Paused) | (Planning, Active) => {
            Err("Planning goals must be approved before they can be paused or activated.")
        }
        (AwaitingApproval, Active) => Err("Use goal approve instead of setting active directly."),
        (Active, Active) | (Paused, Paused) => Err("Goal is already in that status."),
        (Active, AwaitingApproval) | (Paused, AwaitingApproval) => {
            Err("Cannot move an existing plan back to awaiting approval.")
        }
        (_, Done) => Err("Goal completion is daemon-owned; do not set done manually."),
        (_, Planning) => Err("Use goal replan to return a goal to planning."),
        (Active, Paused) | (Paused, Active) | (_, Failed) => Ok(()),
        _ => Err("Unsupported goal status transition."),
    }
}

// --- Phase 5: Zero-config commands ---

fn cmd_use(name: &str, path: &str) {
    let abs = expand_path(path);
    if !abs.is_dir() {
        eprintln!("Path does not exist: {}", abs.display());
        std::process::exit(1);
    }
    match resolver::pin_project(name, &abs) {
        Ok(()) => println!("Pinned: {name} -> {}", abs.display()),
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_resolve(name: &str, cli_path: Option<&str>) {
    match resolve_for_cli(name, cli_path, &config::config_dir()) {
        Ok(r) => {
            println!("Resolved: {}", r.id);
            println!("  path:   {}", r.path.display());
            println!("  source: {}", r.source);
            if let Some(ref s) = r.stack {
                println!("  stack:  {s}");
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

fn format_project_resolution_error(name: &str, err: &resolver::ResolveError) -> String {
    match err {
        resolver::ResolveError::Ambiguous { .. } => format!(
            "{err}\n\nHint: punk-run resolve {name} --path /absolute/path/to/project\nHint: punk-run use {name} /path/to/project"
        ),
        _ => format!("{err}\n\nHint: punk-run use {name} /path/to/project"),
    }
}

fn resolve_for_cli(
    name: &str,
    cli_path: Option<&str>,
    config_dir: &Path,
) -> Result<resolver::ResolvedProject, String> {
    let path = cli_path.map(expand_path);
    if path.is_some() {
        return resolver::resolve(name, path.as_deref(), None)
            .map_err(|e| format_project_resolution_error(name, &e));
    }
    let cfg = config::load_or_default(config_dir)
        .map_err(|e| format!("Config error in {}: {e}", config_dir.display()))?;
    resolver::resolve(name, None, Some(&cfg)).map_err(|e| format_project_resolution_error(name, &e))
}

fn cmd_forget(name: &str) {
    match resolver::unpin_project(name) {
        Ok(true) => println!("Unpinned: {name}"),
        Ok(false) => println!("Not pinned: {name}"),
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_projects() {
    maybe_warn_jj_degraded_mode();
    let cfg = load_config_or_exit(&config::config_dir());
    let projects = resolver::list_known(Some(&cfg));

    if projects.is_empty() {
        println!("No projects found.");
        println!("\nHint: punk-run use <name> /path/to/project");
        return;
    }

    println!("Projects ({})\n", projects.len());
    println!("  {:<15} {:<45} {:<10} SOURCE", "ID", "PATH", "STACK");
    for p in &projects {
        println!(
            "  {:<15} {:<45} {:<10} {}",
            p.id,
            truncate(&p.path.to_string_lossy(), 45),
            p.stack.as_deref().unwrap_or("-"),
            p.source
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectBootstrapSummary {
    project: String,
    repo_root: PathBuf,
    bootstrap_file: PathBuf,
    bootstrap_file_created: bool,
    skill_name: String,
    skill_created: bool,
}

fn cmd_init(project: Option<&str>) {
    maybe_warn_jj_degraded_mode();
    let dir = config::config_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("Failed to initialize config dir {}: {e}", dir.display());
        std::process::exit(1);
    }

    let status = config::config_status(&dir);
    let agents = config::detect_agents();
    let discovered = resolver::scan_all_roots();

    let summary = match initialize_config_files(&dir, &status, &agents, &discovered) {
        Ok(summary) => summary,
        Err(e) => {
            eprintln!("Failed to write config files in {}: {e}", dir.display());
            std::process::exit(1);
        }
    };

    if summary.created.is_empty() && summary.notices.is_empty() {
        println!("Config already complete: {}", dir.display());
    } else {
        for f in &summary.created {
            println!("Created: {f}");
        }
        for notice in &summary.notices {
            println!("{notice}");
        }
    }
    println!(
        "\nDetected {} agent(s), {} project(s)",
        agents.agents.len(),
        discovered.len()
    );
    println!("Config:  {}", dir.display());

    if let Some(project) = project {
        let cwd = std::env::current_dir().unwrap_or_else(|e| {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        });
        match bootstrap_current_project(&bus::bus_dir(), project, &cwd) {
            Ok(summary) => {
                println!();
                print!("{}", format_project_bootstrap_summary(&summary));
            }
            Err(e) => {
                eprintln!("Project bootstrap failed: {e}");
                std::process::exit(1);
            }
        }
    }
}

fn bootstrap_current_project(
    bus_path: &Path,
    project: &str,
    cwd: &Path,
) -> Result<ProjectBootstrapSummary, String> {
    let safe_project = sanitize::safe_id(project)?;
    let repo_root = detect_repo_root_or_cwd(cwd);

    resolver::pin_project(&safe_project, &repo_root).map_err(|e| e.to_string())?;

    let (bootstrap_file, bootstrap_file_created) =
        ensure_project_bootstrap_file(&repo_root, &safe_project)?;
    let skill_name = format!("{safe_project}-core");
    let skill_created =
        ensure_project_bootstrap_skill(bus_path, &safe_project, &skill_name, &bootstrap_file)?;

    Ok(ProjectBootstrapSummary {
        project: safe_project,
        repo_root,
        bootstrap_file,
        bootstrap_file_created,
        skill_name,
        skill_created,
    })
}

fn detect_repo_root_or_cwd(cwd: &Path) -> PathBuf {
    detect_git_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf())
}

fn detect_git_repo_root(cwd: &Path) -> Option<PathBuf> {
    let mut current = cwd;
    loop {
        let dot_git = current.join(".git");
        if dot_git.is_dir() || dot_git.is_file() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn ensure_project_bootstrap_file(
    repo_root: &Path,
    project: &str,
) -> Result<(PathBuf, bool), String> {
    let bootstrap_dir = repo_root.join(".punk/bootstrap");
    std::fs::create_dir_all(&bootstrap_dir).map_err(|e| e.to_string())?;

    let path = bootstrap_dir.join(format!("{project}-core.md"));
    if path.exists() {
        return Ok((path, false));
    }

    std::fs::write(&path, default_project_skill_template())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok((path, true))
}

fn ensure_project_bootstrap_skill(
    bus_path: &Path,
    project: &str,
    skill_name: &str,
    bootstrap_file: &Path,
) -> Result<bool, String> {
    let active_skill_path = bus_path
        .parent()
        .unwrap_or(bus_path)
        .join("skills")
        .join(format!("{skill_name}.md"));
    if active_skill_path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(bootstrap_file)
        .map_err(|e| format!("read {}: {e}", bootstrap_file.display()))?;
    let categories = vec![
        "plan".to_string(),
        "planning".to_string(),
        "fix".to_string(),
        "codegen".to_string(),
        "goal".to_string(),
    ];
    let projects = vec![project.to_string()];
    skill::create_skill_with_triggers(
        bus_path,
        skill_name,
        &format!("Core project rules for {project}"),
        &content,
        &projects,
        &categories,
    )?;
    Ok(true)
}

fn default_project_skill_template() -> &'static str {
    "Use existing architecture and naming before introducing new abstractions.\n\nPrefer additive changes over rewrites.\n\nKeep slices bounded:\n- 1-3 files when possible\n- one diff, one purpose\n\nPrefer existing helpers, modules, and interfaces before creating new ones.\n\nFor behavior changes:\n- preserve schemas unless acceptance explicitly changes them\n- no silent broad refactors\n\nFor tests:\n- prefer focused tests near changed behavior\n- no change without verification\n\nFail closed instead of guessing.\n"
}

fn format_project_bootstrap_summary(summary: &ProjectBootstrapSummary) -> String {
    let bootstrap_status = if summary.bootstrap_file_created {
        "created"
    } else {
        "existing"
    };
    let skill_status = if summary.skill_created {
        "created"
    } else {
        "existing"
    };

    format!(
        "Project: {project}\nPinned:   {repo_root}\nBootstrap file ({bootstrap_status}): {bootstrap_file}\nSkill ({skill_status}): {skill_name}\n",
        project = summary.project,
        repo_root = summary.repo_root.display(),
        bootstrap_file = summary.bootstrap_file.display(),
        skill_name = summary.skill_name,
    )
}

fn cmd_vcs_status() {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let mode = detect_vcs_mode(&cwd);
    println!("{}", format_vcs_status(mode));
}

fn cmd_vcs_enable_jj() {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };

    match detect_vcs_mode(&cwd) {
        VcsMode::Jj => {
            println!("jj is already enabled for this repo.");
        }
        VcsMode::GitWithJjAvailableButDisabled => match enable_jj_for_repo(&cwd) {
            Ok(()) => println!("Enabled jj for this repo."),
            Err(e) => {
                eprintln!("Failed to enable jj: {e}");
                std::process::exit(1);
            }
        },
        VcsMode::GitOnly => {
            eprintln!("jj is not installed; cannot enable jj for this repo.");
            std::process::exit(1);
        }
        VcsMode::NoVcs => {
            eprintln!("No Git or jj repo detected in the current directory.");
            std::process::exit(1);
        }
    }
}

fn parse_research_kind(raw: &str) -> Result<research::ResearchKind, String> {
    match raw {
        "architecture" => Ok(research::ResearchKind::Architecture),
        "migration-risk" | "migration_risk" => Ok(research::ResearchKind::MigrationRisk),
        "cleanup-impact" | "cleanup_impact" => Ok(research::ResearchKind::CleanupImpact),
        "skill-improvement" | "skill_improvement" => Ok(research::ResearchKind::SkillImprovement),
        "model-protocol-comparison" | "model_protocol_comparison" => {
            Ok(research::ResearchKind::ModelProtocolComparison)
        }
        _ => Err(format!(
            "unknown research kind: {raw} (expected architecture, migration-risk, cleanup-impact, skill-improvement, or model-protocol-comparison)"
        )),
    }
}

fn parse_research_outcome(raw: &str) -> Result<research::ResearchOutcome, String> {
    match raw {
        "answer" => Ok(research::ResearchOutcome::Answer),
        "candidate-patch" | "candidate_patch" => Ok(research::ResearchOutcome::CandidatePatch),
        "contract-patch" | "contract_patch" => Ok(research::ResearchOutcome::ContractPatch),
        "adr-draft" | "adr_draft" => Ok(research::ResearchOutcome::AdrDraft),
        "risk-memo" | "risk_memo" => Ok(research::ResearchOutcome::RiskMemo),
        "eval-suite-patch" | "eval_suite_patch" => Ok(research::ResearchOutcome::EvalSuitePatch),
        "escalate" => Ok(research::ResearchOutcome::Escalate),
        _ => Err(format!(
            "unknown research outcome: {raw} (expected answer, candidate-patch, contract-patch, adr-draft, risk-memo, eval-suite-patch, or escalate)"
        )),
    }
}

fn parse_research_artifact_kind(raw: &str) -> Result<research::ResearchArtifactKind, String> {
    match raw {
        "note" => Ok(research::ResearchArtifactKind::Note),
        "hypothesis" => Ok(research::ResearchArtifactKind::Hypothesis),
        "comparison" => Ok(research::ResearchArtifactKind::Comparison),
        "critique" => Ok(research::ResearchArtifactKind::Critique),
        "synthesis-input" | "synthesis_input" => Ok(research::ResearchArtifactKind::SynthesisInput),
        _ => Err(format!(
            "unknown research artifact kind: {raw} (expected note, hypothesis, comparison, critique, or synthesis-input)"
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_research_start(
    kind: &str,
    project: &str,
    question: &str,
    goal: &str,
    subject_ref: Option<&str>,
    constraints: &[String],
    success: &[String],
    context_refs: &[String],
    max_rounds: Option<u32>,
    max_worker_slots: Option<u32>,
    max_duration_minutes: Option<u32>,
    max_artifacts: Option<u32>,
    max_cost_usd: Option<f64>,
    output_schema_ref: Option<&str>,
) {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let kind = parse_research_kind(kind).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    let mut budget = research::ResearchBudget::default();
    if let Some(value) = max_rounds {
        budget.max_rounds = value;
    }
    if let Some(value) = max_worker_slots {
        budget.max_worker_slots = value;
    }
    if let Some(value) = max_duration_minutes {
        budget.max_duration_minutes = value;
    }
    if let Some(value) = max_artifacts {
        budget.max_artifacts = value;
    }
    budget.max_cost_usd = max_cost_usd;

    let started = research::start_research(
        &cwd,
        research::StartResearchRequest {
            kind,
            project_id: project.to_string(),
            subject_ref: subject_ref.map(|value| value.to_string()),
            question: question.to_string(),
            goal: goal.to_string(),
            constraints: constraints.to_vec(),
            success_criteria: success.to_vec(),
            budget,
            context_refs: context_refs.to_vec(),
            output_schema_ref: output_schema_ref.map(|value| value.to_string()),
        },
    )
    .unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    println!("Research id: {}", started.record.research_id);
    println!("Status: frozen");
    println!("Root dir: {}", started.root_dir.display());
    println!("Packet: {}", started.record.packet_path);
    println!("Artifacts: {}", started.record.artifacts_dir);
}

fn cmd_research_list() {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let runs = research::summarize_research_runs(&cwd).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });
    if runs.is_empty() {
        println!("No research runs.");
        return;
    }

    println!("Research runs ({})\n", runs.len());
    for run in runs {
        println!(
            "  {:<48} {:<10} {:<22} artifacts={} synthesis={} {}",
            run.research_id,
            format!("{:?}", run.status).to_ascii_lowercase(),
            run.project_id,
            run.artifact_count,
            if run.has_synthesis { "yes" } else { "no" },
            run.created_at.to_rfc3339()
        );
    }
}

fn cmd_research_synthesize(
    research_id: &str,
    outcome: &str,
    title: &str,
    findings: &[String],
    recommendations: &[String],
    evidence_refs: &[String],
    unresolved: &[String],
) {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let outcome = parse_research_outcome(outcome).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    let write = research::synthesize_research(
        &cwd,
        research_id,
        research::SynthesizeResearchRequest {
            outcome,
            title: title.to_string(),
            findings: findings.to_vec(),
            recommendations: recommendations.to_vec(),
            evidence_refs: evidence_refs.to_vec(),
            unresolved_questions: unresolved.to_vec(),
        },
    )
    .unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    println!("Research id: {}", write.record.research_id);
    println!("Status: {:?}", write.record.status);
    println!("Synthesis: {}", write.synthesis_path.display());
}

fn cmd_research_artifact(
    research_id: &str,
    kind: &str,
    title: &str,
    file: &str,
    evidence_refs: &[String],
) {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let kind = parse_research_artifact_kind(kind).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });
    let content = std::fs::read_to_string(file).unwrap_or_else(|e| {
        eprintln!("Error reading {file}: {e}");
        std::process::exit(1);
    });

    let write = research::write_research_artifact(
        &cwd,
        research_id,
        research::WriteResearchArtifactRequest {
            kind,
            title: title.to_string(),
            content,
            evidence_refs: evidence_refs.to_vec(),
        },
    )
    .unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    println!("Research id: {}", write.artifact.research_id);
    println!("Artifact: {}", write.artifact_path.display());
}

fn cmd_research_show(research_id: &str) {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let inspect = research::inspect_research(&cwd, research_id).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    println!("Research id: {}", inspect.record.research_id);
    println!("Status: {:?}", inspect.record.status);
    println!("Kind: {:?}", inspect.record.kind);
    println!("Project: {}", inspect.record.project_id);
    println!("Root dir: {}", inspect.root_dir.display());
    println!("Question: {}", inspect.packet.question.question);
    println!("Goal: {}", inspect.packet.question.goal);
    println!("Artifacts: {}", inspect.artifacts.len());
    for artifact in &inspect.artifacts {
        println!("  - {:?}: {}", artifact.kind, artifact.title);
    }
    match &inspect.synthesis {
        Some(synthesis) => {
            println!("Synthesis outcome: {:?}", synthesis.outcome);
            println!("Synthesis title: {}", synthesis.title);
        }
        None => println!("Synthesis outcome: none"),
    }
}

fn cmd_eval_task(task_id: &str) {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let bus_path = bus::bus_dir();
    let record = eval::evaluate_task(&cwd, &bus_path, task_id).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    println!("Task: {}", record.task_id);
    println!("Project: {}", record.project_id);
    println!(
        "Receipt status: {}",
        format!("{:?}", record.receipt_status).to_ascii_lowercase()
    );
    println!(
        "Gate outcome: {}",
        format!("{:?}", record.gate_outcome).to_ascii_lowercase()
    );
    println!("Overall score: {:.2}", record.overall_score);
    println!(
        "Metrics: contract={:.2} scope={:.2} target={:.2} integrity={:.2} cleanup={:.2} docs={:.2} drift_penalty={:.2}",
        record.metrics.contract_satisfaction,
        record.metrics.scope_discipline,
        record.metrics.target_pass_rate,
        record.metrics.integrity_pass_rate,
        record.metrics.cleanup_completion,
        record.metrics.docs_parity,
        record.metrics.drift_penalty,
    );
    if record.notes.is_empty() {
        println!("Notes: none");
    } else {
        println!("Notes:");
        for note in &record.notes {
            println!("  - {note}");
        }
    }
}

fn cmd_eval_list() {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let records = eval::list_task_evals(&cwd).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });
    if records.is_empty() {
        println!("No task evals.");
        return;
    }

    println!("Task evals ({})\n", records.len());
    for record in records {
        println!(
            "  {:<24} {:<18} score={:.2} gate={} status={} {}",
            record.task_id,
            record.project_id,
            record.overall_score,
            format!("{:?}", record.gate_outcome).to_ascii_lowercase(),
            format!("{:?}", record.receipt_status).to_ascii_lowercase(),
            record.created_at.to_rfc3339(),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_eval_skill(
    name: &str,
    project: &str,
    suite: &str,
    role: Option<&str>,
    baseline_contract_satisfaction: f64,
    candidate_contract_satisfaction: f64,
    baseline_target_pass_rate: f64,
    candidate_target_pass_rate: f64,
    baseline_blocked_run_rate: f64,
    candidate_blocked_run_rate: f64,
    baseline_escalation_rate: f64,
    candidate_escalation_rate: f64,
    baseline_scope_discipline: f64,
    candidate_scope_discipline: f64,
    baseline_integrity_pass_rate: f64,
    candidate_integrity_pass_rate: f64,
    baseline_cleanup_completion: f64,
    candidate_cleanup_completion: f64,
    baseline_docs_parity: f64,
    candidate_docs_parity: f64,
    baseline_drift_penalty: f64,
    candidate_drift_penalty: f64,
    suite_size: usize,
    evidence_refs: &[String],
    notes: &[String],
) {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let bus_path = bus::bus_dir();
    let record = eval::evaluate_skill(
        &cwd,
        &bus_path,
        eval::EvaluateSkillRequest {
            skill_name: name.to_string(),
            project_id: project.to_string(),
            suite_id: suite.to_string(),
            role: role.map(str::to_string),
            baseline: eval::SkillEvalMetricSet {
                primary: eval::SkillEvalPrimaryMetrics {
                    contract_satisfaction: baseline_contract_satisfaction,
                    target_pass_rate: baseline_target_pass_rate,
                    blocked_run_rate: baseline_blocked_run_rate,
                    escalation_rate: baseline_escalation_rate,
                },
                safety: eval::SkillEvalSafetyMetrics {
                    scope_discipline: baseline_scope_discipline,
                    integrity_pass_rate: baseline_integrity_pass_rate,
                    cleanup_completion: baseline_cleanup_completion,
                    docs_parity: baseline_docs_parity,
                    drift_penalty: baseline_drift_penalty,
                },
            },
            candidate: eval::SkillEvalMetricSet {
                primary: eval::SkillEvalPrimaryMetrics {
                    contract_satisfaction: candidate_contract_satisfaction,
                    target_pass_rate: candidate_target_pass_rate,
                    blocked_run_rate: candidate_blocked_run_rate,
                    escalation_rate: candidate_escalation_rate,
                },
                safety: eval::SkillEvalSafetyMetrics {
                    scope_discipline: candidate_scope_discipline,
                    integrity_pass_rate: candidate_integrity_pass_rate,
                    cleanup_completion: candidate_cleanup_completion,
                    docs_parity: candidate_docs_parity,
                    drift_penalty: candidate_drift_penalty,
                },
            },
            suite_size,
            evidence_refs: evidence_refs.to_vec(),
            notes: notes.to_vec(),
        },
    )
    .unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    println!("Skill: {}", record.skill_name);
    println!("Project: {}", record.project_id);
    println!("Suite: {}", record.suite_id);
    if let Some(role) = &record.role {
        println!("Role: {role}");
    }
    println!("Candidate path: {}", record.candidate_path.display());
    println!(
        "Decision: {}",
        format!("{:?}", record.decision).to_ascii_lowercase()
    );
    println!(
        "Primary score: baseline={:.2} candidate={:.2}",
        record.baseline_primary_score, record.candidate_primary_score
    );
    println!(
        "Suite coverage: {} ({})",
        record.suite_size,
        if record.sufficient_suite {
            "sufficient"
        } else {
            "insufficient"
        }
    );
    if record.primary_improvements.is_empty() {
        println!("Primary improvements: none");
    } else {
        println!("Primary improvements:");
        for item in &record.primary_improvements {
            println!("  - {item}");
        }
    }
    if record.primary_regressions.is_empty() {
        println!("Primary regressions: none");
    } else {
        println!("Primary regressions:");
        for item in &record.primary_regressions {
            println!("  - {item}");
        }
    }
    if record.safety_regressions.is_empty() {
        println!("Safety regressions: none");
    } else {
        println!("Safety regressions:");
        for item in &record.safety_regressions {
            println!("  - {item}");
        }
    }
    if record.decision_reasons.is_empty() {
        println!("Decision reasons: none");
    } else {
        println!("Decision reasons:");
        for reason in &record.decision_reasons {
            println!("  - {reason}");
        }
    }
}

fn cmd_eval_skill_list() {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let records = eval::list_skill_evals(&cwd).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });
    if records.is_empty() {
        println!("No skill evals.");
        return;
    }

    println!("Skill evals ({})\n", records.len());
    for record in records {
        println!(
            "  {:<24} {:<16} suite={:<16} decision={} score={:.2}->{:.2} {}",
            record.eval_id,
            record.skill_name,
            record.suite_id,
            format!("{:?}", record.decision).to_ascii_lowercase(),
            record.baseline_primary_score,
            record.candidate_primary_score,
            record.created_at.to_rfc3339(),
        );
    }
}

fn cmd_eval_skill_summary(
    project_filter: Option<&str>,
    skill_filter: Option<&str>,
    limit: Option<usize>,
) {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let summary = eval::summarize_skill_evals(&cwd, limit, project_filter, skill_filter)
        .unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        });

    println!("Skill eval summary");
    if let Some(project) = project_filter {
        println!("Project filter: {project}");
    }
    if let Some(skill) = skill_filter {
        println!("Skill filter: {skill}");
    }
    if let Some(limit) = limit {
        println!("Limit: newest {limit}");
    }
    println!(
        "Totals: total={} promote={} reject={} rollback={}",
        summary.total, summary.promote_count, summary.reject_count, summary.rollback_count
    );
    println!(
        "Average primary score: baseline={:.2} candidate={:.2} delta={:.2}",
        summary.avg_baseline_primary_score,
        summary.avg_candidate_primary_score,
        summary.avg_score_delta,
    );
    println!("Projects:");
    for project in &summary.projects {
        println!(
            "  - {:<18} total={} promote={} reject={} rollback={} avg_candidate_score={:.2}",
            project.project_id,
            project.total,
            project.promote_count,
            project.reject_count,
            project.rollback_count,
            project.avg_candidate_primary_score,
        );
    }
    println!("Skills:");
    for skill in &summary.skills {
        println!(
            "  - {:<24} total={} promote={} reject={} rollback={} avg_delta={:.2}",
            skill.skill_name,
            skill.total,
            skill.promote_count,
            skill.reject_count,
            skill.rollback_count,
            skill.avg_score_delta,
        );
    }
    println!("Weakest skill evals:");
    for record in &summary.weakest {
        println!(
            "  - {:<24} {:<18} decision={} candidate_score={:.2} {}",
            record.skill_name,
            record.project_id,
            format!("{:?}", record.decision).to_ascii_lowercase(),
            record.candidate_primary_score,
            record.created_at.to_rfc3339(),
        );
    }
}

fn cmd_eval_summary(project_filter: Option<&str>, limit: Option<usize>) {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let summary = eval::summarize_task_evals(&cwd, limit, project_filter).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    println!("Eval summary");
    if let Some(project) = project_filter {
        println!("Project filter: {project}");
    }
    if let Some(limit) = limit {
        println!("Limit: newest {limit}");
    }
    println!(
        "Totals: total={} accept={} reject={}",
        summary.total, summary.accept_count, summary.reject_count
    );
    println!("Average score: {:.2}", summary.avg_score);
    println!(
        "Average metrics: contract={:.2} scope={:.2} target={:.2} integrity={:.2} cleanup={:.2} docs={:.2} drift_penalty={:.2}",
        summary.avg_contract_satisfaction,
        summary.avg_scope_discipline,
        summary.avg_target_pass_rate,
        summary.avg_integrity_pass_rate,
        summary.avg_cleanup_completion,
        summary.avg_docs_parity,
        summary.avg_drift_penalty,
    );
    println!("Projects:");
    for project in &summary.projects {
        println!(
            "  - {:<18} total={} accept={} reject={} avg_score={:.2}",
            project.project_id,
            project.total,
            project.accept_count,
            project.reject_count,
            project.avg_score,
        );
    }
    println!("Weakest tasks:");
    for task in &summary.weakest_tasks {
        println!(
            "  - {:<24} {:<18} score={:.2} gate={} {}",
            task.task_id,
            task.project_id,
            task.overall_score,
            format!("{:?}", task.gate_outcome).to_ascii_lowercase(),
            task.created_at.to_rfc3339(),
        );
    }
}

fn parse_benchmark_outcome(raw: &str) -> Result<benchmark::BenchmarkOutcome, String> {
    match raw {
        "pass" => Ok(benchmark::BenchmarkOutcome::Pass),
        "fail" => Ok(benchmark::BenchmarkOutcome::Fail),
        "flaky" => Ok(benchmark::BenchmarkOutcome::Flaky),
        _ => Err(format!(
            "unknown benchmark outcome: {raw} (expected pass, fail, or flaky)"
        )),
    }
}

fn parse_benchmark_metrics(raw: &[String]) -> Result<Vec<benchmark::BenchmarkMetric>, String> {
    let mut metrics = Vec::new();
    for item in raw {
        let Some((name, value)) = item.split_once('=') else {
            return Err(format!(
                "invalid metric format: {item} (expected name=value)"
            ));
        };
        let parsed = value
            .parse::<f64>()
            .map_err(|_| format!("invalid metric value in: {item}"))?;
        metrics.push(benchmark::BenchmarkMetric {
            name: name.to_string(),
            value: parsed,
        });
    }
    Ok(metrics)
}

fn cmd_benchmark_record(
    suite: &str,
    project: &str,
    outcome: &str,
    score: f64,
    subject_ref: Option<&str>,
    metrics: &[String],
    notes: &[String],
) {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let outcome = parse_benchmark_outcome(outcome).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });
    let metrics = parse_benchmark_metrics(metrics).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    let result = benchmark::record_benchmark(
        &cwd,
        benchmark::RecordBenchmarkRequest {
            suite: suite.to_string(),
            project_id: project.to_string(),
            subject_ref: subject_ref.map(|value| value.to_string()),
            outcome,
            score,
            metrics,
            notes: notes.to_vec(),
        },
    )
    .unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    println!("Benchmark id: {}", result.benchmark_id);
    println!("Suite: {}", result.suite);
    println!("Project: {}", result.project_id);
    println!("Outcome: {:?}", result.outcome);
    println!("Score: {:.2}", result.score);
}

fn cmd_benchmark_list() {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let results = benchmark::list_benchmarks(&cwd).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });
    if results.is_empty() {
        println!("No benchmark results.");
        return;
    }

    println!("Benchmark results ({})\n", results.len());
    for result in results {
        println!(
            "  {:<36} {:<20} {:<18} score={:.2} {:?} {}",
            result.benchmark_id,
            result.suite,
            result.project_id,
            result.score,
            result.outcome,
            result.created_at.to_rfc3339(),
        );
    }
}

fn cmd_benchmark_show(benchmark_id: &str) {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let result = benchmark::load_benchmark(&cwd, benchmark_id).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    println!("Benchmark id: {}", result.benchmark_id);
    println!("Suite: {}", result.suite);
    println!("Project: {}", result.project_id);
    println!("Outcome: {:?}", result.outcome);
    println!("Score: {:.2}", result.score);
    if let Some(subject_ref) = &result.subject_ref {
        println!("Subject ref: {subject_ref}");
    }
    if result.metrics.is_empty() {
        println!("Metrics: none");
    } else {
        println!("Metrics:");
        for metric in &result.metrics {
            println!("  - {}={:.3}", metric.name, metric.value);
        }
    }
    if result.notes.is_empty() {
        println!("Notes: none");
    } else {
        println!("Notes:");
        for note in &result.notes {
            println!("  - {note}");
        }
    }
}

fn cmd_benchmark_summary(
    project_filter: Option<&str>,
    suite_filter: Option<&str>,
    limit: Option<usize>,
) {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to resolve current directory: {e}");
            std::process::exit(1);
        }
    };
    let summary = benchmark::summarize_benchmarks(&cwd, limit, project_filter, suite_filter)
        .unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        });

    println!("Benchmark summary");
    if let Some(project) = project_filter {
        println!("Project filter: {project}");
    }
    if let Some(suite) = suite_filter {
        println!("Suite filter: {suite}");
    }
    if let Some(limit) = limit {
        println!("Limit: newest {limit}");
    }
    println!(
        "Totals: total={} pass={} fail={} flaky={}",
        summary.total, summary.pass_count, summary.fail_count, summary.flaky_count
    );
    println!("Average score: {:.2}", summary.avg_score);
    println!("Projects:");
    for project in &summary.projects {
        println!(
            "  - {:<18} total={} pass={} fail={} flaky={} avg_score={:.2}",
            project.project_id,
            project.total,
            project.pass_count,
            project.fail_count,
            project.flaky_count,
            project.avg_score,
        );
    }
    println!("Suites:");
    for suite in &summary.suites {
        println!(
            "  - {:<20} total={} pass={} fail={} flaky={} avg_score={:.2}",
            suite.suite,
            suite.total,
            suite.pass_count,
            suite.fail_count,
            suite.flaky_count,
            suite.avg_score,
        );
    }
    println!("Weakest benchmarks:");
    for result in &summary.weakest {
        println!(
            "  - {:<36} {:<18} {:<18} score={:.2} {:?} {}",
            result.benchmark_id,
            result.project_id,
            result.suite,
            result.score,
            result.outcome,
            result.created_at.to_rfc3339(),
        );
    }
}

fn load_config_or_exit(dir: &Path) -> config::Config {
    match config::load_or_default(dir) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Config error in {}: {e}", dir.display());
            std::process::exit(1);
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct InitSummary {
    created: Vec<String>,
    notices: Vec<String>,
}

fn initialize_config_files(
    dir: &Path,
    status: &config::ConfigStatus,
    agents: &config::AgentsFile,
    discovered: &[resolver::ResolvedProject],
) -> std::io::Result<InitSummary> {
    let mut summary = InitSummary::default();

    if !status.has_projects && !discovered.is_empty() {
        let mut toml = String::from("# Auto-generated by punk-run init\n\n");
        for p in discovered {
            toml.push_str(&format!(
                "[[projects]]\nid = \"{}\"\npath = \"{}\"\nstack = \"{}\"\n\n",
                p.id,
                p.path.display(),
                p.stack.as_deref().unwrap_or("")
            ));
        }
        std::fs::write(dir.join("projects.toml"), toml)?;
        summary
            .created
            .push(format!("projects.toml ({} projects)", discovered.len()));
    }

    if !status.has_agents {
        if agents.agents.is_empty() {
            summary
                .notices
                .push("Skipped agents.toml: no supported agent CLI detected".to_string());
        } else {
            let mut toml = String::from("# Auto-generated by punk-run init\n\n");
            let mut agent_list: Vec<_> = agents.agents.iter().collect();
            agent_list.sort_by_key(|(k, _)| (*k).clone());
            for (id, a) in &agent_list {
                toml.push_str(&format!(
                    "[agents.{}]\nprovider = \"{}\"\nmodel = \"{}\"\nrole = \"{}\"\ninvoke = \"{}\"\nbudget_usd = {:.1}\n\n",
                    id, a.provider, a.model, a.role, a.invoke, a.budget_usd
                ));
            }
            std::fs::write(dir.join("agents.toml"), toml)?;
            summary
                .created
                .push(format!("agents.toml ({} agents)", agent_list.len()));
        }
    }

    if !status.has_policy {
        let toml = "# Auto-generated by punk-run init\n\n\
            [defaults]\n\
            model = \"sonnet\"\n\
            budget_usd = 1.0\n\
            timeout_s = 600\n\
            max_slots = 5\n\n\
            [budget]\n\
            monthly_ceiling_usd = 50.0\n\
            soft_alert_pct = 80\n\
            hard_stop_pct = 90\n";
        std::fs::write(dir.join("policy.toml"), toml)?;
        summary.created.push("policy.toml".to_string());
    }

    Ok(summary)
}

fn expand_path(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(rest)
    } else {
        PathBuf::from(p)
    }
}

fn maybe_warn_jj_degraded_mode() {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => return,
    };
    if should_warn_about_disabled_jj(detect_vcs_mode(&cwd)) {
        eprintln!("{}", format_jj_degraded_mode_warning("."));
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
            "VCS mode: git-only (degraded; run `punk-run vcs enable-jj`)"
        }
        VcsMode::NoVcs => "VCS mode: no VCS detected",
    }
}

fn format_jj_degraded_mode_warning(enable_target: &str) -> String {
    format!(
        "Warning: running in degraded git-only mode; enable jj for fuller punk functionality with `punk-run vcs enable-jj` (cwd: {enable_target})"
    )
}

// --- Utilities ---

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

fn format_cost(usd: f64) -> String {
    if usd < 0.01 {
        "$0".to_string()
    } else {
        format!("${:.2}", usd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use punk_core::vcs::VcsMode;
    use punk_orch::config::{
        Agent, AgentsFile, BudgetPolicy, Config as OrchConfig, ConfigStatus, PolicyDefaults,
        PolicyFile, Project, ProjectsFile,
    };
    use punk_orch::resolver::{ResolveSource, ResolvedProject};
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sample_agents() -> AgentsFile {
        let mut agents = HashMap::new();
        agents.insert(
            "codex".to_string(),
            Agent {
                provider: "codex".to_string(),
                model: "o4-mini".to_string(),
                role: "engineer".to_string(),
                invoke: "cli".to_string(),
                budget_usd: 1.0,
                system_prompt: None,
                skills: vec![],
            },
        );
        AgentsFile { agents }
    }

    fn config_with_agents(agents: AgentsFile) -> OrchConfig {
        OrchConfig {
            projects: ProjectsFile {
                projects: vec![Project {
                    id: "demo".to_string(),
                    path: "/tmp/demo".to_string(),
                    stack: String::new(),
                    active: true,
                    budget_usd: 0.0,
                    checkpoint: String::new(),
                }],
            },
            agents,
            policy: PolicyFile {
                defaults: PolicyDefaults {
                    model: "sonnet".to_string(),
                    budget_usd: 1.0,
                    timeout_s: 600,
                    max_slots: 1,
                },
                budget: BudgetPolicy::default(),
                rules: vec![],
                features: HashMap::new(),
            },
            dir: PathBuf::from("/tmp/config"),
        }
    }

    fn empty_status(dir: &Path) -> ConfigStatus {
        ConfigStatus {
            dir: dir.to_path_buf(),
            has_projects: false,
            has_agents: false,
            has_policy: false,
        }
    }

    fn temp_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{}-{}-{}", prefix, std::process::id(), nanos));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detect_repo_root_or_cwd_prefers_nearest_git_root() {
        let tmp = temp_test_dir("punk-run-init-repo-root");
        let repo_root = tmp.join("interviewcoach");
        let nested = repo_root.join("apps/web");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(repo_root.join(".git")).unwrap();

        let resolved = detect_repo_root_or_cwd(&nested);
        assert_eq!(resolved, repo_root);

        let no_git = tmp.join("plain/subdir");
        fs::create_dir_all(&no_git).unwrap();
        assert_eq!(detect_repo_root_or_cwd(&no_git), no_git);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ensure_project_bootstrap_file_is_idempotent() {
        let tmp = temp_test_dir("punk-run-init-bootstrap-file");

        let (path, created) = ensure_project_bootstrap_file(&tmp, "interviewcoach").unwrap();
        assert!(created);
        assert!(path.exists());
        let first = fs::read_to_string(&path).unwrap();
        assert!(first.contains("Prefer additive changes over rewrites."));

        fs::write(&path, "custom content\n").unwrap();
        let (_, created_again) = ensure_project_bootstrap_file(&tmp, "interviewcoach").unwrap();
        assert!(!created_again);
        assert_eq!(fs::read_to_string(&path).unwrap(), "custom content\n");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ensure_project_bootstrap_skill_is_idempotent() {
        let tmp = temp_test_dir("punk-run-init-skill");
        let bus = tmp.join("bus");
        let bootstrap = tmp.join("interviewcoach-core.md");
        fs::write(&bootstrap, "Prefer focused tests.\n").unwrap();

        let created = ensure_project_bootstrap_skill(
            &bus,
            "interviewcoach",
            "interviewcoach-core",
            &bootstrap,
        )
        .unwrap();
        assert!(created);

        let active = tmp.join("skills/interviewcoach-core.md");
        let content = fs::read_to_string(&active).unwrap();
        assert!(content.contains("project: [\"interviewcoach\"]"));
        assert!(
            content.contains("category: [\"plan\", \"planning\", \"fix\", \"codegen\", \"goal\"]")
        );

        let created_again = ensure_project_bootstrap_skill(
            &bus,
            "interviewcoach",
            "interviewcoach-core",
            &bootstrap,
        )
        .unwrap();
        assert!(!created_again);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn format_project_bootstrap_summary_marks_created_vs_existing() {
        let summary = ProjectBootstrapSummary {
            project: "interviewcoach".to_string(),
            repo_root: PathBuf::from("/tmp/interviewcoach"),
            bootstrap_file: PathBuf::from(
                "/tmp/interviewcoach/.punk/bootstrap/interviewcoach-core.md",
            ),
            bootstrap_file_created: true,
            skill_name: "interviewcoach-core".to_string(),
            skill_created: false,
        };

        let rendered = format_project_bootstrap_summary(&summary);
        assert!(rendered.contains("Project: interviewcoach"));
        assert!(rendered.contains("Bootstrap file (created):"));
        assert!(rendered.contains("Skill (existing): interviewcoach-core"));
    }

    #[test]
    fn resolve_status_project_filter_prefers_explicit_project() {
        let tmp = temp_test_dir("punk-run-status-scope-explicit");
        let cwd = tmp.join("projects/interviewcoach/app");
        fs::create_dir_all(&cwd).unwrap();
        let known = vec![ResolvedProject {
            id: "interviewcoach".to_string(),
            path: tmp.join("projects/interviewcoach"),
            source: ResolveSource::LazyScan,
            stack: Some("rust".to_string()),
        }];

        let resolved = resolve_status_project_filter(Some("signum"), Some(&cwd), &known);
        assert_eq!(resolved.as_deref(), Some("signum"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_status_project_filter_infers_project_from_cwd() {
        let tmp = temp_test_dir("punk-run-status-scope-cwd");
        let project_root = tmp.join("projects/interviewcoach");
        let cwd = project_root.join("apps/web");
        fs::create_dir_all(&cwd).unwrap();
        let known = vec![ResolvedProject {
            id: "interviewcoach".to_string(),
            path: project_root,
            source: ResolveSource::LazyScan,
            stack: Some("typescript".to_string()),
        }];

        let resolved = resolve_status_project_filter(None, Some(&cwd), &known);
        assert_eq!(resolved.as_deref(), Some("interviewcoach"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_status_project_filter_falls_back_to_global_when_cwd_unknown() {
        let tmp = temp_test_dir("punk-run-status-scope-global");
        let cwd = tmp.join("random-project");
        fs::create_dir_all(&cwd).unwrap();
        let known = vec![ResolvedProject {
            id: "interviewcoach".to_string(),
            path: tmp.join("projects/interviewcoach"),
            source: ResolveSource::LazyScan,
            stack: Some("typescript".to_string()),
        }];

        let resolved = resolve_status_project_filter(None, Some(&cwd), &known);
        assert!(resolved.is_none());
        assert_eq!(format_status_scope_label(resolved.as_deref()), "global");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn initialize_config_files_surfaces_write_errors() {
        let tmp = temp_test_dir("punk-run-init-write-error");
        let bad_dir = tmp.join("blocked-config");
        fs::write(&bad_dir, "not a directory").unwrap();

        let err = initialize_config_files(&bad_dir, &empty_status(&bad_dir), &sample_agents(), &[])
            .unwrap_err();
        assert!(
            matches!(
                err.kind(),
                std::io::ErrorKind::NotADirectory
                    | std::io::ErrorKind::Other
                    | std::io::ErrorKind::PermissionDenied
            ),
            "unexpected error kind: {:?}",
            err.kind()
        );
        let _ = fs::remove_file(&bad_dir);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn initialize_config_files_skips_agents_file_when_none_detected() {
        let tmp = temp_test_dir("punk-run-init-no-agents");
        let status = empty_status(&tmp);
        let discovered = vec![ResolvedProject {
            id: "punk".to_string(),
            path: tmp.join("punk-project"),
            source: ResolveSource::CliPath,
            stack: Some("rust".to_string()),
        }];

        let summary = initialize_config_files(
            &tmp,
            &status,
            &AgentsFile {
                agents: HashMap::new(),
            },
            &discovered,
        )
        .unwrap();

        assert!(tmp.join("projects.toml").is_file());
        assert!(tmp.join("policy.toml").is_file());
        assert!(!tmp.join("agents.toml").exists());
        let policy = fs::read_to_string(tmp.join("policy.toml")).unwrap();
        assert!(policy.contains("soft_alert_pct = 80"));
        assert!(policy.contains("hard_stop_pct = 90"));
        assert!(
            summary
                .notices
                .iter()
                .any(|n| n.contains("Skipped agents.toml"))
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_for_cli_path_bypasses_broken_config() {
        let repo = temp_test_dir("punk-run-resolve-cli-path");
        fs::create_dir_all(repo.join(".git")).unwrap();

        let config_dir = temp_test_dir("punk-run-broken-config");
        fs::write(config_dir.join("agents.toml"), "[agents.claude").unwrap();

        let resolved = resolve_for_cli("demo", Some(repo.to_string_lossy().as_ref()), &config_dir)
            .expect("cli path should bypass broken config");
        assert_eq!(resolved.source, ResolveSource::CliPath);
        assert_eq!(resolved.path, fs::canonicalize(&repo).unwrap());

        let _ = fs::remove_dir_all(&config_dir);
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn project_resolution_error_includes_ambiguity_hints() {
        let err = punk_orch::resolver::ResolveError::Ambiguous {
            name: "demo".to_string(),
            candidates: vec![
                punk_orch::resolver::AmbiguousProjectCandidate {
                    path: PathBuf::from("/tmp/a"),
                    sources: vec![ResolveSource::Toml],
                },
                punk_orch::resolver::AmbiguousProjectCandidate {
                    path: PathBuf::from("/tmp/b"),
                    sources: vec![ResolveSource::LazyScan],
                },
            ],
        };

        let rendered = format_project_resolution_error("demo", &err);
        assert!(rendered.contains("project 'demo' is ambiguous"));
        assert!(rendered.contains("punk-run resolve demo --path /absolute/path/to/project"));
        assert!(rendered.contains("punk-run use demo /path/to/project"));
    }

    #[test]
    fn format_triage_report_groups_dead_before_failed_and_shows_hints() {
        let entries = vec![
            ops::TriageEntry {
                task_id: "dead-1".to_string(),
                project: "specpunk".to_string(),
                model: "codex".to_string(),
                source: "dead".to_string(),
                error_excerpt: "stale worktree".to_string(),
            },
            ops::TriageEntry {
                task_id: "failed-1".to_string(),
                project: "specpunk".to_string(),
                model: "claude".to_string(),
                source: "failed".to_string(),
                error_excerpt: "test failure".to_string(),
            },
        ];

        let rendered = format_triage_report(&entries);
        assert!(rendered.contains("Tasks pending triage (2)"));
        assert!(rendered.contains("Sources: dead-letter=1 failed=1"));
        let dead_idx = rendered.find("Dead-letter (1)").unwrap();
        let failed_idx = rendered.find("Failed (1)").unwrap();
        assert!(dead_idx < failed_idx);
        assert!(rendered.contains("Hint: inspect the dead-letter receipt"));
        assert!(rendered.contains("Hint: review the error excerpt"));
        assert!(rendered.contains("punk-run retry <id>"));
        assert!(rendered.contains("punk-run cancel <id>"));
    }

    #[test]
    fn format_triage_report_handles_empty_entries() {
        assert_eq!(format_triage_report(&[]), "No tasks pending triage.\n");
    }

    fn sample_ask_snapshot() -> AskSnapshot {
        AskSnapshot {
            captured_at: "2026-04-02T11:22:33".to_string(),
            recent_total: 3,
            recent_success: 2,
            running: vec!["run-1 (specpunk, codex)".to_string()],
            queued: vec!["queue-1 (signum, claude)".to_string()],
            failed: vec!["fail-1 (specpunk, codex)".to_string()],
            goals: vec!["goal-1 (Active, $1.00/$3.00)".to_string()],
        }
    }

    #[test]
    fn format_ask_provenance_lists_snapshot_ids() {
        let rendered = format_ask_provenance(&sample_ask_snapshot());
        assert!(rendered.contains("Snapshot: 3 recent / 2 ok"));
        assert!(rendered.contains("run-1 (specpunk, codex)"));
        assert!(rendered.contains("queue-1 (signum, claude)"));
        assert!(rendered.contains("fail-1 (specpunk, codex)"));
        assert!(rendered.contains("goal-1 (Active, $1.00/$3.00)"));
    }

    #[test]
    fn deterministic_ask_fallback_mentions_unknown_and_attempts() {
        let snapshot = sample_ask_snapshot();
        let report = panel::FallbackAskReport {
            available_providers: vec!["claude".into(), "codex".into()],
            attempts: vec![
                panel::ProviderResponse {
                    provider: "claude".into(),
                    answer: String::new(),
                    exit_code: 1,
                    error: Some("exit 1".into()),
                    duration_ms: 10,
                    timed_out: false,
                },
                panel::ProviderResponse {
                    provider: "codex".into(),
                    answer: String::new(),
                    exit_code: 1,
                    error: Some("timeout".into()),
                    duration_ms: 20,
                    timed_out: true,
                },
            ],
            selected_provider: None,
            answer: None,
        };

        let rendered = format_deterministic_ask_fallback("what is blocked?", &snapshot, &report);
        assert!(rendered.contains("AI answer unavailable."));
        assert!(rendered.contains("Answer: unknown"));
        assert!(rendered.contains("Providers tried: claude, codex"));
        assert!(rendered.contains("claude: exit 1"));
        assert!(rendered.contains("codex: timeout"));
        assert!(rendered.contains("Provenance:"));
    }

    fn sample_pipeline_opps() -> Vec<pipeline::Opportunity> {
        vec![
            pipeline::Opportunity {
                id: 1,
                project: "alpha".to_string(),
                contact: "Alice".to_string(),
                stage: pipeline::Stage::Lead,
                next_step: "Send intro".to_string(),
                due: "2026-04-01".to_string(),
                value_usd: Some(5000),
                updated_at: punk_orch::chrono::Utc::now(),
            },
            pipeline::Opportunity {
                id: 2,
                project: "beta".to_string(),
                contact: "Bob".to_string(),
                stage: pipeline::Stage::Negotiation,
                next_step: "Review terms".to_string(),
                due: "2026-04-03".to_string(),
                value_usd: Some(9000),
                updated_at: punk_orch::chrono::Utc::now(),
            },
            pipeline::Opportunity {
                id: 3,
                project: "gamma".to_string(),
                contact: "Carol".to_string(),
                stage: pipeline::Stage::Won,
                next_step: "Archive".to_string(),
                due: "2026-04-04".to_string(),
                value_usd: Some(12000),
                updated_at: punk_orch::chrono::Utc::now(),
            },
        ]
    }

    #[test]
    fn format_pipeline_report_groups_active_and_closed_and_flags_stale() {
        let rendered = format_pipeline_report(&sample_pipeline_opps(), "2026-04-02");
        assert!(rendered.contains("Pipeline (3 opportunities)"));
        assert!(rendered.contains("Summary: active=2 stale=1 won=1 lost=0"));
        let active_idx = rendered.find("Active (2)").unwrap();
        let closed_idx = rendered.find("Closed (1)").unwrap();
        assert!(active_idx < closed_idx);
        assert!(rendered.contains("2026-04-01 !"));
        assert!(rendered.contains("Hint: punk-run pipeline stale"));
        assert!(rendered.contains("punk-run pipeline advance <id>"));
    }

    #[test]
    fn format_pipeline_report_handles_empty_pipeline() {
        assert_eq!(
            format_pipeline_report(&[], "2026-04-02"),
            "Pipeline empty.\n"
        );
    }

    #[test]
    fn format_retry_outcome_includes_project_model_and_source() {
        let rendered = format_retry_outcome(&ops::RetryOutcome {
            task_id: "task-1".into(),
            project: "specpunk".into(),
            model: "codex".into(),
            source: "failed".into(),
            destination: "new/p1".into(),
        });
        assert_eq!(
            rendered,
            "Requeued: task-1 (project=specpunk, model=codex) from failed -> new/p1"
        );
    }

    #[test]
    fn format_cancel_outcome_distinguishes_queued_and_running() {
        let queued = format_cancel_outcome(&ops::CancelOutcome::Queued {
            task_id: "task-2".into(),
            queue_lane: "new/p2".into(),
        });
        assert_eq!(queued, "Cancelled queued task: task-2 (new/p2)");

        let running = format_cancel_outcome(&ops::CancelOutcome::Running {
            task_id: "task-3".into(),
            signal_path: "/tmp/bus/.cancel/task-3".into(),
        });
        assert!(running.contains("Cancellation signaled for running task: task-3"));
        assert!(running.contains("/tmp/bus/.cancel/task-3"));
    }

    #[test]
    fn resolve_queue_agent_prefers_detected_provider_order() {
        let mut agents = HashMap::new();
        agents.insert(
            "gemini".to_string(),
            Agent {
                provider: "gemini".to_string(),
                model: "gemini-2.5-flash".to_string(),
                role: "engineer".to_string(),
                invoke: "cli".to_string(),
                budget_usd: 1.0,
                system_prompt: None,
                skills: vec![],
            },
        );
        agents.insert(
            "codex".to_string(),
            Agent {
                provider: "codex".to_string(),
                model: "o4-mini".to_string(),
                role: "engineer".to_string(),
                invoke: "cli".to_string(),
                budget_usd: 1.0,
                system_prompt: None,
                skills: vec![],
            },
        );
        let cfg = config_with_agents(AgentsFile { agents });

        assert_eq!(resolve_queue_agent(&cfg, None).unwrap(), "codex");
    }

    #[test]
    fn resolve_queue_agent_preserves_explicit_override() {
        let cfg = config_with_agents(sample_agents());
        assert_eq!(
            resolve_queue_agent(&cfg, Some("gemini-custom")).unwrap(),
            "gemini-custom"
        );
    }

    #[test]
    fn resolve_queue_agent_fails_when_no_agents_and_no_override() {
        let cfg = config_with_agents(AgentsFile {
            agents: HashMap::new(),
        });
        let err = resolve_queue_agent(&cfg, None).unwrap_err();
        assert!(err.contains("No supported agents detected"));
        assert!(err.contains("--agent"));
    }

    #[test]
    fn jj_degraded_mode_warning_mentions_degraded_mode_and_enable_action() {
        let warning = format_jj_degraded_mode_warning(".");
        assert!(warning.contains("degraded git-only mode"));
        assert!(warning.contains("punk-run vcs enable-jj"));
        assert!(warning.contains("fuller punk functionality"));
    }

    #[test]
    fn only_git_with_jj_disabled_emits_warning_message() {
        assert!(should_warn_about_disabled_jj(
            VcsMode::GitWithJjAvailableButDisabled
        ));
        assert!(!should_warn_about_disabled_jj(VcsMode::GitOnly));
        assert!(!should_warn_about_disabled_jj(VcsMode::Jj));
        assert!(!should_warn_about_disabled_jj(VcsMode::NoVcs));
    }

    #[test]
    fn vcs_status_marks_degraded_git_mode() {
        assert_eq!(
            format_vcs_status(VcsMode::GitWithJjAvailableButDisabled),
            "VCS mode: git-only (degraded; run `punk-run vcs enable-jj`)"
        );
        assert_eq!(format_vcs_status(VcsMode::Jj), "VCS mode: jj");
    }
}
#[cfg(test)]
mod guard_tests {
    use super::*;

    #[test]
    fn still_alive_guard_message_mentions_project_and_run() {
        let triage = punk_orch::run::RunTriage {
            run_id: "run_123".to_string(),
            status: Some(punk_orch::run::RunStatus::Running),
            age_s: Some(12),
            heartbeat_age_s: Some(4),
            has_receipt: false,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            verdict: punk_orch::run::TriageVerdict::StillAlive,
        };

        let msg = format_still_alive_guard(&triage, "specpunk", "goal planning");
        assert!(msg.contains("specpunk"));
        assert!(msg.contains("run_123"));
        assert!(msg.contains("still alive"));
    }
}

#[cfg(test)]
mod cli_goal_tests {
    use super::*;
    use punk_orch::chrono::Utc;
    use std::fs;

    fn sample_goal(status: goal::GoalStatus, step_statuses: &[goal::StepStatus]) -> goal::Goal {
        goal::Goal {
            id: "goal-1".into(),
            project: "specpunk".into(),
            objective: "ship".into(),
            deadline: None,
            budget_usd: 5.0,
            spent_usd: 1.0,
            status,
            status_reason: None,
            plan: Some(goal::Plan {
                version: 1,
                created_by: "test".into(),
                approved_at: None,
                steps: step_statuses
                    .iter()
                    .enumerate()
                    .map(|(idx, status)| goal::Step {
                        step: idx as u32 + 1,
                        category: "fix".into(),
                        prompt: format!("step {}", idx + 1),
                        agent: "claude-sonnet".into(),
                        est_cost_usd: 0.5,
                        depends_on: vec![],
                        status: *status,
                        task_id: Some(format!("task-{}", idx + 1)),
                        sub_tasks: vec![],
                    })
                    .collect(),
            }),
            created_at: Utc::now(),
            completed_at: None,
        }
    }

    #[test]
    fn validate_goal_status_transition_blocks_invalid_moves() {
        let planning = sample_goal(goal::GoalStatus::Planning, &[goal::StepStatus::Pending]);
        assert!(validate_goal_status_transition(&planning, &goal::GoalStatus::Paused).is_err());
        assert!(validate_goal_status_transition(&planning, &goal::GoalStatus::Active).is_err());

        let awaiting = sample_goal(
            goal::GoalStatus::AwaitingApproval,
            &[goal::StepStatus::Pending],
        );
        assert!(validate_goal_status_transition(&awaiting, &goal::GoalStatus::Active).is_err());

        let done = sample_goal(goal::GoalStatus::Done, &[goal::StepStatus::Done]);
        assert!(validate_goal_status_transition(&done, &goal::GoalStatus::Paused).is_err());
    }

    #[test]
    fn validate_goal_status_transition_allows_pause_resume_cancel() {
        let active = sample_goal(goal::GoalStatus::Active, &[goal::StepStatus::Running]);
        assert!(validate_goal_status_transition(&active, &goal::GoalStatus::Paused).is_ok());
        assert!(validate_goal_status_transition(&active, &goal::GoalStatus::Failed).is_ok());

        let paused = sample_goal(goal::GoalStatus::Paused, &[goal::StepStatus::Queued]);
        assert!(validate_goal_status_transition(&paused, &goal::GoalStatus::Active).is_ok());
        assert!(validate_goal_status_transition(&paused, &goal::GoalStatus::Failed).is_ok());
    }

    #[test]
    fn goal_has_inflight_steps_detects_queued_and_running() {
        let goal = sample_goal(
            goal::GoalStatus::Active,
            &[
                goal::StepStatus::Done,
                goal::StepStatus::Queued,
                goal::StepStatus::Running,
            ],
        );
        assert!(goal_has_inflight_steps(&goal));
        assert_eq!(
            goal_inflight_task_ids(&goal),
            vec!["task-2".to_string(), "task-3".to_string()]
        );

        let settled = sample_goal(
            goal::GoalStatus::Failed,
            &[
                goal::StepStatus::Done,
                goal::StepStatus::Blocked,
                goal::StepStatus::Failed,
            ],
        );
        assert!(!goal_has_inflight_steps(&settled));
    }

    #[test]
    fn format_goals_report_includes_status_summary() {
        let mut paused = sample_goal(goal::GoalStatus::Paused, &[goal::StepStatus::Queued]);
        paused.id = "goal-2".into();
        paused.project = "signum".into();

        let rendered = format_goals_report(&[
            sample_goal(goal::GoalStatus::Active, &[goal::StepStatus::Running]),
            paused,
        ]);
        assert!(rendered.contains("Goals (2)"));
        assert!(rendered.contains("active=1"));
        assert!(rendered.contains("paused=1"));
        assert!(rendered.contains("plan v1: 0/1 steps done"));
    }

    #[test]
    fn format_goal_status_report_shows_step_summary_and_actions() {
        let goal = sample_goal(
            goal::GoalStatus::AwaitingApproval,
            &[
                goal::StepStatus::Done,
                goal::StepStatus::Queued,
                goal::StepStatus::Blocked,
            ],
        );
        let rendered = format_goal_status_report(&goal);
        assert!(rendered.contains("Goal: goal-1"));
        assert!(rendered.contains("status:    awaitingapproval"));
        assert!(rendered.contains(
            "Step summary: pending=0 queued=1 running=0 done=1 blocked=1 failed=0 skipped=0"
        ));
        assert!(rendered.contains("punk-run goal approve goal-1"));
    }

    #[test]
    fn format_goal_status_report_shows_replan_needed_reason() {
        let mut goal = sample_goal(
            goal::GoalStatus::Failed,
            &[goal::StepStatus::Failed, goal::StepStatus::Blocked],
        );
        goal.status_reason = Some("replan_needed_dead_end".into());

        let rendered = format_goal_status_report(&goal);
        assert!(rendered.contains("reason:    replan_needed_dead_end"));
        assert!(rendered.contains("replan needed: blocked or failed steps left no runnable work"));
        assert!(rendered.contains("punk-run goal replan goal-1"));
    }

    #[test]
    fn format_goal_attention_line_reports_dead_end_goal_count() {
        let root = std::env::temp_dir().join(format!(
            "punk-run-goal-attention-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let bus = root.join("bus");
        fs::create_dir_all(&bus).unwrap();

        goal::save_goal(
            &bus,
            &goal::Goal {
                id: "goal-dead-end".into(),
                project: "specpunk".into(),
                objective: "recover".into(),
                deadline: None,
                budget_usd: 5.0,
                spent_usd: 1.0,
                status: goal::GoalStatus::Failed,
                status_reason: Some("replan_needed_dead_end".into()),
                plan: None,
                created_at: Utc::now(),
                completed_at: None,
            },
        )
        .unwrap();

        let rendered = goal::format_goal_attention_line(&bus).unwrap();
        assert_eq!(rendered, "Goal attention: replan dead-end goals (1)");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn format_goal_approval_report_lists_queued_tasks_and_actions() {
        let goal = sample_goal(goal::GoalStatus::Active, &[goal::StepStatus::Queued]);
        let rendered = format_goal_approval_report(&goal, &["task-1".into(), "task-2".into()]);
        assert!(rendered.contains("Approved: goal-1"));
        assert!(rendered.contains("queued:  2 step(s)"));
        assert!(rendered.contains("tasks:   task-1, task-2"));
        assert!(rendered.contains("punk-run goal status goal-1"));
        assert!(rendered.contains("punk-run status --project specpunk"));
    }

    #[test]
    fn activate_goal_queues_steps_and_marks_goal_active() {
        let root = std::env::temp_dir().join(format!(
            "punk-run-goal-activate-{}",
            punk_orch::chrono::Utc::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
        ));
        let bus = root.join("bus");
        std::fs::create_dir_all(&bus).unwrap();
        std::fs::create_dir_all(bus.join("new/p1")).unwrap();

        let mut goal = sample_goal(
            goal::GoalStatus::AwaitingApproval,
            &[goal::StepStatus::Pending],
        );
        goal::save_goal(&bus, &goal).unwrap();

        let queued = activate_goal(&bus, &mut goal).unwrap();
        assert_eq!(goal.status, goal::GoalStatus::Active);
        assert_eq!(queued, vec!["goal-1-step1".to_string()]);
        assert!(goal.plan.as_ref().unwrap().approved_at.is_some());
        assert!(bus.join("new/p1/goal-1-step1.json").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn format_goal_transition_outcome_for_cancel_mentions_replan() {
        let rendered = format_goal_transition_outcome(GoalTransitionOutcome {
            goal_id: "goal-1",
            project: "specpunk",
            old_status: goal::GoalStatus::Active,
            new_status: goal::GoalStatus::Failed,
            inflight_task_count: 2,
        });
        assert!(rendered.contains("Goal updated: goal-1 (project=specpunk) active -> failed"));
        assert!(rendered.contains("inflight tasks signaled: 2"));
        assert!(rendered.contains("punk-run goal replan goal-1"));
    }

    #[test]
    fn format_goal_budget_and_replan_outcomes_are_actionable() {
        let budget = format_goal_budget_outcome(GoalBudgetOutcome {
            goal_id: "goal-1",
            project: "specpunk",
            old_budget_usd: 5.0,
            new_budget_usd: 7.5,
            spent_usd: 1.0,
        });
        assert!(budget.contains("Budget updated: goal-1 (project=specpunk) $5.00 -> $7.50"));

        let replan = format_goal_replan_outcome(GoalReplanOutcome {
            goal_id: "goal-1",
            project: "specpunk",
            objective: "ship checkpoint",
        });
        assert!(replan.contains("Replan ready: goal-1 (project=specpunk)"));
        assert!(replan.contains("status: planning"));
        assert!(replan.contains("punk-run goal create specpunk \"ship checkpoint\""));
    }

    #[test]
    fn goal_planner_failure_mentions_attempts_and_providers() {
        let rendered = format_goal_planner_failure(&GoalPlannerReport {
            available_providers: vec!["claude".into(), "codex".into()],
            selected_provider: None,
            plan_text: None,
            attempts: vec![
                GoalPlannerAttempt {
                    provider: "claude".into(),
                    error: Some("exit 1: boom".into()),
                },
                GoalPlannerAttempt {
                    provider: "codex".into(),
                    error: Some("exit 1: nope".into()),
                },
            ],
        });
        assert!(rendered.contains("Planner unavailable."));
        assert!(rendered.contains("Providers tried: claude, codex"));
        assert!(rendered.contains("- claude: exit 1: boom"));
        assert!(rendered.contains("- codex: exit 1: nope"));
    }

    #[test]
    fn goal_planner_failure_handles_no_detected_providers() {
        let rendered = format_goal_planner_failure(&GoalPlannerReport {
            available_providers: vec![],
            selected_provider: None,
            plan_text: None,
            attempts: vec![],
        });
        assert!(rendered.contains("Planner unavailable."));
        assert!(rendered.contains("no supported providers detected"));
    }
}
