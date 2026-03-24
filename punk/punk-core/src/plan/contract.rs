use serde::{Deserialize, Serialize};

use super::ceremony::{CeremonyLevel, ModelTier};

/// Contract schema version 1.
pub const CONTRACT_VERSION: &str = "1";

/// Routing metadata embedded in each contract for the future learned router.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingMetadata {
    pub complexity_score: u8,
    pub ceremony_level: CeremonyLevel,
    pub suggested_model_tier: ModelTier,
    /// Round-trip latency to the LLM in milliseconds (0 for manual/mock).
    pub latency_ms: u64,
    /// Estimated prompt token count (0 if unknown).
    pub token_estimate: usize,
    /// Monotonically-increasing version of the router policy used.
    pub router_policy_version: String,
    /// Fraction of touched files in unfamiliar territory (0.0–1.0).
    /// OXRL evidence: cheap models match expensive on in-distribution,
    /// collapse on OOD. Used by learned router to select model tier.
    #[serde(default)]
    pub unfamiliarity_ratio: f64,
}

/// One acceptance criterion inside a contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub description: String,
    /// Optional verify command/assertion string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify: Option<String>,
}

/// Scope block — what may and may not be touched.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scope {
    pub touch: Vec<String>,
    pub dont_touch: Vec<String>,
}

/// Top-level contract struct (v1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    /// Schema version — always "1" for this implementation.
    pub version: String,
    /// One-sentence description of what is being built.
    pub goal: String,
    /// File scope.
    pub scope: Scope,
    /// Acceptance criteria (at least one must be verifiable).
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    /// Known facts assumed to be true.
    pub assumptions: Vec<String>,
    /// Warnings emitted by quality or staleness checks.
    pub warnings: Vec<String>,
    /// Ceremony level detected at generation time.
    pub ceremony_level: CeremonyLevel,
    /// ISO-8601 timestamp when the contract was created.
    pub created_at: String,
    /// VCS change/commit identifier at generation time.
    pub change_id: String,
    /// SHA-256 hash of the canonical JSON bytes (set after serialisation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_hash: Option<String>,
    /// Routing metadata for future learned router.
    pub routing_metadata: RoutingMetadata,
    /// Stable SHA-256 of the task description string.
    pub task_id: String,
    /// Incrementing counter — how many contracts share the same task_id.
    pub attempt_number: u32,
}

/// Outcome of the interactive approval flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackOutcome {
    Approve,
    ApproveWithEdit,
    Reject,
    Quit,
}

/// Feedback record saved alongside the contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feedback {
    pub outcome: FeedbackOutcome,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_contract() -> Contract {
        Contract {
            version: CONTRACT_VERSION.to_string(),
            goal: "add user auth".to_string(),
            scope: Scope {
                touch: vec!["src/auth.rs".to_string()],
                dont_touch: vec![],
            },
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "AC-01".to_string(),
                description: "cargo test passes".to_string(),
                verify: Some("cargo test".to_string()),
            }],
            assumptions: vec!["tokio runtime present".to_string()],
            warnings: vec![],
            ceremony_level: CeremonyLevel::Lightweight,
            created_at: "2026-03-23T00:00:00Z".to_string(),
            change_id: "abc123".to_string(),
            approval_hash: None,
            routing_metadata: RoutingMetadata {
                complexity_score: 3,
                ceremony_level: CeremonyLevel::Lightweight,
                suggested_model_tier: ModelTier::Sonnet,
                latency_ms: 0,
                token_estimate: 0,
                router_policy_version: "1.0".to_string(),
                unfamiliarity_ratio: 0.0,
            },
            task_id: "deadbeef".to_string(),
            attempt_number: 1,
        }
    }

    #[test]
    fn contract_schema_valid() {
        let c = minimal_contract();
        // Serialise → deserialise round-trip
        let json = serde_json::to_string_pretty(&c).expect("serialize");
        let back: Contract = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.version, CONTRACT_VERSION);
        assert!(!back.goal.is_empty());
        assert!(!back.acceptance_criteria.is_empty());
        assert!(!back.assumptions.is_empty());
        assert!(!back.created_at.is_empty());
        assert!(!back.change_id.is_empty());
        assert!(!back.scope.touch.is_empty());
        assert!(!back.task_id.is_empty());
        assert_eq!(back.attempt_number, 1);

        // routing_metadata fields present
        let rm = &back.routing_metadata;
        assert!(!rm.router_policy_version.is_empty());
    }

    #[test]
    fn routing_metadata() {
        let c = minimal_contract();
        let json = serde_json::to_string_pretty(&c).expect("serialize");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");

        let rm = &v["routing_metadata"];
        assert!(rm.is_object(), "routing_metadata must be an object");
        assert!(rm.get("complexity_score").is_some());
        assert!(rm.get("ceremony_level").is_some());
        assert!(rm.get("suggested_model_tier").is_some());
        assert!(rm.get("latency_ms").is_some());
        assert!(rm.get("token_estimate").is_some());
        assert!(rm.get("router_policy_version").is_some());

        // Feedback schema check
        let fb = Feedback {
            outcome: FeedbackOutcome::Approve,
            timestamp: "2026-03-23T00:00:00Z".to_string(),
            note: None,
        };
        let fj = serde_json::to_string(&fb).expect("serialize feedback");
        let fv: serde_json::Value = serde_json::from_str(&fj).expect("parse feedback");
        assert_eq!(fv["outcome"], "approve");
        assert!(fv.get("timestamp").is_some());
    }
}
