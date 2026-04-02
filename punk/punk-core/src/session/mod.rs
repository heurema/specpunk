//! Phase 15: Session context priming for AI agents.
//! Generate context pack from project state for session start.

use std::path::Path;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPack {
    pub project_intent: String,
    pub conventions_summary: String,
    pub active_contract: Option<ContractSummary>,
    pub recent_events: Vec<String>,
    pub never_touch: Vec<String>,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractSummary {
    pub goal: String,
    pub scope_touch: Vec<String>,
    pub scope_dont_touch: Vec<String>,
    pub risk_level: String,
}

// ---------------------------------------------------------------------------
// Build context pack
// ---------------------------------------------------------------------------

/// Generate a context pack from .punk/ artifacts.
pub fn build_context_pack(root: &Path) -> ContextPack {
    let punk_dir = root.join(".punk");

    let intent = std::fs::read_to_string(punk_dir.join("intent.md")).unwrap_or_default();
    let conventions =
        std::fs::read_to_string(punk_dir.join("conventions.json")).unwrap_or_default();

    // Load never_touch from scan.json
    let never_touch = load_never_touch(&punk_dir);

    // Load active contract summary
    let active_contract = load_active_contract(root);

    // Load recent recall events
    let recent_events: Vec<String> = crate::recall::load_all(root)
        .into_iter()
        .rev()
        .take(5)
        .map(|e| format!("[{:?}] {} — {}", e.kind, e.context, e.why))
        .collect();

    // Rough token estimate (4 chars ≈ 1 token)
    let contract_chars = active_contract
        .as_ref()
        .map(|c| c.goal.len() + c.scope_touch.join(" ").len() + c.scope_dont_touch.join(" ").len())
        .unwrap_or(0);
    let total_chars = intent.len()
        + conventions.len()
        + contract_chars
        + recent_events.iter().map(|e| e.len()).sum::<usize>();
    let token_estimate = total_chars / 4;

    ContextPack {
        project_intent: intent,
        conventions_summary: conventions,
        active_contract,
        recent_events,
        never_touch,
        token_estimate,
    }
}

fn load_never_touch(punk_dir: &Path) -> Vec<String> {
    let scan_path = punk_dir.join("scan.json");
    if let Ok(raw) = std::fs::read_to_string(scan_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            return v["never_touch"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
        }
    }
    Vec::new()
}

fn load_active_contract(root: &Path) -> Option<ContractSummary> {
    let change_id = crate::vcs::detect(root).ok()?.change_id().ok()?;
    let contract_path = root
        .join(".punk")
        .join("contracts")
        .join(&change_id)
        .join("contract.json");
    let raw = std::fs::read_to_string(contract_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;

    Some(ContractSummary {
        goal: v["goal"].as_str().unwrap_or("").to_string(),
        scope_touch: v["scope"]["touch"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        scope_dont_touch: v["scope"]["dont_touch"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        risk_level: v["risk_level"].as_str().unwrap_or("low").to_string(),
    })
}

/// Render context pack for terminal or agent injection.
pub fn render_context_pack(pack: &ContextPack) -> String {
    let mut out = format!(
        "punk session: context pack (~{} tokens)\n\n",
        pack.token_estimate
    );

    if !pack.project_intent.is_empty() {
        out.push_str("## Intent\n");
        // First 5 lines
        for line in pack.project_intent.lines().take(5) {
            out.push_str(&format!("  {line}\n"));
        }
        out.push('\n');
    }

    if let Some(c) = &pack.active_contract {
        out.push_str(&format!("## Active Contract (risk={})\n", c.risk_level));
        out.push_str(&format!("  goal: {}\n", c.goal));
        out.push_str(&format!("  touch: {}\n", c.scope_touch.join(", ")));
        if !c.scope_dont_touch.is_empty() {
            out.push_str(&format!(
                "  dont_touch: {}\n",
                c.scope_dont_touch.join(", ")
            ));
        }
        out.push('\n');
    }

    if !pack.never_touch.is_empty() {
        out.push_str(&format!(
            "## Boundaries: {}\n\n",
            pack.never_touch.join(", ")
        ));
    }

    if !pack.recent_events.is_empty() {
        out.push_str("## Recent Events\n");
        for e in &pack.recent_events {
            out.push_str(&format!("  {e}\n"));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn build_empty_project() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".punk")).unwrap();
        let pack = build_context_pack(tmp.path());
        assert!(pack.project_intent.is_empty());
        assert!(pack.active_contract.is_none());
    }

    #[test]
    fn build_with_intent() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".punk")).unwrap();
        std::fs::write(
            tmp.path().join(".punk/intent.md"),
            "# My Project\nDoes things.",
        )
        .unwrap();
        let pack = build_context_pack(tmp.path());
        assert!(pack.project_intent.contains("My Project"));
        assert!(pack.token_estimate > 0);
    }

    #[test]
    fn render_pack() {
        let pack = ContextPack {
            project_intent: "# Project\nBuilds stuff.".into(),
            conventions_summary: "[]".into(),
            active_contract: Some(ContractSummary {
                goal: "add auth".into(),
                scope_touch: vec!["src/auth.rs".into()],
                scope_dont_touch: vec![],
                risk_level: "high".into(),
            }),
            recent_events: vec!["[AuditFail] auth — 3 findings".into()],
            never_touch: vec![".env".into()],
            token_estimate: 100,
        };
        let out = render_context_pack(&pack);
        assert!(out.contains("~100 tokens"));
        assert!(out.contains("add auth"));
        assert!(out.contains("AuditFail"));
    }

    #[test]
    fn pack_roundtrip() {
        let pack = ContextPack {
            project_intent: "test".into(),
            conventions_summary: "[]".into(),
            active_contract: None,
            recent_events: vec![],
            never_touch: vec!["migrations/".into()],
            token_estimate: 10,
        };
        let json = serde_json::to_string(&pack).unwrap();
        let back: ContextPack = serde_json::from_str(&json).unwrap();
        assert_eq!(back.never_touch, vec!["migrations/"]);
    }
}
