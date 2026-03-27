use clap::{Parser, Subcommand};
use punk_orch::{bus, config, daemon, doctor, goal, morning, ops, pipeline};

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
    },
    /// Show loaded configuration
    Config,
    /// Start the daemon (foreground)
    Daemon {
        /// Shadow mode: log decisions without dispatching
        #[arg(long)]
        shadow: bool,
        /// Max concurrent slots
        #[arg(long, default_value_t = 5)]
        slots: u32,
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
    /// Cancel a goal
    Cancel { goal_id: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Status { recent } => cmd_status(recent),
        Command::Config => cmd_config(),
        Command::Daemon { shadow, slots } => {
            // Wire policy.toml max_slots if CLI didn't override
            let effective_slots = if slots != 5 {
                slots // explicit CLI override
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
            PipelineAction::Lose { id } => {
                let bus_path = bus::bus_dir();
                match pipeline::set_stage(&bus_path, id, pipeline::Stage::Lost) {
                    Ok(opp) => println!("#{}: LOST", opp.id),
                    Err(e) => { eprintln!("Error: {e}"); std::process::exit(1); }
                }
            }
        },
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
        },
    }

    Ok(())
}

fn cmd_status(recent_limit: usize) {
    let bus_path = bus::bus_dir();
    let state = bus::read_state(&bus_path, recent_limit);

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
