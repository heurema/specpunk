use std::fs;
use std::process::Stdio;

use tokio::process::Command;

/// Response from a single provider.
#[derive(Debug)]
pub struct ProviderResponse {
    pub provider: String,
    pub answer: String,
    pub exit_code: i32,
    pub error: Option<String>,
}

/// Ask all providers the same question in parallel.
pub async fn ask_all(question: &str, timeout_s: u64) -> Vec<ProviderResponse> {
    let providers = vec!["claude", "codex", "gemini"];
    let mut handles = Vec::new();

    for provider in providers {
        let q = question.to_string();
        let t = timeout_s;
        let p = provider.to_string();
        let handle = tokio::spawn(async move { ask_provider(&p, &q, t).await });
        handles.push((provider.to_string(), handle));
    }

    let mut responses = Vec::new();
    for (provider, handle) in handles {
        match handle.await {
            Ok(resp) => responses.push(resp),
            Err(e) => responses.push(ProviderResponse {
                provider,
                answer: String::new(),
                exit_code: -1,
                error: Some(format!("join error: {e}")),
            }),
        }
    }

    responses
}

async fn ask_provider(provider: &str, question: &str, timeout_s: u64) -> ProviderResponse {
    let tmp_out =
        std::env::temp_dir().join(format!("punk-panel-{provider}-{}.txt", std::process::id()));

    let result = match provider {
        "claude" => {
            let mut cmd = Command::new("claude");
            cmd.args([
                "-p",
                question,
                "--output-format",
                "text",
                "--model",
                "sonnet",
            ]);
            cmd.env_remove("CLAUDECODE");
            cmd.env_remove("ANTHROPIC_API_KEY");
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::null());
            run_with_timeout(cmd, timeout_s).await
        }
        "codex" => {
            let out_file = tmp_out.clone();
            let mut cmd = Command::new("codex");
            cmd.args(["exec", "--ephemeral", "-p", "fast", "--output-last-message"]);
            cmd.arg(&out_file);
            cmd.arg(question);
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
            let res = run_with_timeout(cmd, timeout_s).await;
            // Read output from file
            if res.is_ok() {
                if let Ok(content) = fs::read_to_string(&out_file) {
                    fs::remove_file(&out_file).ok();
                    return ProviderResponse {
                        provider: provider.to_string(),
                        answer: content,
                        exit_code: 0,
                        error: None,
                    };
                }
            }
            fs::remove_file(&out_file).ok();
            res
        }
        "gemini" => {
            let mut cmd = Command::new("gemini");
            cmd.args(["-p", question, "-o", "text"]);
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::null());
            run_with_timeout(cmd, timeout_s).await
        }
        _ => Err(format!("unknown provider: {provider}")),
    };

    match result {
        Ok(output) => ProviderResponse {
            provider: provider.to_string(),
            answer: output,
            exit_code: 0,
            error: None,
        },
        Err(e) => ProviderResponse {
            provider: provider.to_string(),
            answer: String::new(),
            exit_code: 1,
            error: Some(e),
        },
    }
}

async fn run_with_timeout(mut cmd: Command, timeout_s: u64) -> Result<String, String> {
    let child = cmd.spawn().map_err(|e| e.to_string())?;

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_s),
        child.wait_with_output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        }
        Ok(Ok(output)) => Err(format!("exit {}", output.status.code().unwrap_or(-1))),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("timeout".into()),
    }
}
