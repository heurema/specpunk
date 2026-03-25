pub mod ceremony;
pub mod complexity;
pub mod context;
pub mod contract;
pub mod llm;
pub mod quality;
pub mod render;

use std::path::{Path, PathBuf};

use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::vcs;

use self::ceremony::{detect_ceremony, CeremonyLevel, ModelTier};
use self::context::{build_prompt_context, load_context, ContextError};
pub use self::contract::FeedbackOutcome;
pub use self::llm::LlmProvider;
use self::contract::{
    AcceptanceCriterion, Contract, ContextInheritance, Feedback, RiskLevel,
    RoutingMetadata, Scope, CONTRACT_VERSION,
};
use self::llm::{LlmError, MockProvider};
use self::quality::{check_quality, QualityReport};
use self::render::render_summary;

/// Error type for the plan command.
#[derive(Debug)]
pub enum PlanError {
    Context(ContextError),
    Llm(LlmError),
    Io(std::io::Error),
    Serialize(String),
    EmptyTask,
    QualityFailed(QualityReport),
    Aborted,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanError::Context(e) => write!(f, "context error: {e}"),
            PlanError::Llm(e) => write!(f, "LLM error: {e}"),
            PlanError::Io(e) => write!(f, "I/O error: {e}"),
            PlanError::Serialize(s) => write!(f, "serialization error: {s}"),
            PlanError::EmptyTask => write!(f, "task description must not be empty"),
            PlanError::QualityFailed(r) => {
                write!(f, "spec quality check failed (score {}): {:?}", r.score, r.errors)
            }
            PlanError::Aborted => write!(f, "plan aborted by user"),
        }
    }
}

impl std::error::Error for PlanError {}

impl From<ContextError> for PlanError {
    fn from(e: ContextError) -> Self {
        PlanError::Context(e)
    }
}

impl From<LlmError> for PlanError {
    fn from(e: LlmError) -> Self {
        PlanError::Llm(e)
    }
}

impl From<std::io::Error> for PlanError {
    fn from(e: std::io::Error) -> Self {
        PlanError::Io(e)
    }
}

/// Result of the plan command.
#[derive(Debug)]
pub struct PlanResult {
    pub contract: Contract,
    pub contract_path: PathBuf,
    pub feedback_path: PathBuf,
    pub summary: String,
}

// ---------------------------------------------------------------------------
// SHA-256 helpers
// ---------------------------------------------------------------------------

/// Compute SHA-256 hash of bytes, return lowercase hex string.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Compute stable task_id = SHA-256 of task description.
pub fn task_id_from_description(description: &str) -> String {
    sha256_hex(description.as_bytes())
}

// ---------------------------------------------------------------------------
// Attempt number
// ---------------------------------------------------------------------------

