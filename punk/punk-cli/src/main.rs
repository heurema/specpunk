use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use punk_core::init::run_init;

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
    /// Generate an implementation plan from a contract
    Plan,
    /// Check implementation against the active contract
    Check,
    /// Record a receipt for the completed contract
    Receipt,
    /// Show current workspace status
    Status,
    /// Manage punk configuration
    Config,
}

fn main() {
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
        Commands::Plan => {
            eprintln!("punk plan: not yet implemented");
            std::process::exit(1);
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
