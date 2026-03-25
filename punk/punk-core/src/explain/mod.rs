//! Phase 17: Explain gate — human comprehension requirement.
//! Every change gets a structured explanation artifact.

use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Explanation {
    pub schema_version: String,
    pub contract_id: String,
    pub timestamp: String,
    pub what_changed: String,
    pub why_this_approach: String,
    pub what_can_break: String,
    pub confirmed_by: Option<String>,
    pub confirmed_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Create + confirm
// ---------------------------------------------------------------------------

/// Create an explanation draft.
pub fn create_draft(
    contract_id: &str,
    what: &str,
    why: &str,
    risks: &str,
) -> Explanation {
    Explanation {
        schema_version: "1.0".to_string(),
        contract_id: contract_id.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        what_changed: what.to_string(),
        why_this_approach: why.to_string(),
        what_can_break: risks.to_string(),
        confirmed_by: None,
        confirmed_at: None,
    }
}

/// Confirm an explanation (human approval).
pub fn confirm(explanation: &mut Explanation, by: &str) {
    explanation.confirmed_by = Some(by.to_string());
    explanation.confirmed_at = Some(Utc::now().to_rfc3339());
}

/// Save explanation to contract directory.
pub fn save(explanation: &Explanation, contract_dir: &Path) -> Result<(), std::io::Error> {
    let json = serde_json::to_string_pretty(explanation)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let target = contract_dir.join("explanation.json");
    let mut tmp = tempfile::NamedTempFile::new_in(contract_dir)?;
    std::io::Write::write_all(&mut tmp, json.as_bytes())?;
    tmp.persist(&target).map_err(|e| e.error)?;
    Ok(())
}

/// Load explanation from contract directory.
pub fn load(contract_dir: &Path) -> Option<Explanation> {
    let path = contract_dir.join("explanation.json");
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn render_explanation(e: &Explanation) -> String {
    let mut out = format!("## Explanation: {}\n\n", e.contract_id);
    out.push_str(&format!("**What changed:** {}\n\n", e.what_changed));
    out.push_str(&format!("**Why this approach:** {}\n\n", e.why_this_approach));
    out.push_str(&format!("**What can break:** {}\n\n", e.what_can_break));
    if let Some(by) = &e.confirmed_by {
        out.push_str(&format!("Confirmed by: {by}\n"));
    } else {
        out.push_str("**NOT CONFIRMED** — requires human review.\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn create_and_confirm() {
        let mut e = create_draft("c1", "added auth", "JWT is standard", "token expiry edge case");
        assert!(e.confirmed_by.is_none());
        confirm(&mut e, "alice");
        assert_eq!(e.confirmed_by, Some("alice".to_string()));
        assert!(e.confirmed_at.is_some());
    }

    #[test]
    fn save_and_load() {
        let tmp = TempDir::new().unwrap();
        let e = create_draft("c1", "what", "why", "risks");
        save(&e, tmp.path()).unwrap();

        let loaded = load(tmp.path()).unwrap();
        assert_eq!(loaded.contract_id, "c1");
        assert_eq!(loaded.what_changed, "what");
    }

    #[test]
    fn render_unconfirmed() {
        let e = create_draft("c1", "changes", "reasons", "risks");
        let out = render_explanation(&e);
        assert!(out.contains("NOT CONFIRMED"));
    }

    #[test]
    fn render_confirmed() {
        let mut e = create_draft("c1", "changes", "reasons", "risks");
        confirm(&mut e, "bob");
        let out = render_explanation(&e);
        assert!(out.contains("Confirmed by: bob"));
    }

    #[test]
    fn roundtrip() {
        let e = create_draft("c1", "w", "y", "r");
        let json = serde_json::to_string(&e).unwrap();
        let back: Explanation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.schema_version, "1.0");
    }
}
