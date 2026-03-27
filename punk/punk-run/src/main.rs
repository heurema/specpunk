use clap::{Parser, Subcommand};
use punk_orch::{bus, config, daemon, doctor, morning, ops};

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
