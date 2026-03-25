//! 4-pass contract self-critique (anti-sycophancy).
//! Deterministic checks + heuristics. No LLM required.
//!
//! Pass 1: Ambiguity detection — vague terms, undefined scope
//! Pass 2: Contradiction check — conflicting ACs, scope overlaps
//! Pass 3: Coverage analysis — goal keywords vs AC coverage
//! Pass 4: Reconstruction test — can goal be inferred from ACs alone?

use serde::{Deserialize, Serialize};

use crate::plan::contract::Contract;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CritiqueSeverity { Block, Warn, Info }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CritiqueFinding {
    pub pass: u8,
    pub pass_name: String,
    pub severity: CritiqueSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CritiqueReport {
    pub findings: Vec<CritiqueFinding>,
    pub readiness: Readiness,
    pub passes_run: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Go,
    GoWithWarnings,
    NeedsRevision,
}

// ---------------------------------------------------------------------------
// Vague/ambiguous terms
// ---------------------------------------------------------------------------

const VAGUE_TERMS: &[&str] = &[
    "improve", "better", "optimize", "enhance", "refactor",
    "clean up", "fix up", "make it work", "handle properly",
    "good enough", "as needed", "if possible", "when appropriate",
    "various", "several", "some", "etc", "and so on",
    "performant", "scalable", "robust", "efficient",
];

const UNDEFINED_SCOPE: &[&str] = &[
    "all files", "everything", "the whole", "entire codebase",
    "wherever needed", "as appropriate", "related files",
];

// ---------------------------------------------------------------------------
// 4-pass critique
// ---------------------------------------------------------------------------

/// Run all 4 critique passes on a contract.
pub fn critique(contract: &Contract) -> CritiqueReport {
    let mut findings = Vec::new();

    // Pass 1: Ambiguity
    findings.extend(pass_ambiguity(contract));

    // Pass 2: Contradiction
    findings.extend(pass_contradiction(contract));

    // Pass 3: Coverage
    findings.extend(pass_coverage(contract));

    // Pass 4: Reconstruction
    findings.extend(pass_reconstruction(contract));

    let has_blocks = findings.iter().any(|f| f.severity == CritiqueSeverity::Block);
    let has_warnings = findings.iter().any(|f| f.severity == CritiqueSeverity::Warn);

    let readiness = if has_blocks {
        Readiness::NeedsRevision
    } else if has_warnings {
        Readiness::GoWithWarnings
    } else {
        Readiness::Go
    };

    CritiqueReport { findings, readiness, passes_run: 4 }
}

/// Pass 1: Ambiguity detection.
fn pass_ambiguity(contract: &Contract) -> Vec<CritiqueFinding> {
    let mut findings = Vec::new();

    // Check goal for vague terms
    let goal_lower = contract.goal.to_lowercase();
    for term in VAGUE_TERMS {
        if goal_lower.contains(term) {
            findings.push(CritiqueFinding {
                pass: 1, pass_name: "ambiguity".into(),
                severity: CritiqueSeverity::Warn,
                message: format!("goal contains vague term '{term}' — be more specific"),
            });
        }
    }

    // Check for undefined scope boundaries
    for term in UNDEFINED_SCOPE {
        if goal_lower.contains(term) {
            findings.push(CritiqueFinding {
                pass: 1, pass_name: "ambiguity".into(),
                severity: CritiqueSeverity::Block,
                message: format!("goal uses undefined scope '{term}' — list specific files"),
            });
        }
    }

    // Check ACs for vague language
    for ac in &contract.acceptance_criteria {
        let desc_lower = ac.description.to_lowercase();
        for term in VAGUE_TERMS {
            if desc_lower.contains(term) {
                findings.push(CritiqueFinding {
                    pass: 1, pass_name: "ambiguity".into(),
                    severity: CritiqueSeverity::Warn,
                    message: format!("AC {} uses vague term '{term}'", ac.id),
                });
                break; // one per AC
            }
        }

        // AC without verify = not testable
        if ac.verify.is_none() && ac.verify_steps.is_empty() {
            findings.push(CritiqueFinding {
                pass: 1, pass_name: "ambiguity".into(),
                severity: CritiqueSeverity::Warn,
                message: format!("AC {} has no verify command — how to confirm it?", ac.id),
            });
        }
    }

    // Empty scope = ambiguous
    if contract.scope.touch.is_empty() {
        findings.push(CritiqueFinding {
            pass: 1, pass_name: "ambiguity".into(),
            severity: CritiqueSeverity::Warn,
            message: "scope.touch is empty — no files declared".into(),
        });
    }

    findings
}

/// Pass 2: Contradiction detection.
fn pass_contradiction(contract: &Contract) -> Vec<CritiqueFinding> {
    let mut findings = Vec::new();

    // Check: file in both touch and dont_touch
    for file in &contract.scope.touch {
        for dt in &contract.scope.dont_touch {
            if file == dt || file.starts_with(dt) || dt.starts_with(file) {
                findings.push(CritiqueFinding {
                    pass: 2, pass_name: "contradiction".into(),
                    severity: CritiqueSeverity::Block,
                    message: format!("'{file}' appears in both touch and dont_touch scope"),
                });
            }
        }
    }

    // Check: assumptions contradict each other (basic keyword matching)
    for (i, a) in contract.assumptions.iter().enumerate() {
        let a_lower = a.to_lowercase();
        for b in contract.assumptions.iter().skip(i + 1) {
            let b_lower = b.to_lowercase();
            // Simple negation detection
            if (a_lower.contains("not ") && b_lower.contains(&a_lower.replace("not ", "")))
                || (b_lower.contains("not ") && a_lower.contains(&b_lower.replace("not ", "")))
            {
                findings.push(CritiqueFinding {
                    pass: 2, pass_name: "contradiction".into(),
                    severity: CritiqueSeverity::Warn,
                    message: format!("assumptions may contradict: '{a}' vs '{b}'"),
                });
            }
        }
    }

    // Check: removals target files in scope.touch
    for removal in &contract.removals {
        if contract.scope.touch.iter().any(|t| t == &removal.path || removal.path.starts_with(t)) {
            findings.push(CritiqueFinding {
                pass: 2, pass_name: "contradiction".into(),
                severity: CritiqueSeverity::Warn,
                message: format!("removal '{}' targets a file in scope.touch", removal.path),
            });
        }
    }

    findings
}

/// Pass 3: Coverage analysis — are all goal keywords covered by ACs?
fn pass_coverage(contract: &Contract) -> Vec<CritiqueFinding> {
    let mut findings = Vec::new();

    // Extract meaningful keywords from goal (skip stopwords)
    let stopwords = ["the", "a", "an", "in", "on", "of", "for", "and", "or", "to",
                     "is", "with", "that", "this", "add", "fix", "update", "create",
                     "new", "from", "by", "at", "as", "be", "it", "do", "make"];

    let goal_keywords: Vec<String> = contract.goal.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !stopwords.contains(w))
        .map(|w| w.to_string())
        .collect();

    if goal_keywords.is_empty() {
        return findings;
    }

    // Check which keywords appear in at least one AC
    let all_ac_text: String = contract.acceptance_criteria.iter()
        .map(|ac| ac.description.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    let uncovered: Vec<&str> = goal_keywords.iter()
        .filter(|kw| !all_ac_text.contains(kw.as_str()))
        .map(|kw| kw.as_str())
        .collect();

    if !uncovered.is_empty() {
        let coverage_ratio = 1.0 - (uncovered.len() as f64 / goal_keywords.len() as f64);
        let severity = if coverage_ratio < 0.5 {
            CritiqueSeverity::Block
        } else {
            CritiqueSeverity::Warn
        };

        findings.push(CritiqueFinding {
            pass: 3, pass_name: "coverage".into(),
            severity,
            message: format!(
                "goal keywords not covered by ACs ({:.0}% coverage): {}",
                coverage_ratio * 100.0,
                uncovered.join(", "),
            ),
        });
    }

    // Too few ACs for the scope
    if contract.acceptance_criteria.len() < 2 && contract.scope.touch.len() > 3 {
        findings.push(CritiqueFinding {
            pass: 3, pass_name: "coverage".into(),
            severity: CritiqueSeverity::Warn,
            message: format!(
                "{} ACs for {} files in scope — likely under-specified",
                contract.acceptance_criteria.len(), contract.scope.touch.len(),
            ),
        });
    }

    findings
}

/// Pass 4: Reconstruction test — can the goal be inferred from ACs?
fn pass_reconstruction(contract: &Contract) -> Vec<CritiqueFinding> {
    let mut findings = Vec::new();

    if contract.acceptance_criteria.is_empty() {
        findings.push(CritiqueFinding {
            pass: 4, pass_name: "reconstruction".into(),
            severity: CritiqueSeverity::Block,
            message: "0 acceptance criteria — goal cannot be reconstructed".into(),
        });
        return findings;
    }

    // Check: do ACs collectively mention enough of the goal?
    let goal_words: Vec<&str> = contract.goal.split_whitespace()
        .filter(|w| w.len() > 3)
        .collect();

    let ac_words: String = contract.acceptance_criteria.iter()
        .map(|ac| ac.description.clone())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

    let reconstructable = goal_words.iter()
        .filter(|w| ac_words.contains(&w.to_lowercase()))
        .count();

    let ratio = if goal_words.is_empty() { 1.0 } else { reconstructable as f64 / goal_words.len() as f64 };

    if ratio < 0.3 {
        findings.push(CritiqueFinding {
            pass: 4, pass_name: "reconstruction".into(),
            severity: CritiqueSeverity::Warn,
            message: format!(
                "ACs cover only {:.0}% of goal vocabulary — someone reading ACs alone cannot infer the task",
                ratio * 100.0,
            ),
        });
    }

    findings
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

pub fn render_critique(report: &CritiqueReport) -> String {
    if report.findings.is_empty() {
        return format!("punk critique: PASS ({} passes, readiness={:?})\n", report.passes_run, report.readiness);
    }

    let mut out = format!(
        "punk critique: {} findings, readiness={:?}\n\n",
        report.findings.len(), report.readiness,
    );

    for f in &report.findings {
        let icon = match f.severity {
            CritiqueSeverity::Block => "BLOCK",
            CritiqueSeverity::Warn => "WARN ",
            CritiqueSeverity::Info => "INFO ",
        };
        out.push_str(&format!("  [{}] pass {}: {} — {}\n", icon, f.pass, f.pass_name, f.message));
    }

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::ceremony::{CeremonyLevel, ModelTier};
    use crate::plan::contract::*;

    fn make_contract(goal: &str, touch: Vec<&str>, acs: Vec<(&str, &str)>) -> Contract {
        Contract {
            version: CONTRACT_VERSION.to_string(),
            goal: goal.to_string(),
            scope: Scope {
                touch: touch.into_iter().map(|s| s.to_string()).collect(),
                dont_touch: vec![],
            },
            acceptance_criteria: acs.into_iter().map(|(id, desc)| AcceptanceCriterion {
                id: id.to_string(), description: desc.to_string(),
                verify: Some("test".into()), verify_steps: vec![],
            }).collect(),
            assumptions: vec![], warnings: vec![],
            ceremony_level: CeremonyLevel::Full,
            created_at: "t".into(), change_id: "c".into(),
            approval_hash: None,
            routing_metadata: RoutingMetadata {
                complexity_score: 5, ceremony_level: CeremonyLevel::Full,
                suggested_model_tier: ModelTier::Sonnet,
                latency_ms: 0, token_estimate: 0,
                router_policy_version: "1.0".into(), unfamiliarity_ratio: 0.0,
            },
            task_id: "tid".into(), attempt_number: 1,
            risk_level: RiskLevel::Low,
            holdout_scenarios: vec![], removals: vec![],
            cleanup_obligations: vec![],
            context_inheritance: ContextInheritance::default(),
        }
    }

    #[test]
    fn clean_contract_passes() {
        let c = make_contract(
            "add JWT authentication to login endpoint",
            vec!["src/auth.rs", "src/middleware.rs"],
            vec![
                ("AC-01", "login endpoint returns JWT authentication token"),
                ("AC-02", "invalid credentials return 401"),
            ],
        );
        let report = critique(&c);
        assert_eq!(report.readiness, Readiness::Go, "findings: {:?}", report.findings);
    }

    #[test]
    fn vague_goal_warned() {
        let c = make_contract(
            "improve the auth system",
            vec!["src/auth.rs"],
            vec![("AC-01", "tests pass")],
        );
        let report = critique(&c);
        assert!(report.findings.iter().any(|f| f.message.contains("improve")));
    }

    #[test]
    fn undefined_scope_blocks() {
        let c = make_contract(
            "fix everything in the entire codebase",
            vec!["src/"],
            vec![("AC-01", "tests pass")],
        );
        let report = critique(&c);
        assert_eq!(report.readiness, Readiness::NeedsRevision);
    }

    #[test]
    fn scope_contradiction_blocks() {
        let mut c = make_contract("add auth", vec!["src/auth.rs"], vec![("AC-01", "auth works")]);
        c.scope.dont_touch = vec!["src/auth.rs".into()];
        let report = critique(&c);
        assert!(report.findings.iter().any(|f| f.pass == 2 && f.message.contains("both touch and dont_touch")));
    }

    #[test]
    fn low_coverage_warned() {
        let c = make_contract(
            "add rate limiting with Redis caching and monitoring dashboard",
            vec!["src/api.rs"],
            vec![("AC-01", "api responds within 100ms")],
        );
        let report = critique(&c);
        assert!(report.findings.iter().any(|f| f.pass == 3));
    }

    #[test]
    fn zero_acs_blocks() {
        let c = make_contract("add auth", vec!["src/auth.rs"], vec![]);
        let report = critique(&c);
        assert_eq!(report.readiness, Readiness::NeedsRevision);
        assert!(report.findings.iter().any(|f| f.pass == 4 && f.message.contains("0 acceptance")));
    }

    #[test]
    fn ac_without_verify_warned() {
        let mut c = make_contract("add auth", vec!["src/auth.rs"], vec![("AC-01", "auth works")]);
        c.acceptance_criteria[0].verify = None;
        let report = critique(&c);
        assert!(report.findings.iter().any(|f| f.message.contains("no verify")));
    }

    #[test]
    fn empty_scope_warned() {
        let c = make_contract("add auth", vec![], vec![("AC-01", "auth works")]);
        let report = critique(&c);
        assert!(report.findings.iter().any(|f| f.message.contains("empty")));
    }

    #[test]
    fn report_roundtrip() {
        let r = CritiqueReport {
            findings: vec![CritiqueFinding {
                pass: 1, pass_name: "ambiguity".into(),
                severity: CritiqueSeverity::Warn,
                message: "test".into(),
            }],
            readiness: Readiness::GoWithWarnings,
            passes_run: 4,
        };
        let j = serde_json::to_string(&r).unwrap();
        let back: CritiqueReport = serde_json::from_str(&j).unwrap();
        assert_eq!(back.readiness, Readiness::GoWithWarnings);
    }

    #[test]
    fn render_output() {
        let r = CritiqueReport {
            findings: vec![CritiqueFinding {
                pass: 1, pass_name: "ambiguity".into(),
                severity: CritiqueSeverity::Block,
                message: "vague goal".into(),
            }],
            readiness: Readiness::NeedsRevision,
            passes_run: 4,
        };
        let out = render_critique(&r);
        assert!(out.contains("BLOCK"));
        assert!(out.contains("NeedsRevision"));
    }
}
