use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use chrono::Utc;
use tokio::process::Command;

/// Strategy hint for each provider.
#[derive(Debug, Clone)]
pub struct Strategy {
    pub label: String,
    pub hint: String,
    pub provider: String,
}

impl Strategy {
    pub fn defaults() -> Vec<Self> {
        vec![
            Self {
                label: "A".into(),
                hint: "Solve with the smallest possible change. Minimize lines changed, new files, and new dependencies.".into(),
                provider: "claude".into(),
            },
            Self {
                label: "B".into(),
                hint: "Solve by improving the existing code structure. Extract helpers, rename for clarity, improve testability.".into(),
                provider: "codex".into(),
            },
            Self {
                label: "C".into(),
                hint: "Solve with the best possible abstraction. Introduce new modules or patterns if justified.".into(),
                provider: "gemini".into(),
            },
        ]
    }
}

/// Result of one diverge solution.
#[derive(Debug)]
pub struct SolutionResult {
    pub label: String,
    pub provider: String,
    pub strategy_hint: String,
    pub exit_code: i32,
    pub files_changed: Vec<String>,
    pub lines_added: u32,
    pub lines_removed: u32,
    pub stdout_path: PathBuf,
}

/// Run diverge: dispatch spec to multiple providers in isolated worktrees.
pub async fn run_diverge(
    project_path: &Path,
    spec: &str,
    strategies: &[Strategy],
    timeout_s: u64,
) -> Result<Vec<SolutionResult>, String> {
    // Preflight: check git repo
    if !project_path.join(".git").exists() {
        return Err("not a git repository".into());
    }

    let base_commit = git_output(project_path, &["rev-parse", "HEAD"])?;
    let run_id = format!(
        "{}-{}",
        Utc::now().format("%Y%m%dT%H%M%S"),
        std::process::id()
    );
    let run_dir = std::env::temp_dir().join(format!("punk-diverge-{run_id}"));
    fs::create_dir_all(&run_dir).map_err(|e| e.to_string())?;

    // Create worktrees
    let mut worktrees = Vec::new();
    for s in strategies {
        let wt_path = run_dir.join(&s.label);
        let result = std::process::Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(&wt_path)
            .arg(&base_commit)
            .current_dir(project_path)
            .output();

        match result {
            Ok(out) if out.status.success() => worktrees.push(wt_path),
            _ => {
                cleanup_worktrees(project_path, &worktrees);
                return Err(format!("failed to create worktree for {}", s.label));
            }
        }
    }

    // Write prompt files
    for (i, s) in strategies.iter().enumerate() {
        let prompt = format!(
            "# Implementation Task\n\n## Strategy\n{}\n\n## Specification\n{}\n\n## Rules\n- Do NOT commit. Leave changes as uncommitted files.\n- Follow existing project patterns.\n",
            s.hint, spec
        );
        fs::write(worktrees[i].join(".diverge-prompt.md"), &prompt).ok();
    }

    // Dispatch all providers in parallel
    let mut handles = Vec::new();
    for (i, s) in strategies.iter().enumerate() {
        let wt = worktrees[i].clone();
        let provider = s.provider.clone();
        let label = s.label.clone();
        let timeout = timeout_s;

        let handle = tokio::spawn(async move {
            let result = dispatch_provider(&provider, &wt, timeout).await;
            (label, provider, result)
        });
        handles.push(handle);
    }

    // Collect results
    let mut solutions = Vec::new();
    for (i, handle) in handles.into_iter().enumerate() {
        let (label, provider, result) = handle.await.map_err(|e| e.to_string())?;
        let exit_code = result.unwrap_or(-1);

        // Get diff stats
        let (files, added, removed) = diff_stats(&worktrees[i], &base_commit);

        solutions.push(SolutionResult {
            label,
            provider,
            strategy_hint: strategies[i].hint.clone(),
            exit_code,
            files_changed: files,
            lines_added: added,
            lines_removed: removed,
            stdout_path: worktrees[i].join("stdout.log"),
        });
    }

    // Don't cleanup worktrees yet — caller may want to inspect or merge
    // Cleanup happens when user selects or discards

    Ok(solutions)
}

/// Cleanup worktrees.
pub fn cleanup_worktrees(project_path: &Path, worktrees: &[PathBuf]) {
    for wt in worktrees {
        std::process::Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(wt)
            .current_dir(project_path)
            .output()
            .ok();
    }
}

async fn dispatch_provider(provider: &str, worktree: &Path, timeout_s: u64) -> Result<i32, String> {
    let prompt_path = worktree.join(".diverge-prompt.md");
    let prompt = fs::read_to_string(&prompt_path).map_err(|e| e.to_string())?;
    let stdout_path = worktree.join("stdout.log");
    let stderr_path = worktree.join("stderr.log");

    let stdout_file = fs::File::create(&stdout_path).map_err(|e| e.to_string())?;
    let stderr_file = fs::File::create(&stderr_path).map_err(|e| e.to_string())?;

    let mut cmd = match provider {
        "claude" => {
            let mut c = Command::new("claude");
            c.args(["-p", &prompt, "--output-format", "text"]);
            c.env_remove("CLAUDECODE");
            c.env_remove("ANTHROPIC_API_KEY");
            c
        }
        "codex" => {
            let mut c = Command::new("codex");
            c.args(["exec", "--full-auto", &prompt]);
            c
        }
        "gemini" => {
            let mut c = Command::new("gemini");
            c.args(["--yolo", "-p", &prompt, "-o", "text"]);
            c
        }
        _ => return Err(format!("unknown provider: {provider}")),
    };

    cmd.current_dir(worktree);
    cmd.stdout(Stdio::from(stdout_file));
    cmd.stderr(Stdio::from(stderr_file));

    let child = cmd.spawn().map_err(|e| format!("{provider}: {e}"))?;

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_s),
        child.wait_with_output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => Ok(output.status.code().unwrap_or(-1)),
        Ok(Err(e)) => Err(format!("{provider}: {e}")),
        Err(_) => Ok(124), // timeout
    }
}

fn git_output(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn diff_stats(worktree: &Path, base_commit: &str) -> (Vec<String>, u32, u32) {
    let files = std::process::Command::new("git")
        .args(["diff", "--name-only", base_commit])
        .current_dir(worktree)
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let numstat = std::process::Command::new("git")
        .args(["diff", "--numstat", base_commit])
        .current_dir(worktree)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let (mut added, mut removed) = (0u32, 0u32);
    for line in numstat.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            added += parts[0].parse::<u32>().unwrap_or(0);
            removed += parts[1].parse::<u32>().unwrap_or(0);
        }
    }

    (files, added, removed)
}
