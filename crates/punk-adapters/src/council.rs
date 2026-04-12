use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::json;

/// Provider-facing execution contract for future council slot runs.
///
/// This is intentionally narrow: council adapters normalize one slot run at a time and return
/// provider output plus minimal metadata. They do not own rubric semantics, scoring policy,
/// synthesis policy, or final acceptance.
pub trait ProviderAdapter {
    fn provider_name(&self) -> &'static str;
    fn preflight(&self) -> Result<ProviderPreflightReport>;
    fn run_slot(&self, spec: &SlotRunSpec) -> Result<RawSlotResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderPreflightReport {
    pub provider: String,
    pub ready: bool,
    pub model: Option<String>,
    pub warnings: Vec<String>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SlotRunSpec {
    pub contract_id: String,
    pub slot_name: String,
    pub prompt: String,
    pub payload: NormalizedCouncilPayload,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawSlotResult {
    pub provider: String,
    pub slot_name: String,
    pub output_text: String,
    pub finish_reason: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NormalizedCouncilPayload {
    pub contract_id: String,
    pub slot_name: String,
    pub objective: String,
    pub instructions: Vec<String>,
    pub context: Vec<NormalizedCouncilContextItem>,
    pub expected_outputs: Vec<String>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NormalizedCouncilContextItem {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeCouncilMode {
    ValidProposal,
    ValidReview,
    MalformedProposal,
    MalformedReview,
    Timeout,
}

impl FakeCouncilMode {
    fn finish_reason(self) -> Option<&'static str> {
        match self {
            Self::ValidProposal | Self::ValidReview => Some("stop"),
            Self::MalformedProposal | Self::MalformedReview => Some("stop"),
            Self::Timeout => None,
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Self::ValidProposal => "valid_proposal",
            Self::ValidReview => "valid_review",
            Self::MalformedProposal => "malformed_proposal",
            Self::MalformedReview => "malformed_review",
            Self::Timeout => "timeout",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeCouncilAdapter {
    provider_name: &'static str,
    mode: FakeCouncilMode,
    timeout: Duration,
}

impl FakeCouncilAdapter {
    pub fn new(mode: FakeCouncilMode) -> Self {
        Self {
            provider_name: "fake-council",
            mode,
            timeout: Duration::from_secs(30),
        }
    }

    pub fn named(provider_name: &'static str, mode: FakeCouncilMode) -> Self {
        Self {
            provider_name,
            mode,
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn mode(&self) -> FakeCouncilMode {
        self.mode
    }
}

impl ProviderAdapter for FakeCouncilAdapter {
    fn provider_name(&self) -> &'static str {
        self.provider_name
    }

    fn preflight(&self) -> Result<ProviderPreflightReport> {
        let mut metadata = BTreeMap::new();
        metadata.insert("adapter_kind".to_string(), "fake".to_string());
        metadata.insert("scenario".to_string(), self.mode.kind().to_string());
        metadata.insert(
            "timeout_ms".to_string(),
            self.timeout.as_millis().to_string(),
        );

        Ok(ProviderPreflightReport {
            provider: self.provider_name.to_string(),
            ready: true,
            model: Some("deterministic-fixture".to_string()),
            warnings: vec!["fake adapter for protocol tests only".to_string()],
            metadata,
        })
    }

    fn run_slot(&self, spec: &SlotRunSpec) -> Result<RawSlotResult> {
        if self.mode == FakeCouncilMode::Timeout {
            return Err(anyhow!(
                "fake council adapter timeout after {} ms for slot {}",
                self.timeout.as_millis(),
                spec.slot_name
            ));
        }

        let output_text = match self.mode {
            FakeCouncilMode::ValidProposal => fake_proposal_payload(spec),
            FakeCouncilMode::ValidReview => fake_review_payload(spec),
            FakeCouncilMode::MalformedProposal => fake_malformed_proposal_payload(spec),
            FakeCouncilMode::MalformedReview => fake_malformed_review_payload(spec),
            FakeCouncilMode::Timeout => unreachable!("timeout handled above"),
        };

        let mut metadata = BTreeMap::new();
        metadata.insert("adapter_kind".to_string(), "fake".to_string());
        metadata.insert("scenario".to_string(), self.mode.kind().to_string());

        Ok(RawSlotResult {
            provider: self.provider_name.to_string(),
            slot_name: spec.slot_name.clone(),
            output_text,
            finish_reason: self.mode.finish_reason().map(str::to_string),
            metadata,
        })
    }
}

pub fn fake_proposal_payload(spec: &SlotRunSpec) -> String {
    let context = spec
        .payload
        .context
        .iter()
        .map(|item| json!({ "key": item.key, "value": item.value }))
        .collect::<Vec<_>>();

    json!({
        "contract_id": spec.contract_id,
        "slot_name": spec.slot_name,
        "kind": "proposal",
        "proposal": {
            "summary": format!("Deterministic fake proposal for {}", spec.payload.objective),
            "rationale": [
                format!("Objective: {}", spec.payload.objective),
                format!("Prompt size: {}", spec.prompt.len()),
                format!("Instruction count: {}", spec.payload.instructions.len()),
            ],
            "changes": spec.payload.expected_outputs,
            "context": context,
        }
    })
    .to_string()
}

pub fn fake_review_payload(spec: &SlotRunSpec) -> String {
    json!({
        "contract_id": spec.contract_id,
        "slot_name": spec.slot_name,
        "kind": "review",
        "review": {
            "summary": format!("Deterministic fake review for {}", spec.payload.objective),
            "verdict": "approve",
            "issues": [],
            "checks": [
                "format",
                "scope",
                "determinism",
            ],
            "notes": [
                format!("Expected outputs: {}", spec.payload.expected_outputs.join(", ")),
                format!("Metadata keys: {}", spec.payload.metadata.len()),
            ]
        }
    })
    .to_string()
}

pub fn fake_malformed_proposal_payload(spec: &SlotRunSpec) -> String {
    format!(
        "{{\"contract_id\":\"{}\",\"slot_name\":\"{}\",\"kind\":\"proposal\",\"proposal\":{{\"summary\":42}}",
        spec.contract_id, spec.slot_name
    )
}

pub fn fake_malformed_review_payload(spec: &SlotRunSpec) -> String {
    format!(
        "review:{}:{}:{{verdict:approve,issues:[",
        spec.contract_id, spec.slot_name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_spec() -> SlotRunSpec {
        SlotRunSpec {
            contract_id: "ct_test".to_string(),
            slot_name: "proposal".to_string(),
            prompt: "Implement deterministic fake adapters".to_string(),
            payload: NormalizedCouncilPayload {
                contract_id: "ct_test".to_string(),
                slot_name: "proposal".to_string(),
                objective: "Verify council protocol fixtures".to_string(),
                instructions: vec![
                    "Use deterministic fixtures".to_string(),
                    "Keep scenario explicit".to_string(),
                ],
                context: vec![NormalizedCouncilContextItem {
                    key: "repo".to_string(),
                    value: "specpunk".to_string(),
                }],
                expected_outputs: vec![
                    "proposal payload".to_string(),
                    "review payload".to_string(),
                ],
                metadata: BTreeMap::from([("contract_id".to_string(), "ct_test".to_string())]),
            },
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn fake_adapter_returns_valid_proposal_payload() {
        let spec = sample_spec();
        let adapter = FakeCouncilAdapter::new(FakeCouncilMode::ValidProposal);

        let result = adapter.run_slot(&spec).expect("valid proposal result");
        let payload: serde_json::Value =
            serde_json::from_str(&result.output_text).expect("proposal json");

        assert_eq!(payload["kind"], "proposal");
        assert_eq!(payload["contract_id"], "ct_test");
        assert_eq!(result.finish_reason.as_deref(), Some("stop"));
        assert_eq!(
            result.metadata.get("scenario").map(String::as_str),
            Some("valid_proposal")
        );
    }

    #[test]
    fn fake_adapter_returns_valid_review_payload() {
        let mut spec = sample_spec();
        spec.slot_name = "review".to_string();
        spec.payload.slot_name = "review".to_string();
        let adapter = FakeCouncilAdapter::new(FakeCouncilMode::ValidReview);

        let result = adapter.run_slot(&spec).expect("valid review result");
        let payload: serde_json::Value =
            serde_json::from_str(&result.output_text).expect("review json");

        assert_eq!(payload["kind"], "review");
        assert_eq!(payload["review"]["verdict"], "approve");
        assert_eq!(
            result.metadata.get("scenario").map(String::as_str),
            Some("valid_review")
        );
    }

    #[test]
    fn fake_adapter_supports_deterministic_malformed_payloads() {
        let spec = sample_spec();

        let malformed_proposal = FakeCouncilAdapter::new(FakeCouncilMode::MalformedProposal)
            .run_slot(&spec)
            .expect("malformed proposal result");
        let malformed_review = FakeCouncilAdapter::new(FakeCouncilMode::MalformedReview)
            .run_slot(&spec)
            .expect("malformed review result");

        assert!(
            serde_json::from_str::<serde_json::Value>(&malformed_proposal.output_text).is_err()
        );
        assert!(serde_json::from_str::<serde_json::Value>(&malformed_review.output_text).is_err());
        assert_eq!(
            malformed_proposal
                .metadata
                .get("scenario")
                .map(String::as_str),
            Some("malformed_proposal")
        );
        assert_eq!(
            malformed_review
                .metadata
                .get("scenario")
                .map(String::as_str),
            Some("malformed_review")
        );
    }

    #[test]
    fn fake_adapter_supports_deterministic_timeout_errors() {
        let spec = sample_spec();
        let adapter = FakeCouncilAdapter::new(FakeCouncilMode::Timeout)
            .with_timeout(Duration::from_millis(25));

        let error = adapter.run_slot(&spec).expect_err("timeout error");
        let message = error.to_string();

        assert!(message.contains("timeout"));
        assert!(message.contains("25 ms"));
        assert!(message.contains("proposal"));
    }

    #[test]
    fn fake_adapter_preflight_exposes_explicit_scenario() {
        let report = FakeCouncilAdapter::named("fake-review", FakeCouncilMode::ValidReview)
            .with_timeout(Duration::from_millis(5))
            .preflight()
            .expect("preflight report");

        assert_eq!(report.provider, "fake-review");
        assert_eq!(
            report.metadata.get("adapter_kind").map(String::as_str),
            Some("fake")
        );
        assert_eq!(
            report.metadata.get("scenario").map(String::as_str),
            Some("valid_review")
        );
        assert_eq!(
            report.metadata.get("timeout_ms").map(String::as_str),
            Some("5")
        );
    }
}
