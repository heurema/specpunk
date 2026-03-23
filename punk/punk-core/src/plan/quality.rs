use crate::plan::contract::AcceptanceCriterion;

/// Weasel words that make ACs non-verifiable.
const WEASEL_WORDS: &[&str] = &["improve", "better", "optimize", "optimise", "properly"];

/// A report from the spec quality heuristic.
#[derive(Debug, Clone)]
pub struct QualityReport {
    /// 0–100 score. 100 = perfect spec.
    pub score: u8,
    /// Human-readable warnings (non-fatal).
    pub warnings: Vec<String>,
    /// Fatal issues that should block contract acceptance.
    pub errors: Vec<String>,
}

impl QualityReport {
    /// Returns true if the spec passes minimum quality (no errors).
    pub fn is_acceptable(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Run the spec quality heuristic over a set of acceptance criteria.
///
/// Rules:
/// 1. At least one AC must have a concrete verify command/assertion.
/// 2. ACs must not contain weasel words (improve, better, optimize, properly).
/// 3. Scope touch list should not be empty (warns, doesn't error).
pub fn check_quality(
    acs: &[AcceptanceCriterion],
    scope_touch: &[String],
    scope_dont_touch: &[String],
) -> QualityReport {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    let mut deductions: i32 = 0;

    // Rule 1: at least one verifiable AC
    let has_verifiable = acs.iter().any(|ac| {
        ac.verify
            .as_deref()
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    });
    if !has_verifiable {
        errors.push(
            "no AC has a concrete verify command — at least one AC must be verifiable".to_string(),
        );
        deductions += 40;
    }

    // Rule 2: weasel words in AC descriptions
    for ac in acs {
        let desc_lower = ac.description.to_lowercase();
        for word in WEASEL_WORDS {
            if desc_lower.contains(word) {
                errors.push(format!(
                    "{}: description contains weasel word '{}' — replace with a concrete assertion",
                    ac.id, word
                ));
                deductions += 15;
            }
        }
    }

    // Rule 3: scope specificity
    if scope_touch.is_empty() {
        warnings.push(
            "scope.touch is empty — consider listing specific files to keep diffs bounded"
                .to_string(),
        );
        deductions += 10;
    }

    // Warn if dont_touch is empty but touch is non-empty (minor)
    if !scope_touch.is_empty() && scope_dont_touch.is_empty() {
        warnings.push(
            "scope.dont_touch is empty — consider listing files that must not change".to_string(),
        );
        deductions += 5;
    }

    let score = (100i32 - deductions).max(0) as u8;
    QualityReport { score, warnings, errors }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ac(id: &str, desc: &str, verify: Option<&str>) -> AcceptanceCriterion {
        AcceptanceCriterion {
            id: id.to_string(),
            description: desc.to_string(),
            verify: verify.map(|s| s.to_string()),
        }
    }

    #[test]
    fn quality_heuristic() {
        // Good spec
        let good_acs = vec![ac("AC-01", "cargo test passes", Some("cargo test"))];
        let report = check_quality(&good_acs, &["src/lib.rs".to_string()], &[]);
        assert!(report.is_acceptable(), "good spec should pass: {:?}", report.errors);
        assert!(report.score > 50);

        // Weasel word
        let weasel_acs = vec![ac("AC-01", "improve performance", Some("cargo bench"))];
        let report = check_quality(&weasel_acs, &["src/lib.rs".to_string()], &[]);
        assert!(
            !report.is_acceptable(),
            "weasel word should cause error"
        );
        assert!(
            report.errors.iter().any(|e| e.contains("improve")),
            "error should mention 'improve'"
        );

        // No verifiable AC
        let no_verify = vec![ac("AC-01", "the system works correctly", None)];
        let report = check_quality(&no_verify, &["src/lib.rs".to_string()], &[]);
        assert!(!report.is_acceptable(), "missing verify should cause error");
        assert!(
            report.errors.iter().any(|e| e.contains("verifiable")),
            "error should mention verifiable"
        );

        // Multiple weasel words
        let multi = vec![
            ac("AC-01", "better error messages", Some("cargo test")),
            ac("AC-02", "optimize the database queries", Some("cargo bench")),
        ];
        let report = check_quality(&multi, &["src/db.rs".to_string()], &[]);
        assert!(!report.is_acceptable());
        assert!(report.errors.iter().any(|e| e.contains("better")));
        assert!(report.errors.iter().any(|e| e.contains("optim")));

        // "properly" weasel word
        let properly = vec![ac("AC-01", "handle errors properly", Some("cargo test"))];
        let report = check_quality(&properly, &["src/lib.rs".to_string()], &[]);
        assert!(!report.is_acceptable());
        assert!(report.errors.iter().any(|e| e.contains("properly")));
    }
}
