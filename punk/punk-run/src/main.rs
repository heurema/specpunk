use clap::{Parser, Subcommand};
use punk_orch::{bus, config, daemon, diverge, doctor, goal, graph, morning, ops, panel, pipeline, ratchet, recall, skill};

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
        #[arg(long, default_value = "claude")]
        agent: String,
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
        Command::Daemon { shadow, slots, background } => {
            if background {
                // Fork to background
                let exe = std::env::current_exe().unwrap();
                let mut cmd = std::process::Command::new(exe);
                cmd.args(["daemon"]);
                if shadow { cmd.arg("--shadow"); }
                if slots != 5 { cmd.args(["--slots", &slots.to_string()]); }
                cmd.stdin(std::process::Stdio::null());
                cmd.stdout(std::process::Stdio::null());
                cmd.stderr(std::fs::File::create(
                    bus::bus_dir().parent().unwrap_or(&bus::bus_dir()).join("daemon.log")
                ).map(std::process::Stdio::from).unwrap_or(std::process::Stdio::null()));
                match cmd.spawn() {
                    Ok(child) => println!("Daemon started (PID {})", child.id()),
                    Err(e) => { eprintln!("Failed to start daemon: {e}"); std::process::exit(1); }
                }
                return Ok(());
            }
            // Wire policy.toml max_slots if CLI didn't override
            let effective_slots = if slots != 5 {
                slots
            } else if let Ok(cfg) = config::load(&config::config_dir()) {
                cfg.policy.defaults.max_slots
            } else {
                slots
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
                Ok(()) => println!("Requeued: {task_id}"),
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Cancel { task_id } => {
            let bus_path = bus::bus_dir();
            match ops::cancel_task(&bus_path, &task_id) {
                Ok(()) => println!("Cancelled: {task_id}"),
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Doctor => {
            let bus_path = bus::bus_dir();
            let config_dir = config::config_dir();
            let report = doctor::check_all(&bus_path, &config_dir);
            print!("{}", report.display());
        }
        Command::PolicyCheck { project, category, priority } => {
            cmd_policy_check(&project, &category, &priority);
        }
        Command::Queue {
            project, prompt, agent, category, priority, timeout, budget, worktree, after,
        } => {
            cmd_queue(&project, &prompt, &agent, &category, &priority, timeout, budget, worktree, after.as_deref());
        }
        Command::Receipts { project, since } => {
            cmd_receipts(project.as_deref(), since);
        }
        Command::Ask { question } => cmd_ask(&question),
        Command::Pipeline { action } => match action {
            PipelineAction::List => cmd_pipeline_list(),
            PipelineAction::Add { project, contact, next_step, due, value } => {
                let bus_path = bus::bus_dir();
                match pipeline::add(&bus_path, &project, &contact, &next_step, &due, value) {
                    Ok(opp) => println!("Added: #{} {} ({}) -> {}", opp.id, opp.contact, opp.project, opp.next_step),
                    Err(e) => { eprintln!("Error: {e}"); std::process::exit(1); }
                }
            }
            PipelineAction::Advance { id } => {
                let bus_path = bus::bus_dir();
                match pipeline::advance(&bus_path, id) {
                    Ok(opp) => println!("#{}: -> {:?}", opp.id, opp.stage),
                    Err(e) => { eprintln!("Error: {e}"); std::process::exit(1); }
                }
            }
            PipelineAction::Win { id } => {
                let bus_path = bus::bus_dir();
                match pipeline::set_stage(&bus_path, id, pipeline::Stage::Won) {
                    Ok(opp) => println!("#{}: WON", opp.id),
                    Err(e) => { eprintln!("Error: {e}"); std::process::exit(1); }
                }
            }
            PipelineAction::Stale => {
                let bus_path = bus::bus_dir();
                let opps = pipeline::load_pipeline(&bus_path);
                let today = punk_orch::chrono::Utc::now().format("%Y-%m-%d").to_string();
                let stale: Vec<_> = opps.iter().filter(|o| {
                    o.due < today && o.stage != pipeline::Stage::Won && o.stage != pipeline::Stage::Lost
                }).collect();
                if stale.is_empty() {
                    println!("No stale opportunities.");
                } else {
                    println!("Stale opportunities ({}):\n", stale.len());
                    for o in &stale {
                        println!("  #{} {} ({}) — due {} — {:?}", o.id, o.contact, o.project, o.due, o.stage);
                    }
                }
            }
            PipelineAction::Lose { id } => {
                let bus_path = bus::bus_dir();
                match pipeline::set_stage(&bus_path, id, pipeline::Stage::Lost) {
                    Ok(opp) => println!("#{}: LOST", opp.id),
                    Err(e) => { eprintln!("Error: {e}"); std::process::exit(1); }
                }
            }
        },
        Command::Diverge { project, spec, timeout } => {
            cmd_diverge(&project, &spec, timeout).await;
        }
        Command::Panel { question, timeout } => {
            cmd_panel(&question, timeout).await;
        }
        Command::Skill { action } => match action {
            SkillAction::List => {
                let bus_path = bus::bus_dir();
                let skills = skill::list_skills(&bus_path);
                if skills.is_empty() {
                    println!("No skills.");
                } else {
                    println!("Skills ({})\n", skills.len());
                    for s in &skills {
                        println!("  {:<20} {}", s.name, s.description);
                    }
                }
            }
            SkillAction::Create { name, description, file } => {
                let bus_path = bus::bus_dir();
                let content = std::fs::read_to_string(&file).unwrap_or_else(|e| {
                    eprintln!("Error reading {file}: {e}");
                    std::process::exit(1);
                });
                match skill::create_skill(&bus_path, &name, &description, &content) {
                    Ok(path) => println!("Created: {}", path.display()),
                    Err(e) => { eprintln!("Error: {e}"); std::process::exit(1); }
                }
            }
        },
        Command::Recall { query, project, limit } => {
            let bus_path = bus::bus_dir();
            let events = recall::recall(&bus_path, &query, project.as_deref(), limit);
            if events.is_empty() {
                println!("No relevant knowledge found for: {query}");
            } else {
                print!("{}", recall::format_recall(&events));
            }
        }
        Command::Remember { project, context, why, kind } => {
            let bus_path = bus::bus_dir();
            let event_kind = match kind.as_str() {
                "invariant" => recall::EventKind::Invariant,
                "failure" => recall::EventKind::Failure,
                "lesson" => recall::EventKind::Lesson,
                _ => recall::EventKind::Lesson,
            };
            match recall::add_manual(&bus_path, &project, event_kind, &context, &why) {
                Ok(()) => println!("Remembered: [{kind}] {context}"),
                Err(e) => { eprintln!("Error: {e}"); std::process::exit(1); }
            }
        }
        Command::Ratchet => cmd_ratchet(),
        Command::Graph { chart_type, since } => {
            let bus_path = bus::bus_dir();
            match chart_type.as_str() {
                "cost" => print!("{}", graph::cost_chart(&bus_path, since)),
                "project" => print!("{}", graph::project_chart(&bus_path, since)),
                _ => eprintln!("Unknown chart type: {chart_type}. Available: cost, project"),
            }
        }
        Command::Goal { action } => match action {
            GoalAction::Create {
                project,
                objective,
                budget,
                deadline,
            } => cmd_goal(&project, &objective, budget, deadline.as_deref()),
            GoalAction::List => cmd_goals(),
            GoalAction::Status { goal_id } => cmd_goal_status(&goal_id),
            GoalAction::Approve { goal_id } => cmd_approve(&goal_id),
            GoalAction::Pause { goal_id } => cmd_goal_set_status(&goal_id, goal::GoalStatus::Paused),
            GoalAction::Resume { goal_id } => cmd_goal_set_status(&goal_id, goal::GoalStatus::Active),
            GoalAction::Cancel { goal_id } => cmd_goal_set_status(&goal_id, goal::GoalStatus::Failed),
            GoalAction::Budget { goal_id, usd } => {
                let bus_path = bus::bus_dir();
                let mut g = match goal::load_goal(&bus_path, &goal_id) {
                    Some(g) => g,
                    None => { eprintln!("Goal not found: {goal_id}"); std::process::exit(1); }
                };
                g.budget_usd = usd;
                goal::save_goal(&bus_path, &g).ok();
                println!("{goal_id}: budget -> ${usd:.2}");
            }
            GoalAction::Replan { goal_id } => {
                let bus_path = bus::bus_dir();
                let mut g = match goal::load_goal(&bus_path, &goal_id) {
                    Some(g) => g,
                    None => { eprintln!("Goal not found: {goal_id}"); std::process::exit(1); }
                };
                g.plan = None;
                g.status = goal::GoalStatus::Planning;
                goal::save_goal(&bus_path, &g).ok();
                println!("{goal_id}: plan cleared, status -> planning");
                println!("Re-run: punk-run goal create {} \"{}\"", g.project, g.objective);
            }
        },
    }

    Ok(())
}

fn cmd_status(recent_limit: usize, project_filter: Option<&str>) {
    let bus_path = bus::bus_dir();
    let mut state = bus::read_state(&bus_path, recent_limit);

    // Apply project filter
    if let Some(proj) = project_filter {
        state.queued.retain(|t| t.project == proj);
        state.running.retain(|t| t.project == proj);
        state.done.retain(|t| t.project == proj);
        state.failed.retain(|t| t.project == proj);
    }

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
}

fn cmd_config() {
    let dir = config::config_dir();
    println!("Config dir: {}\n", dir.display());

    match config::load(&dir) {
        Ok(cfg) => {
            let active: Vec<_> = cfg.projects.projects.iter().filter(|p| p.active).collect();
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

            let mut agents: Vec<_> = cfg.agents.agents.iter().collect();
            agents.sort_by_key(|(k, _)| (*k).clone());
            println!("Agents ({})", agents.len());
            println!(
                "  {:<22} {:<10} {:<16} {:<10} {:>8}",
                "ID", "PROVIDER", "MODEL", "ROLE", "BUDGET"
            );
            for (id, a) in &agents {
                println!(
                    "  {:<22} {:<10} {:<16} {:<10} {:>7}",
                    id, a.provider, a.model, a.role,
                    format_cost(a.budget_usd)
                );
            }
            println!();

            let d = &cfg.policy.defaults;
            println!("Policy");
            println!(
                "  defaults: model={}, budget=${:.2}, timeout={}s, slots={}",
                d.model, d.budget_usd, d.timeout_s, d.max_slots
            );
            let b = &cfg.policy.budget;
            println!(
                "  budget: ${:.0}/mo ceiling, {}% soft, {}% hard",
                b.monthly_ceiling_usd, b.soft_alert_pct, b.hard_stop_pct
            );
            println!("  rules: {}", cfg.policy.rules.len());
            for r in &cfg.policy.rules {
                let m: Vec<_> = r.match_criteria.iter().map(|(k, v)| format!("{k}={v}")).collect();
                let s: Vec<_> = r.set.iter().map(|(k, v)| format!("{k}={v}")).collect();
                println!("    {} -> {}", m.join(", "), s.join(", "));
            }

            if !cfg.policy.features.is_empty() {
                let enabled: Vec<_> = cfg.policy.features.iter()
                    .filter(|(_, v)| v.as_bool() == Some(true))
                    .map(|(k, _)| k.as_str()).collect();
                let disabled: Vec<_> = cfg.policy.features.iter()
                    .filter(|(_, v)| v.as_bool() == Some(false))
                    .map(|(k, _)| k.as_str()).collect();
                if !enabled.is_empty() {
                    println!("  features ON: {}", enabled.join(", "));
                }
                if !disabled.is_empty() {
                    println!("  features OFF: {}", disabled.join(", "));
                }
            }
        }
        Err(e) => {
            eprintln!("Error loading config: {e}");
            eprintln!("Create config files in: {}", dir.display());
            std::process::exit(1);
        }
    }
}

fn cmd_triage() {
    let bus_path = bus::bus_dir();
    let entries = ops::list_triage(&bus_path);

    if entries.is_empty() {
        println!("No tasks pending triage.");
        return;
    }

    println!("Tasks pending triage ({})\n", entries.len());
    println!(
        "  {:<40} {:<12} {:<8} {:<8} ERROR",
        "ID", "PROJECT", "MODEL", "SOURCE"
    );
    for e in &entries {
        println!(
            "  {:<40} {:<12} {:<8} {:<8} {}",
            truncate(&e.task_id, 40),
            e.project,
            e.model,
            e.source,
            truncate(&e.error_excerpt, 30)
        );
    }
    println!("\nActions: punk-run retry <id> | punk-run cancel <id>");
}

fn cmd_goal(project: &str, objective: &str, budget: f64, deadline: Option<&str>) {
    let bus_path = bus::bus_dir();
    let config_dir = config::config_dir();

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

    // Resolve project path
    let project_path = if let Ok(cfg) = config::load(&config_dir) {
        cfg.projects
            .projects
            .iter()
            .find(|p| p.id == project)
            .map(|p| p.path.replace('~', &dirs::home_dir().unwrap_or_default().to_string_lossy()))
            .unwrap_or_default()
    } else {
        String::new()
    };

    if project_path.is_empty() {
        eprintln!("Warning: project '{}' not found in config, skipping planner", project);
        println!("Add plan manually or configure project in ~/.config/punk/projects.toml");
        return;
    }

    // Generate plan via CLI
    println!("Generating plan...");
    let prompt = goal::build_planner_prompt(&g, std::path::Path::new(&project_path));

    let output = std::process::Command::new("claude")
        .args(["-p", &prompt, "--output-format", "text", "--model", "sonnet"])
        .env_remove("CLAUDECODE")
        .env_remove("ANTHROPIC_API_KEY")
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            match goal::parse_plan(&text, "claude-sonnet") {
                Some(plan) => {
                    let step_count = plan.steps.len();
                    let est_cost: f64 = plan.steps.iter().map(|s| s.est_cost_usd).sum();
                    g.plan = Some(plan);
                    g.status = goal::GoalStatus::AwaitingApproval;
                    goal::save_goal(&bus_path, &g).ok();

                    println!("Plan generated: {} steps, ${:.2} estimated\n", step_count, est_cost);
                    println!("Review and approve:");
                    println!("  punk-run approve {}", g.id);
                }
                None => {
                    eprintln!("Failed to parse planner output. Raw output saved to goal file.");
                    eprintln!("Try: punk-run approve {} (after manual plan edit)", g.id);
                }
            }
        }
        Ok(out) => {
            eprintln!("Planner failed (exit {})", out.status.code().unwrap_or(-1));
            eprintln!("{}", String::from_utf8_lossy(&out.stderr));
        }
        Err(e) => {
            eprintln!("Failed to invoke planner: {e}");
            eprintln!("Install claude CLI or add plan manually");
        }
    }
}

