use serde::{Deserialize, Serialize};

use super::ceremony::{CeremonyLevel, ModelTier};
use crate::dsl::DslStep;

/// Contract schema version.
pub const CONTRACT_VERSION: &str = "2";

// ---------------------------------------------------------------------------
// Risk level
// ---------------------------------------------------------------------------

/// Deterministic risk classification.
/// Low: <5 files, single language, no security keywords
/// Medium: 5-15 files OR 2+ languages
/// High: >15 files OR security keywords detected
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    #[default]
    Low,
    Medium,
    High,
}

/// Security keywords that auto-escalate to High risk.
pub const RISK_KEYWORDS: &[&str] = &[
    "auth", "token", "secret", "payment", "crypto", "permission", "password",
    "jwt", "oauth", "migration", "schema", "deploy", "credential", "session",
    "certificate", "ssl", "tls",
];

// ---------------------------------------------------------------------------
// Holdout scenarios (blind AC verification)
// ---------------------------------------------------------------------------

/// A holdout scenario — hidden acceptance criteria never shown to the implementer.
/// Verified after implementation via the DSL engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Holdout {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub steps: Vec<DslStep>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_timeout() -> u64 {
    5000
}

// ---------------------------------------------------------------------------
// Removals & cleanup obligations
// ---------------------------------------------------------------------------

/// A file or directory to delete as part of the contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Removal {
    pub id: String,
    pub path: String,
    #[serde(default = "default_removal_type")]
    pub removal_type: String,
    pub reason: String,
    #[serde(default)]
    pub prevent_reintroduction: bool,
}

fn default_removal_type() -> String {
    "file".to_string()
}

/// A cleanup obligation — references that must be removed after deletion.
/// Uses Kubernetes Finalizer semantics: blocking obligations must complete
/// before the contract can be finalized.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupObligation {
    pub id: String,
    pub action: String,
    pub target: String,
    #[serde(default)]
    pub blocking: bool,
    #[serde(default)]
    pub verify: Vec<DslStep>,
}

// ---------------------------------------------------------------------------
// Context inheritance
// ---------------------------------------------------------------------------

/// Tracks upstream project context for staleness detection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextInheritance {
    #[serde(default)]
    pub project_ref: Option<String>,
    #[serde(default)]
    pub project_intent_sha256: Option<String>,
    #[serde(default)]
    pub glossary_sha256: Option<String>,
    #[serde(default = "default_staleness_policy")]
    pub staleness_policy: String,
}

fn default_staleness_policy() -> String {
    "warn".to_string()
}

// ---------------------------------------------------------------------------
// Routing metadata
// ---------------------------------------------------------------------------

/// Routing metadata embedded in each contract for the future learned router.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingMetadata {
    pub complexity_score: u8,
    pub ceremony_level: CeremonyLevel,
    pub suggested_model_tier: ModelTier,
    pub latency_ms: u64,
    pub token_estimate: usize,
    pub router_policy_version: String,
    #[serde(default)]
    pub unfamiliarity_ratio: f64,
}

// ---------------------------------------------------------------------------
// Acceptance criteria
// ---------------------------------------------------------------------------

/// One acceptance criterion inside a contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify: Option<String>,
    /// Typed verify steps (v2). Takes precedence over string verify if present.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verify_steps: Vec<DslStep>,
}

/// Scope block — what may and may not be touched.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scope {
    pub touch: Vec<String>,
    pub dont_touch: Vec<String>,
}

// ---------------------------------------------------------------------------
// Contract (v1 + v2 compatible)
// ---------------------------------------------------------------------------

/// Top-level contract struct. Backward-compatible: v1 fields required,
/// v2 fields optional with serde(default).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    pub version: String,
    pub goal: String,
    pub scope: Scope,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub assumptions: Vec<String>,
    pub warnings: Vec<String>,
    pub ceremony_level: CeremonyLevel,
    pub created_at: String,
    pub change_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_hash: Option<String>,
    pub routing_metadata: RoutingMetadata,
    pub task_id: String,
    pub attempt_number: u32,

    // --- v2 fields (all default, backward-compatible with v1) ---

    /// Risk level: low/medium/high. Default: low.
    #[serde(default)]
    pub risk_level: RiskLevel,

    /// Holdout scenarios — blind ACs never shown to implementer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holdout_scenarios: Vec<Holdout>,

    /// Files/directories to delete.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removals: Vec<Removal>,

    /// Cleanup obligations (K8s Finalizer pattern).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cleanup_obligations: Vec<CleanupObligation>,

    /// Context inheritance for staleness detection.
    #[serde(default)]
    pub context_inheritance: ContextInheritance,
}

