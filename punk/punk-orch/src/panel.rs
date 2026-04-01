use std::fs;
use std::process::Stdio;
use std::time::Instant;

use tokio::process::Command;

use crate::config;

/// Response from a single provider.
#[derive(Debug)]
pub struct ProviderResponse {
    pub provider: String,
    pub answer: String,
    pub exit_code: i32,
    pub error: Option<String>,
    pub duration_ms: u128,
    pub timed_out: bool,
}

#[derive(Debug)]
pub struct PanelReport {
    pub available_providers: Vec<String>,
    pub responses: Vec<ProviderResponse>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PanelSummary {
    pub available: usize,
    pub responded: usize,
    pub failed: usize,
    pub timed_out: usize,
}

pub fn detect_available_providers() -> Vec<String> {
    detect_available_providers_with(|name| config::detect_agents().agents.contains_key(name))
}

fn detect_available_providers_with<F>(mut exists: F) -> Vec<String>
where
    F: FnMut(&str) -> bool,
{
    let mut providers = Vec::new();
    for provider in ["claude", "codex", "gemini"] {
        if exists(provider) {
            providers.push(provider.to_string());
        }
    }
    providers
}

pub fn summarize(report: &PanelReport) -> PanelSummary {
    let responded = report.responses.iter().filter(|r| r.exit_code == 0).count();
    let timed_out = report.responses.iter().filter(|r| r.timed_out).count();
    let failed = report.responses.len().saturating_sub(responded);
    PanelSummary {
        available: report.available_providers.len(),
        responded,
        failed,
        timed_out,
    }
}

/// Ask all detected providers the same question in parallel.
pub async fn ask_all(question: &str, timeout_s: u64) -> PanelReport {
    let providers = detect_available_providers();
    let mut handles = Vec::new();

    for provider in &providers {
        let q = question.to_string();
        let t = timeout_s;
        let p = provider.to_string();
        let handle = tokio::spawn(async move { ask_provider(&p, &q, t).await });
        handles.push((provider.clone(), handle));
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
                duration_ms: 0,
                timed_out: false,
            }),
        }
    }

    PanelReport {
        available_providers: providers,
        responses,
    }
}

async fn ask_provider(provider: &str, question: &str, timeout_s: u64) -> ProviderResponse {
    let started = Instant::now();
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
                        duration_ms: started.elapsed().as_millis(),
                        timed_out: false,
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
            duration_ms: started.elapsed().as_millis(),
            timed_out: false,
        },
        Err(e) => ProviderResponse {
            provider: provider.to_string(),
            answer: String::new(),
            exit_code: 1,
            timed_out: e == "timeout",
            error: Some(e),
            duration_ms: started.elapsed().as_millis(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_available_providers_keeps_supported_order() {
        let providers = detect_available_providers_with(|name| name != "claude");
        assert_eq!(providers, vec!["codex".to_string(), "gemini".to_string()]);
    }

    #[test]
    fn summarize_counts_success_failure_and_timeout() {
        let report = PanelReport {
            available_providers: vec!["claude".into(), "codex".into(), "gemini".into()],
            responses: vec![
                ProviderResponse {
                    provider: "claude".into(),
                    answer: "ok".into(),
                    exit_code: 0,
                    error: None,
                    duration_ms: 10,
                    timed_out: false,
                },
                ProviderResponse {
                    provider: "codex".into(),
                    answer: String::new(),
                    exit_code: 1,
                    error: Some("timeout".into()),
                    duration_ms: 20,
                    timed_out: true,
                },
                ProviderResponse {
                    provider: "gemini".into(),
                    answer: String::new(),
                    exit_code: 1,
                    error: Some("exit 1".into()),
                    duration_ms: 5,
                    timed_out: false,
                },
            ],
        };

        assert_eq!(
            summarize(&report),
            PanelSummary {
                available: 3,
                responded: 1,
                failed: 2,
                timed_out: 1,
            }
        );
    }
}