fn cmd_goals() {
    let bus_path = bus::bus_dir();
    let goals = goal::list_goals(&bus_path);

    if goals.is_empty() {
        println!("No goals.");
        return;
    }

    println!("Goals ({})\n", goals.len());
    println!(
        "  {:<35} {:<12} {:<10} {:>8} {:>8} OBJECTIVE",
        "ID", "PROJECT", "STATUS", "SPENT", "BUDGET"
    );
    for g in &goals {
        println!(
            "  {:<35} {:<12} {:<10} {:>8} {:>8} {}",
            truncate(&g.id, 35),
            g.project,
            format!("{:?}", g.status).to_lowercase(),
            format_cost(g.spent_usd),
            format_cost(g.budget_usd),
            truncate(&g.objective, 40)
        );
        if let Some(ref plan) = g.plan {
            let done = plan.steps.iter().filter(|s| s.status == goal::StepStatus::Done).count();
            let total = plan.steps.len();
            println!("    plan v{}: {done}/{total} steps done", plan.version);
        }
    }
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

    if g.plan.is_none() {
        eprintln!("Goal has no plan yet. Run planner first.");
        std::process::exit(1);
    }

    if g.status != goal::GoalStatus::AwaitingApproval && g.status != goal::GoalStatus::Planning {
        eprintln!("Goal status is {:?}, cannot approve.", g.status);
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
        println!("\n  Total estimated: ${total_cost:.2} / ${:.2} budget", g.budget_usd);
    }

    // Approve
    if let Some(ref mut plan) = g.plan {
        plan.approved_at = Some(punk_orch::chrono::Utc::now());
    }
    g.status = goal::GoalStatus::Active;

    // Queue first ready steps
    let queued = goal::queue_ready_steps(&bus_path, &mut g);

    if let Err(e) = goal::save_goal(&bus_path, &g) {
        eprintln!("Error saving goal: {e}");
        std::process::exit(1);
    }

    println!("\nApproved. {} step(s) queued: {}", queued.len(), queued.join(", "));
}

