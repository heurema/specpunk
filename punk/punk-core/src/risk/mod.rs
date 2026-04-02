//! Risk router: deterministic risk classification and ceremony enforcement.
//!
//! Inspects task description + scope to auto-determine assurance tier and
//! route family. Higher tiers require more ceremony (baseline, holdouts, audit).

use serde::{Deserialize, Serialize};

use crate::plan::contract::{RiskLevel, Scope, RISK_KEYWORDS};

// ---------------------------------------------------------------------------
// Route family — what kind of change is this?
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteFamily {
    Standard,
    Deletion,
    Migration,
    Dependency,
    Infra,
    Api,
    Security,
}

/// Keywords that signal each route family.
const DELETION_KEYWORDS: &[&str] = &[
    "remove",
    "delete",
    "drop",
    "deprecate",
    "cleanup",
    "dead code",
];
const MIGRATION_KEYWORDS: &[&str] = &[
    "migrate",
    "migration",
    "schema",
    "alter table",
    "rename column",
];
const DEPENDENCY_KEYWORDS: &[&str] = &[
    "upgrade",
    "dependency",
    "bump",
    "update crate",
    "npm update",
];
const INFRA_KEYWORDS: &[&str] = &[
    "deploy",
    "kubernetes",
    "docker",
    "ci/cd",
    "pipeline",
    "terraform",
    "ansible",
];
const API_KEYWORDS: &[&str] = &[
    "endpoint", "api", "route", "handler", "graphql", "rest", "grpc",
];

// ---------------------------------------------------------------------------
// Assurance tier — how much ceremony is required?
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AssuranceTier {
    /// Mechanical: scope check only, no LLM, no baseline.
    T0,
    /// Standard: scope check + optional baseline.
    T1,
    /// High: baseline required, holdouts required (2+), single-model review.
    T2,
    /// Critical: baseline + holdouts (5+) + multi-model audit + repair loop.
    T3,
}

// ---------------------------------------------------------------------------
// Ceremony matrix
// ---------------------------------------------------------------------------

/// What's required at each assurance tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyRequirements {
    pub tier: AssuranceTier,
    pub baseline_required: bool,
    pub min_holdouts: usize,
    pub single_review: bool,
    pub multi_model_audit: bool,
    pub repair_loop: bool,
}

pub fn ceremony_for_tier(tier: &AssuranceTier) -> CeremonyRequirements {
    match tier {
        AssuranceTier::T0 => CeremonyRequirements {
            tier: tier.clone(),
            baseline_required: false,
            min_holdouts: 0,
            single_review: false,
            multi_model_audit: false,
            repair_loop: false,
        },
        AssuranceTier::T1 => CeremonyRequirements {
            tier: tier.clone(),
            baseline_required: false,
            min_holdouts: 0,
            single_review: false,
            multi_model_audit: false,
            repair_loop: false,
        },
        AssuranceTier::T2 => CeremonyRequirements {
            tier: tier.clone(),
            baseline_required: true,
            min_holdouts: 2,
            single_review: true,
            multi_model_audit: false,
            repair_loop: false,
        },
        AssuranceTier::T3 => CeremonyRequirements {
            tier: tier.clone(),
            baseline_required: true,
            min_holdouts: 5,
            single_review: true,
            multi_model_audit: true,
            repair_loop: true,
        },
    }
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// Detect route family from task description.
/// Security is detected via RISK_KEYWORDS but only when no more specific
/// family matches first. Security keywords also escalate the risk level
/// independently via classify_risk().
pub fn detect_family(goal: &str) -> RouteFamily {
    let lower = goal.to_lowercase();

    // Check specific families first (most specific wins)
    if DELETION_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return RouteFamily::Deletion;
    }
    if MIGRATION_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return RouteFamily::Migration;
    }
    if INFRA_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return RouteFamily::Infra;
    }
    if API_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return RouteFamily::Api;
    }
    if DEPENDENCY_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return RouteFamily::Dependency;
    }
    // Security: only if no more specific family matched
    if RISK_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return RouteFamily::Security;
    }

    RouteFamily::Standard
}

/// Determine assurance tier from risk level + route family.
pub fn determine_tier(risk: &RiskLevel, family: &RouteFamily) -> AssuranceTier {
    // Family escalation: some families always get higher tier
    let family_min = match family {
        RouteFamily::Security => AssuranceTier::T3,
        RouteFamily::Migration => AssuranceTier::T2,
        RouteFamily::Deletion => AssuranceTier::T2,
        RouteFamily::Infra => AssuranceTier::T2,
        RouteFamily::Api => AssuranceTier::T1,
        RouteFamily::Dependency => AssuranceTier::T1,
        RouteFamily::Standard => AssuranceTier::T0,
    };

    // Risk level mapping
    let risk_tier = match risk {
        RiskLevel::Low => AssuranceTier::T0,
        RiskLevel::Medium => AssuranceTier::T1,
        RiskLevel::High => AssuranceTier::T2,
    };

    // Take the higher of family and risk
    if family_min >= risk_tier {
        family_min
    } else {
        risk_tier
    }
}

/// Full risk assessment: risk level + family + tier + ceremony.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub risk_level: RiskLevel,
    pub family: RouteFamily,
    pub tier: AssuranceTier,
    pub ceremony: CeremonyRequirements,
}