// ---------------------------------------------------------------------------
// Feedback
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackOutcome {
    Approve,
    ApproveWithEdit,
    Reject,
    Quit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feedback {
    pub outcome: FeedbackOutcome,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ---------------------------------------------------------------------------
// Risk classification
// ---------------------------------------------------------------------------

/// Classify risk level from contract scope and goal.
pub fn classify_risk(goal: &str, scope: &Scope) -> RiskLevel {
    let goal_lower = goal.to_lowercase();

    // High: security keywords in goal
    if RISK_KEYWORDS.iter().any(|kw| goal_lower.contains(kw)) {
        return RiskLevel::High;
    }

    // High: >15 files in scope
    if scope.touch.len() > 15 {
        return RiskLevel::High;
    }

    // Medium: 5-15 files
    if scope.touch.len() >= 5 {
        return RiskLevel::Medium;
    }

    RiskLevel::Low
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
                verify_steps: vec![],
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
            risk_level: RiskLevel::Low,
            holdout_scenarios: vec![],
            removals: vec![],
            cleanup_obligations: vec![],
            context_inheritance: ContextInheritance::default(),
        }
    }

    #[test]
    fn contract_v2_roundtrip() {
        let c = minimal_contract();
        let json = serde_json::to_string_pretty(&c).expect("serialize");
        let back: Contract = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.version, CONTRACT_VERSION);
        assert_eq!(back.risk_level, RiskLevel::Low);
        assert!(back.holdout_scenarios.is_empty());
        assert!(back.removals.is_empty());
    }

    #[test]
    fn contract_v1_backward_compat() {
        // v1 JSON without v2 fields should still parse
        let v1_json = r#"{
            "version": "1",
            "goal": "test",
            "scope": {"touch": ["src/"], "dont_touch": []},
            "acceptance_criteria": [{"id": "AC-01", "description": "test"}],
            "assumptions": [],
            "warnings": [],
            "ceremony_level": "skip",
            "created_at": "2026-03-25T00:00:00Z",
            "change_id": "abc",
            "routing_metadata": {
                "complexity_score": 1,
                "ceremony_level": "skip",
                "suggested_model_tier": "haiku",
                "latency_ms": 0,
                "token_estimate": 0,
                "router_policy_version": "1.0"
            },
            "task_id": "tid",
            "attempt_number": 1
        }"#;
        let c: Contract = serde_json::from_str(v1_json).expect("v1 should parse as v2");
        assert_eq!(c.version, "1");
        assert_eq!(c.risk_level, RiskLevel::Low);
        assert!(c.holdout_scenarios.is_empty());
    }

    #[test]
    fn contract_with_holdouts() {
        let mut c = minimal_contract();
        c.holdout_scenarios = vec![Holdout {
            id: "HO-1".to_string(),
            description: "API returns 200".to_string(),
            steps: vec![],
            timeout_ms: 5000,
        }];
        let json = serde_json::to_string_pretty(&c).unwrap();
        let back: Contract = serde_json::from_str(&json).unwrap();
        assert_eq!(back.holdout_scenarios.len(), 1);
        assert_eq!(back.holdout_scenarios[0].id, "HO-1");
    }

    #[test]
    fn contract_with_removals() {
        let mut c = minimal_contract();
        c.removals = vec![Removal {
            id: "RM-01".to_string(),
            path: "src/old_module/".to_string(),
            removal_type: "directory".to_string(),
            reason: "replaced by new_module".to_string(),
            prevent_reintroduction: true,
        }];
        c.cleanup_obligations = vec![CleanupObligation {
            id: "CO-01".to_string(),
            action: "remove_references".to_string(),
            target: "src/**/*.rs".to_string(),
            blocking: true,
            verify: vec![],
        }];
        let json = serde_json::to_string_pretty(&c).unwrap();
        let back: Contract = serde_json::from_str(&json).unwrap();
        assert_eq!(back.removals.len(), 1);
        assert!(back.removals[0].prevent_reintroduction);
        assert_eq!(back.cleanup_obligations.len(), 1);
        assert!(back.cleanup_obligations[0].blocking);
    }

    #[test]
    fn risk_classification() {
        let scope_small = Scope { touch: vec!["a.rs".into()], dont_touch: vec![] };
        let scope_medium = Scope { touch: (0..8).map(|i| format!("f{i}.rs")).collect(), dont_touch: vec![] };
        let scope_large = Scope { touch: (0..20).map(|i| format!("f{i}.rs")).collect(), dont_touch: vec![] };

        assert_eq!(classify_risk("add logging", &scope_small), RiskLevel::Low);
        assert_eq!(classify_risk("add logging", &scope_medium), RiskLevel::Medium);
        assert_eq!(classify_risk("add logging", &scope_large), RiskLevel::High);
        assert_eq!(classify_risk("fix auth token validation", &scope_small), RiskLevel::High);
        assert_eq!(classify_risk("add JWT middleware", &scope_small), RiskLevel::High);
    }

    #[test]
    fn routing_metadata_roundtrip() {
        let c = minimal_contract();
        let json = serde_json::to_string_pretty(&c).expect("serialize");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");

        let rm = &v["routing_metadata"];
        assert!(rm.is_object());
        assert!(rm.get("complexity_score").is_some());

        let fb = Feedback {
            outcome: FeedbackOutcome::Approve,
            timestamp: "2026-03-23T00:00:00Z".to_string(),
            note: None,
        };
        let fj = serde_json::to_string(&fb).expect("serialize feedback");
        let fv: serde_json::Value = serde_json::from_str(&fj).expect("parse feedback");
        assert_eq!(fv["outcome"], "approve");
    }
}