async fn cmd_diverge(project: &str, spec: &str, timeout: u64) {
    let config_dir = config::config_dir();
    let project_path = if let Ok(cfg) = config::load(&config_dir) {
        cfg.projects.projects.iter()
            .find(|p| p.id == project)
            .map(|p| p.path.replace('~', &dirs::home_dir().unwrap_or_default().to_string_lossy()))
    } else {
        None
    };

    let path = match project_path {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            eprintln!("Project '{project}' not found in config");
            std::process::exit(1);
        }
    };

    let strategies = diverge::Strategy::defaults();
    println!("Diverge: dispatching to {} providers...\n", strategies.len());

    match diverge::run_diverge(&path, spec, &strategies, timeout).await {
        Ok(solutions) => {
            println!("{:<6} {:<10} {:<6} {:>6} {:>6} FILES", "LABEL", "PROVIDER", "EXIT", "+LINES", "-LINES");
            for s in &solutions {
                println!(
                    "{:<6} {:<10} {:<6} {:>6} {:>6} {}",
                    s.label, s.provider, s.exit_code, s.lines_added, s.lines_removed,
                    s.files_changed.len()
                );
            }
            println!("\nWorktrees preserved. Inspect with: git -C <worktree> diff HEAD");
        }
        Err(e) => {
            eprintln!("Diverge failed: {e}");
            std::process::exit(1);
        }
    }
}