/// Classify a task and return full risk assessment.
pub fn assess(goal: &str, scope: &Scope) -> RiskAssessment {
    let risk_level = crate::plan::contract::classify_risk(goal, scope);
    let family = detect_family(goal);
    let tier = determine_tier(&risk_level, &family);
    let ceremony = ceremony_for_tier(&tier);

    RiskAssessment {
        risk_level,
        family,
        tier,
        ceremony,
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

pub fn render_assessment(a: &RiskAssessment) -> String {
    let mut out = format!(
        "risk: {:?} | family: {:?} | tier: {:?}\n",
        a.risk_level, a.family, a.tier,
    );
    out.push_str(&format!(
        "  baseline: {} | holdouts: {}+ | review: {} | multi-model: {} | repair: {}\n",
        if a.ceremony.baseline_required {
            "required"
        } else {
            "optional"
        },
        a.ceremony.min_holdouts,
        if a.ceremony.single_review {
            "required"
        } else {
            "optional"
        },
        if a.ceremony.multi_model_audit {
            "required"
        } else {
            "no"
        },
        if a.ceremony.repair_loop {
            "required"
        } else {
            "no"
        },
    ));
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(n: usize) -> Scope {
        Scope {
            touch: (0..n).map(|i| format!("f{i}.rs")).collect(),
            dont_touch: vec![],
        }
    }

    #[test]
    fn family_detection() {
        assert_eq!(
            detect_family("fix JWT token validation"),
            RouteFamily::Security
        );
        assert_eq!(
            detect_family("remove old auth module"),
            RouteFamily::Deletion
        );
        assert_eq!(
            detect_family("migrate users table to v2 schema"),
            RouteFamily::Migration
        );
        assert_eq!(
            detect_family("deploy to production kubernetes"),
            RouteFamily::Infra
        );
        assert_eq!(
            detect_family("add REST endpoint for users"),
            RouteFamily::Api
        );
        assert_eq!(
            detect_family("upgrade dependency versions"),
            RouteFamily::Dependency
        );
        assert_eq!(
            detect_family("add logging to service"),
            RouteFamily::Standard
        );
    }

    #[test]
    fn tier_from_family() {
        assert_eq!(
            determine_tier(&RiskLevel::Low, &RouteFamily::Security),
            AssuranceTier::T3
        );
        assert_eq!(
            determine_tier(&RiskLevel::Low, &RouteFamily::Migration),
            AssuranceTier::T2
        );
        assert_eq!(
            determine_tier(&RiskLevel::Low, &RouteFamily::Standard),
            AssuranceTier::T0
        );
    }

    #[test]
    fn tier_from_risk() {
        assert_eq!(
            determine_tier(&RiskLevel::High, &RouteFamily::Standard),
            AssuranceTier::T2
        );
        assert_eq!(
            determine_tier(&RiskLevel::Medium, &RouteFamily::Standard),
            AssuranceTier::T1
        );
        assert_eq!(
            determine_tier(&RiskLevel::Low, &RouteFamily::Standard),
            AssuranceTier::T0
        );
    }

    #[test]
    fn tier_takes_max() {
        // High risk + Security family → T3 (family wins)
        assert_eq!(
            determine_tier(&RiskLevel::High, &RouteFamily::Security),
            AssuranceTier::T3
        );
        // Low risk + Migration → T2 (family escalates)
        assert_eq!(
            determine_tier(&RiskLevel::Low, &RouteFamily::Migration),
            AssuranceTier::T2
        );
    }

    #[test]
    fn ceremony_t0() {
        let c = ceremony_for_tier(&AssuranceTier::T0);
        assert!(!c.baseline_required);
        assert_eq!(c.min_holdouts, 0);
        assert!(!c.multi_model_audit);
    }

    #[test]
    fn ceremony_t3() {
        let c = ceremony_for_tier(&AssuranceTier::T3);
        assert!(c.baseline_required);
        assert_eq!(c.min_holdouts, 5);
        assert!(c.multi_model_audit);
        assert!(c.repair_loop);
    }

    #[test]
    fn full_assessment() {
        let a = assess("fix auth token validation", &scope(3));
        assert_eq!(a.risk_level, RiskLevel::High);
        assert_eq!(a.family, RouteFamily::Security);
        assert_eq!(a.tier, AssuranceTier::T3);
        assert!(a.ceremony.multi_model_audit);
    }

    #[test]
    fn standard_low_risk() {
        let a = assess("add logging to service", &scope(2));
        assert_eq!(a.risk_level, RiskLevel::Low);
        assert_eq!(a.family, RouteFamily::Standard);
        assert_eq!(a.tier, AssuranceTier::T0);
        assert!(!a.ceremony.baseline_required);
    }

    #[test]
    fn medium_api_gets_t1() {
        let a = assess("add REST endpoint for users", &scope(8));
        assert_eq!(a.family, RouteFamily::Api);
        assert_eq!(a.risk_level, RiskLevel::Medium);
        assert_eq!(a.tier, AssuranceTier::T1);
    }

    #[test]
    fn assessment_roundtrip() {
        let a = assess("migrate database schema", &scope(5));
        let json = serde_json::to_string_pretty(&a).unwrap();
        let back: RiskAssessment = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tier, a.tier);
        assert_eq!(back.family, a.family);
    }

    #[test]
    fn render_output() {
        let a = assess("fix auth token", &scope(1));
        let out = render_assessment(&a);
        assert!(out.contains("T3"));
        assert!(out.contains("Security"));
        assert!(out.contains("multi-model: required"));
    }
}