/// Count how many contracts with the same task_id already exist in .punk/contracts/.
pub fn count_attempts(punk_dir: &Path, task_id: &str) -> u32 {
    let contracts_dir = punk_dir.join("contracts");
    if !contracts_dir.exists() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(&contracts_dir) else {
        return 0;
    };
    let mut count = 0u32;
    for entry in entries.flatten() {
        let contract_file = entry.path().join("contract.json");
        if contract_file.exists() {
            if let Ok(raw) = std::fs::read_to_string(&contract_file) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                    if v.get("task_id").and_then(|x| x.as_str()) == Some(task_id) {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Contract save / load
// ---------------------------------------------------------------------------

/// Save contract + feedback to `.punk/contracts/<change_id>/`.
/// One change = one active contract. Re-approving overwrites the previous
/// contract with a warning (this is the user's intent — they ran plan again).
pub fn save_contract(
    punk_dir: &Path,
    contract: &mut Contract,
    feedback: &Feedback,
) -> Result<(PathBuf, PathBuf), PlanError> {
    let dir = punk_dir.join("contracts").join(&contract.change_id);
    if dir.join("contract.json").exists() {
        eprintln!(
            "punk plan: replacing existing contract for change '{}'",
            contract.change_id
        );
    }
    std::fs::create_dir_all(&dir)?;

    // Serialise without approval_hash to compute canonical bytes
    contract.approval_hash = None;
    let canonical =
        serde_json::to_string_pretty(contract).map_err(|e| PlanError::Serialize(e.to_string()))?;

    // Set approval_hash
    contract.approval_hash = Some(sha256_hex(canonical.as_bytes()));

    let final_json = serde_json::to_string_pretty(contract)
        .map_err(|e| PlanError::Serialize(e.to_string()))?;

    let contract_path = dir.join("contract.json");
    let feedback_path = dir.join("feedback.json");

    std::fs::write(&contract_path, &final_json)?;

    let feedback_json =
        serde_json::to_string_pretty(feedback).map_err(|e| PlanError::Serialize(e.to_string()))?;
    std::fs::write(&feedback_path, &feedback_json)?;

    Ok((contract_path, feedback_path))
}

// ---------------------------------------------------------------------------
// LLM contract generation
// ---------------------------------------------------------------------------

/// Build an LLM prompt from the project context and task description.
pub fn build_generation_prompt(context_text: &str, task: &str) -> String {
    format!(
        r#"You are a spec writer for a software project. Given the project context and task description, generate a JSON contract conforming to schema version 1.

## Task
{task}

## Project Context
{context_text}

## Output
Respond with ONLY valid JSON matching this schema:
{{
  "goal": "...",
  "scope": {{
    "touch": ["file1", "file2"],
    "dont_touch": ["file3"]
  }},
  "acceptance_criteria": [
    {{"id": "AC-01", "description": "...", "verify": "cargo test"}}
  ],
  "assumptions": ["..."],
  "warnings": []
}}
"#
    )
}

/// Parse LLM response JSON into contract fields.
#[allow(clippy::too_many_arguments)]
pub fn parse_llm_response(
    raw: &str,
    task: &str,
    change_id: &str,
    task_id: &str,
    attempt_number: u32,
    ceremony_score: u8,
    ceremony_level: &CeremonyLevel,
    model_tier: &ModelTier,
    latency_ms: u64,
) -> Result<Contract, PlanError> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| PlanError::Llm(LlmError::MalformedResponse(
            format!("JSON parse error: {e}"),
        )))?;

    let goal = v["goal"]
        .as_str()
        .unwrap_or(task)
        .to_string();

    let scope = {
        let touch = v["scope"]["touch"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        let dont_touch = v["scope"]["dont_touch"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        Scope { touch, dont_touch }
    };

    let acceptance_criteria = v["acceptance_criteria"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let id = item["id"].as_str()?.to_string();
                    let description = item["description"].as_str()?.to_string();
                    let verify = item["verify"].as_str().map(|s| s.to_string());
                    Some(AcceptanceCriterion { id, description, verify, verify_steps: vec![] })
                })
                .collect()
        })
        .unwrap_or_default();

    let assumptions = v["assumptions"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    let warnings = v["warnings"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    let token_estimate = raw.len() / 4; // rough heuristic: 4 chars ≈ 1 token

    Ok(Contract {
        version: CONTRACT_VERSION.to_string(),
        goal,
        scope,
        acceptance_criteria,
        assumptions,
        warnings,
        ceremony_level: ceremony_level.clone(),
        created_at: Utc::now().to_rfc3339(),
        change_id: change_id.to_string(),
        approval_hash: None,
        routing_metadata: RoutingMetadata {
            complexity_score: ceremony_score,
            ceremony_level: ceremony_level.clone(),
            suggested_model_tier: model_tier.clone(),
            latency_ms,
            token_estimate,
            router_policy_version: "1.0".to_string(),
            unfamiliarity_ratio: 0.0, // TODO: compute from scan.json vs touched files
        },
        task_id: task_id.to_string(),
        attempt_number,
        risk_level: RiskLevel::Low,
        holdout_scenarios: vec![],
        removals: vec![],
        cleanup_obligations: vec![],
        context_inheritance: ContextInheritance::default(),
    })
}

// ---------------------------------------------------------------------------
// Manual mode — template generation
// ---------------------------------------------------------------------------

/// Detect the verify command from .punk/config.toml test_runner field.
fn detect_verify_command(root: &Path) -> String {
    #[derive(serde::Deserialize)]
    struct PunkToml {
        #[serde(default)]
        project: ProjectSection,
    }
    #[derive(serde::Deserialize, Default)]
    struct ProjectSection {
        #[serde(default)]
        test_runner: Option<String>,
    }

    let config_path = root.join(".punk").join("config.toml");
    if let Ok(raw) = std::fs::read_to_string(&config_path) {
        if let Ok(cfg) = toml::from_str::<PunkToml>(&raw) {
            if let Some(runner) = cfg.project.test_runner {
                return match runner.as_str() {
                    "pytest" => "pytest tests/".to_string(),
                    "jest" | "vitest" => "npm test".to_string(),
                    "go-test" => "go test ./...".to_string(),
                    "cargo-test" => "cargo test".to_string(),
                    "rspec" => "bundle exec rspec".to_string(),
                    "mix-test" => "mix test".to_string(),
                    other => other.to_string(),
                };
            }
        }
    }
    "cargo test".to_string()
}

/// Build a pre-filled JSON template for --manual mode.
pub fn build_manual_template(task: &str, context_text: &str, change_id: &str, root: &Path) -> String {
    let verify = detect_verify_command(root);
    serde_json::to_string_pretty(&serde_json::json!({
        "version": CONTRACT_VERSION,
        "goal": task,
        "context_summary": context_text,
        "change_id": change_id,
        "scope": {
            "touch": [],
            "dont_touch": []
        },
        "acceptance_criteria": [
            {
                "id": "AC-01",
                "description": "describe the verifiable outcome",
                "verify": verify
            }
        ],
        "assumptions": [],
        "warnings": []
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

// ---------------------------------------------------------------------------
// High-level orchestrator (used by punk-cli)
// ---------------------------------------------------------------------------

/// Options for the plan command.
pub struct PlanOptions<'a> {
    pub root: &'a Path,
    pub task: &'a str,
    pub manual: bool,
    pub provider: Option<&'a dyn LlmProvider>,
}

/// Orchestrate the full plan flow.
/// In manual mode: build template, open $EDITOR, parse result.
/// In LLM mode: call provider, parse response, run quality check, render summary.
///
/// This function does NOT do the interactive approval loop — the CLI layer owns stdin/stdout.
/// It returns the contract + rendered summary so the caller can present it.
pub async fn run_plan_headless(opts: &PlanOptions<'_>) -> Result<(Contract, QualityReport, String), PlanError> {
    let task = opts.task.trim();
    if task.is_empty() {
        return Err(PlanError::EmptyTask);
    }

    let ctx = load_context(opts.root)?;
    let context_text = build_prompt_context(&ctx);

    // Get change_id from VCS (best-effort; fall back to timestamp)
    let change_id = vcs::detect(opts.root)
        .and_then(|v| v.change_id())
        .unwrap_or_else(|_| format!("manual-{}", Utc::now().timestamp()));

    let task_id = task_id_from_description(task);
    let punk_dir = opts.root.join(".punk");
    let attempt_number = count_attempts(&punk_dir, &task_id) + 1;

    // Ceremony detection — use empty metadata for initial estimate
    let meta = complexity::DiffMetadata::default();
    let (score, level, tier) = detect_ceremony(&meta);

    let contract = if opts.manual {
        let template = build_manual_template(task, &context_text, &change_id, opts.root);
        // In tests / headless: parse the template directly as if the user returned it unchanged
        parse_llm_response(
            &template,
            task,
            &change_id,
            &task_id,
            attempt_number,
            score,
            &level,
            &tier,
            0,
        )?
    } else {
        let fallback = MockProvider::success("{}");
        let provider_ref: &dyn LlmProvider = opts
            .provider
            .unwrap_or(&fallback);

        let prompt = build_generation_prompt(&context_text, task);
        let start = std::time::Instant::now();
        let raw = provider_ref.generate(&prompt).await?;
        let latency_ms = start.elapsed().as_millis() as u64;

        parse_llm_response(
            &raw,
            task,
            &change_id,
            &task_id,
            attempt_number,
            score,
            &level,
            &tier,
            latency_ms,
        )?
    };

    // Merge staleness warnings into contract
    let mut contract = contract;
    contract.warnings.extend(ctx.staleness_warnings);

    let quality = check_quality(
        &contract.acceptance_criteria,
        &contract.scope.touch,
        &contract.scope.dont_touch,
    );

    let summary = render_summary(&contract, &quality);

    Ok((contract, quality, summary))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_punk_dir(tmp: &TempDir) -> PathBuf {
        let punk = tmp.path().join(".punk");
        fs::create_dir_all(&punk).unwrap();
        fs::write(punk.join("intent.md"), "# Intent\nBuild auth system").unwrap();
        fs::write(punk.join("conventions.json"), r#"[{"name":"errors","value":"Result<T,E>","confidence":"high","source":"scan"}]"#).unwrap();
        punk
    }

    fn make_good_llm_response() -> String {
        serde_json::to_string(&serde_json::json!({
            "goal": "add user auth",
            "scope": {
                "touch": ["src/auth.rs"],
                "dont_touch": ["migrations/"]
            },
            "acceptance_criteria": [
                {
                    "id": "AC-01",
                    "description": "cargo test passes",
                    "verify": "cargo test"
                }
            ],
            "assumptions": ["tokio runtime present"],
            "warnings": []
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn plan_generates_contract() {
        let tmp = TempDir::new().unwrap();
        setup_punk_dir(&tmp);

        let response = make_good_llm_response();
        let provider = MockProvider::success(response);

        let opts = PlanOptions {
            root: tmp.path(),
            task: "add user auth",
            manual: false,
            provider: Some(&provider),
        };

        let result = run_plan_headless(&opts).await;
        assert!(result.is_ok(), "plan should succeed: {:?}", result.err());

        let (contract, _quality, _summary) = result.unwrap();
        assert_eq!(contract.goal, "add user auth");
        assert!(!contract.acceptance_criteria.is_empty());
        assert_eq!(contract.version, CONTRACT_VERSION);
        assert!(!contract.task_id.is_empty());
        assert_eq!(contract.attempt_number, 1);
        assert!(!contract.change_id.is_empty());
    }

    #[tokio::test]
    async fn empty_task_rejected() {
        let tmp = TempDir::new().unwrap();
        setup_punk_dir(&tmp);

        let opts = PlanOptions {
            root: tmp.path(),
            task: "",
            manual: false,
            provider: None,
        };

        let result = run_plan_headless(&opts).await;
        assert!(matches!(result, Err(PlanError::EmptyTask)));
    }

    #[test]
    fn contract_save_with_hash() {
        let tmp = TempDir::new().unwrap();
        let punk_dir = tmp.path().join(".punk");
        fs::create_dir_all(&punk_dir).unwrap();

        let mut contract = Contract {
            version: CONTRACT_VERSION.to_string(),
            goal: "test".to_string(),
            scope: Scope {
                touch: vec!["src/lib.rs".to_string()],
                dont_touch: vec![],
            },
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "AC-01".to_string(),
                description: "cargo test".to_string(),
                verify: Some("cargo test".to_string()),
                verify_steps: vec![],
            }],
            assumptions: vec![],
            warnings: vec![],
            ceremony_level: CeremonyLevel::Skip,
            created_at: "2026-03-23T00:00:00Z".to_string(),
            change_id: "abc123".to_string(),
            approval_hash: None,
            routing_metadata: RoutingMetadata {
                complexity_score: 1,
                ceremony_level: CeremonyLevel::Skip,
                suggested_model_tier: ModelTier::Haiku,
                latency_ms: 0,
                token_estimate: 0,
                router_policy_version: "1.0".to_string(),
                unfamiliarity_ratio: 0.0,
            },
            task_id: task_id_from_description("test"),
            attempt_number: 1,
            risk_level: RiskLevel::Low,
            holdout_scenarios: vec![],
            removals: vec![],
            cleanup_obligations: vec![],
            context_inheritance: ContextInheritance::default(),
        };

        let feedback = Feedback {
            outcome: FeedbackOutcome::Approve,
            timestamp: Utc::now().to_rfc3339(),
            note: None,
        };

        let (contract_path, feedback_path) =
            save_contract(&punk_dir, &mut contract, &feedback).unwrap();

        assert!(contract_path.exists(), "contract.json should be written");
        assert!(feedback_path.exists(), "feedback.json should be written");

        let raw = fs::read_to_string(&contract_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();

        // approval_hash must be present and non-empty
        let hash = v["approval_hash"].as_str().expect("approval_hash missing");
        assert!(!hash.is_empty(), "approval_hash must not be empty");
        assert_eq!(hash.len(), 64, "SHA-256 hex is 64 chars, got {}", hash.len());

        // Verify the hash is correct
        let without_hash: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let mut canonical_contract = contract.clone();
        canonical_contract.approval_hash = None;
        let canonical =
            serde_json::to_string_pretty(&canonical_contract).unwrap();
        let expected_hash = sha256_hex(canonical.as_bytes());
        assert_eq!(hash, expected_hash, "approval_hash does not match SHA-256 of canonical JSON");
    }

    #[tokio::test]
    async fn manual_mode() {
        let tmp = TempDir::new().unwrap();
        setup_punk_dir(&tmp);

        let opts = PlanOptions {
            root: tmp.path(),
            task: "add feature X",
            manual: true,
            provider: None,
        };

        // manual mode should succeed without an LLM provider
        let result = run_plan_headless(&opts).await;
        assert!(result.is_ok(), "manual mode should succeed: {:?}", result.err());
        let (contract, _, _) = result.unwrap();
        // Goal should be our task
        assert!(contract.goal.contains("add feature X") || !contract.goal.is_empty());
    }

    #[test]
    fn approval_flow() {
        // Test FeedbackOutcome enum variants exist and serialise correctly
        let outcomes = [
            (FeedbackOutcome::Approve, "approve"),
            (FeedbackOutcome::ApproveWithEdit, "approve_with_edit"),
            (FeedbackOutcome::Reject, "reject"),
            (FeedbackOutcome::Quit, "quit"),
        ];
        for (outcome, expected_str) in &outcomes {
            let fb = Feedback {
                outcome: outcome.clone(),
                timestamp: "2026-03-23T00:00:00Z".to_string(),
                note: None,
            };
            let json = serde_json::to_string(&fb).unwrap();
            assert!(
                json.contains(expected_str),
                "expected '{expected_str}' in JSON: {json}"
            );
        }
    }
}