async fn cmd_panel(question: &str, timeout: u64) {
    println!("Panel: asking all providers...\n");

    let responses = panel::ask_all(question, timeout).await;

    for r in &responses {
        println!("### {} {}", r.provider, if r.exit_code == 0 { "" } else { "(FAILED)" });
        if let Some(ref err) = r.error {
            println!("  Error: {err}");
        } else {
            // Show first 500 chars
            let preview: String = r.answer.chars().take(500).collect();
            println!("{preview}");
        }
        println!();
    }

    let ok_count = responses.iter().filter(|r| r.exit_code == 0).count();
    println!("Panel: {ok_count}/{} providers responded", responses.len());
}

fn cmd_ratchet() {
    let bus_path = bus::bus_dir();
    let current = ratchet::compute_metrics_window(&bus_path, 0, 7);
    let previous = ratchet::compute_metrics_window(&bus_path, 7, 14);

    println!("Metric Ratchet\n");
    println!("  This week:  {}", ratchet::format_metrics(&current));
    println!("  Last week:  {}", ratchet::format_metrics(&previous));
    println!();

    let directives = ratchet::compare(&current, &previous);
    if directives.is_empty() {
        println!("  No significant changes.");
    } else {
        for d in &directives {
            println!("  {d}");
        }
    }
}

