use crate::plan::contract::Contract;
use crate::plan::quality::QualityReport;

/// Render a one-screen summary of the contract for stdout display.
///
/// Returns a `String` suitable for `println!`.
pub fn render_summary(contract: &Contract, quality: &QualityReport) -> String {
    let mut out = Vec::new();

    out.push("╔══════════════════════════════════════════════════════════╗".to_string());
    out.push(format!("║  punk plan -- contract v{}                               ║", contract.version));
    out.push("╚══════════════════════════════════════════════════════════╝".to_string());
    out.push(String::new());

    out.push(format!("GOAL: {}", contract.goal));
    out.push(String::new());

    out.push("SCOPE — touch:".to_string());
    if contract.scope.touch.is_empty() {
        out.push("  (none specified)".to_string());
    } else {
        for f in &contract.scope.touch {
            out.push(format!("  + {f}"));
        }
    }

    out.push("SCOPE — dont_touch:".to_string());
    if contract.scope.dont_touch.is_empty() {
        out.push("  (none specified)".to_string());
    } else {
        for f in &contract.scope.dont_touch {
            out.push(format!("  - {f}"));
        }
    }
    out.push(String::new());

    out.push(format!(
        "ACCEPTANCE CRITERIA ({}):",
        contract.acceptance_criteria.len()
    ));
    for ac in &contract.acceptance_criteria {
        out.push(format!("  [{}] {}", ac.id, ac.description));
        if let Some(v) = &ac.verify {
            out.push(format!("       verify: {v}"));
        }
    }
    out.push(String::new());

    if !contract.warnings.is_empty() {
        out.push("WARNINGS:".to_string());
        for w in &contract.warnings {
            out.push(format!("  ! {w}"));
        }
        out.push(String::new());
    }

    let quality_label = if quality.is_acceptable() { "PASS" } else { "FAIL" };
    out.push(format!(
        "QUALITY: {quality_label} (score {}/100)",
        quality.score
    ));
    if !quality.errors.is_empty() {
        for e in &quality.errors {
            out.push(format!("  ERROR: {e}"));
        }
    }
    if !quality.warnings.is_empty() {
        for w in &quality.warnings {
            out.push(format!("  WARN:  {w}"));
        }
    }
    out.push(String::new());

    out.push(format!(
        "CEREMONY: {}  |  MODEL: {}  |  COMPLEXITY: {}",
        contract.ceremony_level,
        contract.routing_metadata.suggested_model_tier,
        contract.routing_metadata.complexity_score
    ));
    out.push(format!(
        "TASK_ID: {}  |  ATTEMPT: {}",
        contract.task_id, contract.attempt_number
    ));
    out.push(format!(
        "CHANGE_ID: {}  |  CREATED: {}",
        contract.change_id, contract.created_at
    ));

    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::ceremony::{CeremonyLevel, ModelTier};
    use crate::plan::contract::{AcceptanceCriterion, Contract, RoutingMetadata, Scope};
    use crate::plan::quality::check_quality;

    fn sample_contract() -> Contract {
        Contract {
            version: "1".to_string(),
            goal: "add user auth".to_string(),
            scope: Scope {
                touch: vec!["src/auth.rs".to_string()],
                dont_touch: vec!["migrations/".to_string()],
            },
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "AC-01".to_string(),
                description: "cargo test passes".to_string(),
                verify: Some("cargo test".to_string()),
            }],
            assumptions: vec!["tokio present".to_string()],
            warnings: vec!["scan.json is 100 days old".to_string()],
            ceremony_level: CeremonyLevel::Lightweight,
            created_at: "2026-03-23T00:00:00Z".to_string(),
            change_id: "abc123".to_string(),
            approval_hash: None,
            routing_metadata: RoutingMetadata {
                complexity_score: 3,
                ceremony_level: CeremonyLevel::Lightweight,
                suggested_model_tier: ModelTier::Sonnet,
                latency_ms: 120,
                token_estimate: 512,
                router_policy_version: "1.0".to_string(),
                unfamiliarity_ratio: 0.0,
            },
            task_id: "deadbeef".to_string(),
            attempt_number: 1,
        }
    }

    #[test]
    fn summary_render() {
        let contract = sample_contract();
        let quality = check_quality(
            &contract.acceptance_criteria,
            &contract.scope.touch,
            &contract.scope.dont_touch,
        );
        let rendered = render_summary(&contract, &quality);

        // Must contain key sections
        assert!(rendered.contains("GOAL:"), "missing GOAL section");
        assert!(rendered.contains("add user auth"), "missing goal text");
        assert!(rendered.contains("SCOPE"), "missing SCOPE section");
        assert!(rendered.contains("src/auth.rs"), "missing touch file");
        assert!(rendered.contains("migrations/"), "missing dont_touch file");
        assert!(rendered.contains("ACCEPTANCE CRITERIA"), "missing ACs section");
        assert!(rendered.contains("AC-01"), "missing AC id");
        assert!(rendered.contains("WARNINGS"), "missing WARNINGS section");
        assert!(rendered.contains("QUALITY"), "missing QUALITY section");
        assert!(rendered.contains("CEREMONY"), "missing CEREMONY section");
    }
}
