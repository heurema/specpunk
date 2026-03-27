use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;

use tokio::process::Command;

/// Result of running an adapter.
#[derive(Debug)]
pub struct RunResult {
    pub exit_code: i32,
    pub duration_ms: u64,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub pid: u32,
}

/// Provider-specific adapter. Each knows how to invoke its CLI.
#[derive(Debug, Clone)]
pub enum Adapter {
    Claude(ClaudeAdapter),
    Codex(CodexAdapter),
    Gemini(GeminiAdapter),
}

impl Adapter {
    pub fn from_provider(provider: &str) -> Option<Self> {
        match provider {
            "claude" => Some(Self::Claude(ClaudeAdapter)),
            "codex" => Some(Self::Codex(CodexAdapter)),
            "gemini" => Some(Self::Gemini(GeminiAdapter)),
            _ => None,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Claude(_) => "claude",
            Self::Codex(_) => "codex",
            Self::Gemini(_) => "gemini",
        }
    }

    /// Spawn the adapter process. Returns immediately with PID.
    /// Caller is responsible for waiting and collecting result.
    pub async fn spawn(
        &self,
        task: &TaskSpec,
        staging_dir: &Path,
    ) -> std::io::Result<SpawnedProcess> {
        match self {
            Self::Claude(a) => a.spawn(task, staging_dir).await,
            Self::Codex(a) => a.spawn(task, staging_dir).await,
            Self::Gemini(a) => a.spawn(task, staging_dir).await,
        }
    }
}

/// Minimal task spec passed to adapters (extracted from task.json).
#[derive(Debug, Clone)]
pub struct TaskSpec {
    pub task_id: String,
    pub project: String,
    pub project_path: PathBuf,
    pub prompt: String,
    pub model: String,
    pub timeout_s: u64,
    pub budget_usd: Option<f64>,
    pub allowed_tools: String,
    pub disallowed_tools: String,
}

/// A spawned adapter process, not yet completed.
pub struct SpawnedProcess {
    pub child: tokio::process::Child,
    pub pid: u32,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub started_at: Instant,
}

impl SpawnedProcess {
    /// Wait for completion, return RunResult.
    pub async fn wait(mut self) -> RunResult {
        let status = self.child.wait().await;
        let exit_code = status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
        let duration_ms = self.started_at.elapsed().as_millis() as u64;

        RunResult {
            exit_code,
            duration_ms,
            stdout_path: self.stdout_path,
            stderr_path: self.stderr_path,
            pid: self.pid,
        }
    }
}

// --- Claude Adapter ---

#[derive(Debug, Clone, Default)]
pub struct ClaudeAdapter;

impl ClaudeAdapter {
    async fn spawn(&self, task: &TaskSpec, staging_dir: &Path) -> std::io::Result<SpawnedProcess> {
        let stdout_path = staging_dir.join("stdout.json");
        let stderr_path = staging_dir.join("stderr.log");

        let stdout_file = std::fs::File::create(&stdout_path)?;
        let stderr_file = std::fs::File::create(&stderr_path)?;

        let mut cmd = Command::new("claude");
        cmd.arg("-p")
            .arg(&task.prompt)
            .arg("--allowedTools")
            .arg(&task.allowed_tools)
            .arg("--output-format")
            .arg("json");

        if !task.disallowed_tools.is_empty() {
            cmd.arg("--disallowedTools").arg(&task.disallowed_tools);
        }
        if let Some(budget) = task.budget_usd {
            cmd.arg("--max-budget-usd").arg(budget.to_string());
        }
        if !task.model.is_empty() {
            cmd.arg("--model").arg(&task.model);
        }

        cmd.current_dir(&task.project_path);
        cmd.stdout(Stdio::from(stdout_file));
        cmd.stderr(Stdio::from(stderr_file));

        // Critical: unset CLAUDECODE to avoid nested-session detection
        cmd.env_remove("CLAUDECODE");
        // Unset ANTHROPIC_API_KEY to avoid shadowing subscription auth
        cmd.env_remove("ANTHROPIC_API_KEY");

        // Use OAuth token if available
        if let Ok(token) = std::env::var("PUNK_CLAUDE_TOKEN") {
            cmd.env("CLAUDE_CODE_OAUTH_TOKEN", token);
        }

        // Env var contract for agent context
        cmd.env("PUNK_TASK_ID", &task.task_id);
        cmd.env("PUNK_PROJECT", &task.project);

        let child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);

        Ok(SpawnedProcess {
            child,
            pid,
            stdout_path,
            stderr_path,
            started_at: Instant::now(),
        })
    }
}

// --- Codex Adapter ---

#[derive(Debug, Clone, Default)]
pub struct CodexAdapter;

impl CodexAdapter {
    async fn spawn(&self, task: &TaskSpec, staging_dir: &Path) -> std::io::Result<SpawnedProcess> {
        let stdout_path = staging_dir.join("stdout.json");
        let stderr_path = staging_dir.join("stderr.log");

        let stdout_file = std::fs::File::create(&stdout_path)?;
        let stderr_file = std::fs::File::create(&stderr_path)?;

        let mut cmd = Command::new("codex");
        cmd.arg("exec")
            .arg("--full-auto")
            .arg("--json")
            .arg(&task.prompt);

        if !task.model.is_empty() {
            cmd.arg("--model").arg(&task.model);
        }

        cmd.current_dir(&task.project_path);
        cmd.stdout(Stdio::from(stdout_file));
        cmd.stderr(Stdio::from(stderr_file));

        cmd.env("PUNK_TASK_ID", &task.task_id);
        cmd.env("PUNK_PROJECT", &task.project);

        let child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);

        Ok(SpawnedProcess {
            child,
            pid,
            stdout_path,
            stderr_path,
            started_at: Instant::now(),
        })
    }
}

// --- Gemini Adapter ---

#[derive(Debug, Clone, Default)]
pub struct GeminiAdapter;

impl GeminiAdapter {
    async fn spawn(&self, task: &TaskSpec, staging_dir: &Path) -> std::io::Result<SpawnedProcess> {
        let stdout_path = staging_dir.join("stdout.txt");
        let stderr_path = staging_dir.join("stderr.log");

        let stdout_file = std::fs::File::create(&stdout_path)?;
        let stderr_file = std::fs::File::create(&stderr_path)?;

        let mut cmd = Command::new("gemini");
        cmd.arg("--yolo")
            .arg("-p")
            .arg(&task.prompt)
            .arg("-o")
            .arg("text");

        if !task.model.is_empty() {
            cmd.arg("--model").arg(&task.model);
        }

        cmd.current_dir(&task.project_path);
        cmd.stdout(Stdio::from(stdout_file));
        cmd.stderr(Stdio::from(stderr_file));

        cmd.env("PUNK_TASK_ID", &task.task_id);
        cmd.env("PUNK_PROJECT", &task.project);

        let child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);

        Ok(SpawnedProcess {
            child,
            pid,
            stdout_path,
            stderr_path,
            started_at: Instant::now(),
        })
    }
}