fn cmd_policy_check(project: &str, category: &str, priority: &str) {
    let config_dir = config::config_dir();
    match config::load(&config_dir) {
        Ok(cfg) => {
            let d = &cfg.policy.defaults;
            let mut model = d.model.clone();
            let mut budget = d.budget_usd;
            let mut timeout = d.timeout_s;

            // Apply matching rules
            for rule in &cfg.policy.rules {
                let matches = rule.match_criteria.iter().all(|(k, v)| {
                    match k.as_str() {
                        "project" => v == project,
                        "category" => v == category,
                        "priority" => v == priority,
                        _ => false,
                    }
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
            let (pressure, spent) = punk_orch::budget::check_pressure(&bus_path, cfg.policy.budget.monthly_ceiling_usd, cfg.policy.budget.soft_alert_pct, cfg.policy.budget.hard_stop_pct);
            println!("  Budget:   ${spent:.2} / ${:.0} ({pressure:?})", cfg.policy.budget.monthly_ceiling_usd);

            if !punk_orch::budget::priority_allowed(&pressure, priority) {
                println!("\n  BLOCKED: priority {priority} not allowed at {pressure:?} pressure level");
            } else {
                println!("\n  OK: task would be dispatched");
            }
        }
        Err(e) => {
            eprintln!("Error loading config: {e}");
            std::process::exit(1);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_queue(
    project: &str, prompt: &str, agent: &str, category: &str,
    priority: &str, timeout: u64, budget: Option<f64>, worktree: bool, after: Option<&str>,
) {
    let bus_path = bus::bus_dir();
    let config_dir = config::config_dir();

    // Resolve project path from config
    let project_path = config::load(&config_dir)
        .ok()
        .and_then(|cfg| {
            cfg.projects.projects.iter()
                .find(|p| p.id == project)
                .map(|p| p.path.clone())
        })
        .unwrap_or_else(|| format!("~/personal/heurema/{project}"));

    let task_id = format!("{}-{}", project, punk_orch::chrono::Utc::now().format("%Y%m%d-%H%M%S"));

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

fn cmd_receipts(project_filter: Option<&str>, since_days: i64) {
    let bus_path = bus::bus_dir();
    let index = bus_path.parent().unwrap_or(&bus_path).join("receipts/index.jsonl");

    let content = match std::fs::read_to_string(&index) {
        Ok(c) => c,
        Err(_) => {
            println!("No receipts found.");
            return;
        }
    };

    let cutoff = (punk_orch::chrono::Utc::now() - punk_orch::chrono::Duration::days(since_days)).to_rfc3339();

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

        let ts = v.get("created_at").or_else(|| v.get("completed_at"))
            .and_then(|t| t.as_str()).unwrap_or("");
        if ts < cutoff.as_str() { continue; }

        let proj = v.get("project").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(filter) = project_filter {
            if proj != filter { continue; }
        }

        let task_id = v.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        let model = v.get("model").and_then(|v| v.as_str()).unwrap_or("");
        let status = v.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let cost = v.get("cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let dur = v.get("duration_ms").and_then(|v| v.as_u64())
            .or_else(|| v.get("duration_seconds").and_then(|v| v.as_u64()).map(|s| s * 1000))
            .unwrap_or(0) / 1000;

        println!(
            "{:<40} {:<12} {:<8} {:<9} {:>7} {:>5}s",
            truncate(task_id, 40), proj, model, status, format_cost(cost), dur
        );
        count += 1;
    }
    println!("\n{count} receipts (last {since_days}d)");
}

fn cmd_ask(question: &str) {
    let bus_path = bus::bus_dir();
    let state = bus::read_state(&bus_path, 20);

    // Build deterministic data snapshot
    let mut context = format!("Data snapshot ({}):\n", punk_orch::chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S"));
    context.push_str(&format!("- Recent: {} tasks ({} ok)\n",
        state.done.len(),
        state.done.iter().filter(|t| t.status == "success").count()
    ));
    if !state.running.is_empty() {
        context.push_str(&format!("- Running: {} tasks\n", state.running.len()));
    }
    if !state.queued.is_empty() {
        context.push_str(&format!("- Queued: {} tasks\n", state.queued.len()));
    }
    if !state.failed.is_empty() {
        context.push_str(&format!("- Failed: {} tasks pending triage\n", state.failed.len()));
        for t in &state.failed {
            context.push_str(&format!("  - {} ({}, {})\n", t.id, t.project, t.model));
        }
    }
    let goals = goal::list_goals(&bus_path);
    if !goals.is_empty() {
        context.push_str(&format!("- Goals: {}\n", goals.len()));
        for g in &goals {
            context.push_str(&format!("  - {} ({:?}, ${:.2}/${:.2})\n", g.id, g.status, g.spent_usd, g.budget_usd));
        }
    }

    let prompt = format!(
        "{context}\n\nBased ONLY on the data above, answer: {question}\nRules: cite task/goal IDs, don't invent data not in the snapshot, say 'unknown' if data insufficient."
    );

    // Call Claude haiku for fast answer
    let output = std::process::Command::new("claude")
        .args(["-p", &prompt, "--output-format", "text", "--model", "haiku"])
        .env_remove("CLAUDECODE")
        .env_remove("ANTHROPIC_API_KEY")
        .output();

    match output {
        Ok(out) if out.status.success() => {
            println!("{}", String::from_utf8_lossy(&out.stdout));
        }
        Ok(out) => {
            eprintln!("claude failed (exit {})", out.status.code().unwrap_or(-1));
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("claude not found. Install: https://claude.ai/download");
            std::process::exit(1);
        }
    }
}

fn cmd_pipeline_list() {
    let bus_path = bus::bus_dir();
    let opps = pipeline::load_pipeline(&bus_path);

    if opps.is_empty() {
        println!("Pipeline empty.");
        return;
    }

    println!("Pipeline ({} opportunities)\n", opps.len());
    println!(
        "  {:<4} {:<12} {:<15} {:<14} {:<20} {:>8}",
        "ID", "PROJECT", "CONTACT", "STAGE", "NEXT STEP", "VALUE"
    );
    for o in &opps {
        let val = o.value_usd.map(|v| format!("${v}")).unwrap_or_default();
        println!(
            "  {:<4} {:<12} {:<15} {:<14} {:<20} {:>8}",
            o.id,
            truncate(&o.project, 12),
            truncate(&o.contact, 15),
            format!("{:?}", o.stage).to_lowercase(),
            truncate(&o.next_step, 20),
            val
        );
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

    println!("Goal: {}", g.id);
    println!("  project:   {}", g.project);
    println!("  objective: {}", g.objective);
    println!("  status:    {:?}", g.status);
    println!("  budget:    ${:.2} (spent: ${:.2})", g.budget_usd, g.spent_usd);
    if let Some(ref d) = g.deadline {
        println!("  deadline:  {d}");
    }
    println!();

    if let Some(ref plan) = g.plan {
        println!("Plan v{} ({} steps):\n", plan.version, plan.steps.len());
        for step in &plan.steps {
            let status_icon = match step.status {
                goal::StepStatus::Done => "[x]",
                goal::StepStatus::Running => "[>]",
                goal::StepStatus::Blocked => "[!]",
                goal::StepStatus::Pending => "[ ]",
                goal::StepStatus::Skipped => "[-]",
            };
            println!(
                "  {} {}. [{}] {} (${:.2})",
                status_icon, step.step, step.category, step.prompt, step.est_cost_usd
            );
            if let Some(ref tid) = step.task_id {
                println!("      task: {tid}");
            }
        }
    } else {
        println!("  No plan yet.");
    }
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

    let old = format!("{:?}", g.status);
    g.status = new_status;
    let new = format!("{:?}", g.status);

    if let Err(e) = goal::save_goal(&bus_path, &g) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    println!("{goal_id}: {old} -> {new}");
}

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
