//! Recall: pre-action knowledge retrieval.
//!
//! Auto-capture outcomes (check fails, audit rejects, reverts).
//! Surface relevant prior events before risky actions.
//! Two-file model: .events.jsonl (gitignored) + decisions.jsonl (tracked).

use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Event schema
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub kind: EventKind,
    pub date: String,
    pub paths: Vec<String>,
    pub context: String,
    pub risk_tier: Option<String>,
    pub why: String,
    pub source: EventSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    ScopeViolation,
    AuditFail,
    ContractReject,
    ReceiptFail,
    Revert,
    Invariant,
    Decision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    Auto,
    Human,
    Ci,
    Audit,
    Distill,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Append an event to .punk/.events.jsonl (gitignored).
pub fn append_event(root: &Path, event: &Event) -> Result<(), std::io::Error> {
    let events_path = root.join(".punk").join(".events.jsonl");
    let line = serde_json::to_string(event).map_err(|e| std::io::Error::other(e.to_string()))?;

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&events_path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

/// Append a curated decision to .punk/decisions.jsonl (git-tracked).
pub fn append_decision(root: &Path, event: &Event) -> Result<(), std::io::Error> {
    let path = root.join(".punk").join("decisions.jsonl");
    let line = serde_json::to_string(event).map_err(|e| std::io::Error::other(e.to_string()))?;

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

/// Load all events from both files.
pub fn load_all(root: &Path) -> Vec<Event> {
    let mut events = Vec::new();
    for filename in &[".events.jsonl", "decisions.jsonl"] {
        let path = root.join(".punk").join(filename);
        if let Ok(raw) = std::fs::read_to_string(&path) {
            for line in raw.lines() {
                if let Ok(e) = serde_json::from_str::<Event>(line) {
                    events.push(e);
                }
            }
        }
    }
    events
}

// ---------------------------------------------------------------------------
// Auto-capture helpers
// ---------------------------------------------------------------------------

/// Capture a scope violation event.
pub fn capture_scope_violation(root: &Path, files: &[String], contract_id: &str) {
    let event = Event {
        id: format!(
            "ev-{}",
            &crate::plan::sha256_hex(
                format!("scope-{}-{}", contract_id, Utc::now().timestamp()).as_bytes()
            )[..8]
        ),
        kind: EventKind::ScopeViolation,
        date: Utc::now().to_rfc3339(),
        paths: files.to_vec(),
        context: format!("contract {contract_id}"),
        risk_tier: None,
        why: format!("{} files out of scope", files.len()),
        source: EventSource::Auto,
        replacement: None,
    };
    let _ = append_event(root, &event);
}

/// Capture an audit failure event.
pub fn capture_audit_fail(root: &Path, contract_id: &str, decision: &str, findings_count: usize) {
    let event = Event {
        id: format!(
            "ev-{}",
            &crate::plan::sha256_hex(
                format!("audit-{}-{}", contract_id, Utc::now().timestamp()).as_bytes()
            )[..8]
        ),
        kind: EventKind::AuditFail,
        date: Utc::now().to_rfc3339(),
        paths: vec![],
        context: format!("contract {contract_id}"),
        risk_tier: None,
        why: format!("audit {decision}: {findings_count} findings"),
        source: EventSource::Audit,
        replacement: None,
    };
    let _ = append_event(root, &event);
}

/// Capture a contract rejection event.
pub fn capture_contract_reject(root: &Path, contract_id: &str, reason: &str) {
    let event = Event {
        id: format!(
            "ev-{}",
            &crate::plan::sha256_hex(
                format!("reject-{}-{}", contract_id, Utc::now().timestamp()).as_bytes()
            )[..8]
        ),
        kind: EventKind::ContractReject,
        date: Utc::now().to_rfc3339(),
        paths: vec![],
        context: format!("contract {contract_id}"),
        risk_tier: None,
        why: reason.to_string(),
        source: EventSource::Auto,
        replacement: None,
    };
    let _ = append_event(root, &event);
}

// ---------------------------------------------------------------------------
// Recall (search)
// ---------------------------------------------------------------------------

/// Search events by paths, context keywords, or kind.
pub fn recall(root: &Path, query: &str, limit: usize) -> Vec<Event> {
    let events = load_all(root);
    let query_lower = query.to_lowercase();
    let query_parts: Vec<&str> = query_lower.split_whitespace().collect();

    let mut scored: Vec<(f64, Event)> = events
        .into_iter()
        .filter_map(|e| {
            let score = score_event(&e, &query_parts);
            if score > 0.0 {
                Some((score, e))
            } else {
                None
            }
        })
        .collect();

    // Sort by score descending, then by date descending
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    scored.into_iter().map(|(_, e)| e).collect()
}

fn score_event(event: &Event, query_parts: &[&str]) -> f64 {
    let mut score = 0.0;

    let kind_str = format!("{:?}", event.kind);
    let searchable = format!(
        "{} {} {} {}",
        event.paths.join(" "),
        event.context,
        event.why,
        kind_str,
    )
    .to_lowercase();

    for part in query_parts {
        if searchable.contains(part) {
            score += 1.0;
        }
    }

    // Boost invariants and decisions
    if event.kind == EventKind::Invariant || event.kind == EventKind::Decision {
        score *= 1.5;
    }

    score
}

// ---------------------------------------------------------------------------
// Remember (manual)
// ---------------------------------------------------------------------------

/// Create a human invariant.
pub fn remember(
    root: &Path,
    description: &str,
    reason: Option<&str>,
) -> Result<Event, std::io::Error> {
    let event = Event {
        id: format!(
            "inv-{}",
            &crate::plan::sha256_hex(
                format!("inv-{}-{}", description, Utc::now().timestamp()).as_bytes()
            )[..8]
        ),
        kind: EventKind::Invariant,
        date: Utc::now().to_rfc3339(),
        paths: vec![],
        context: description.to_string(),
        risk_tier: None,
        why: reason.unwrap_or("human rule").to_string(),
        source: EventSource::Human,
        replacement: None,
    };
    append_decision(root, &event)?;
    Ok(event)
}

// ---------------------------------------------------------------------------
// Renderers
// ---------------------------------------------------------------------------

pub fn render_recall(events: &[Event]) -> String {
    if events.is_empty() {
        return String::new(); // silent when nothing found
    }

    let mut out = format!("punk recall: {} relevant events\n\n", events.len());
    for e in events {
        out.push_str(&format!(
            "  [{:?}] {} — {}\n    {}\n",
            e.kind,
            e.date.split('T').next().unwrap_or(&e.date),
            e.context,
            e.why,
        ));
        if !e.paths.is_empty() {
            out.push_str(&format!("    files: {}\n", e.paths.join(", ")));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup(tmp: &TempDir) {
        std::fs::create_dir_all(tmp.path().join(".punk")).unwrap();
    }

    #[test]
    fn append_and_load() {
        let tmp = TempDir::new().unwrap();
        setup(&tmp);

        let event = Event {
            id: "ev-1".into(),
            kind: EventKind::ScopeViolation,
            date: "2026-03-25".into(),
            paths: vec!["src/auth.rs".into()],
            context: "contract abc".into(),
            risk_tier: None,
            why: "out of scope".into(),
            source: EventSource::Auto,
            replacement: None,
        };
        append_event(tmp.path(), &event).unwrap();

        let loaded = load_all(tmp.path());
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "ev-1");
    }

    #[test]
    fn append_decision_tracked() {
        let tmp = TempDir::new().unwrap();
        setup(&tmp);

        let event = Event {
            id: "dec-1".into(),
            kind: EventKind::Decision,
            date: "2026-03-25".into(),
            paths: vec![],
            context: "use thiserror".into(),
            risk_tier: None,
            why: "team convention".into(),
            source: EventSource::Human,
            replacement: None,
        };
        append_decision(tmp.path(), &event).unwrap();

        // Should be in decisions.jsonl (git-tracked)
        let content = std::fs::read_to_string(tmp.path().join(".punk/decisions.jsonl")).unwrap();
        assert!(content.contains("dec-1"));
    }

    #[test]
    fn recall_by_path() {
        let tmp = TempDir::new().unwrap();
        setup(&tmp);

        append_event(
            tmp.path(),
            &Event {
                id: "ev-1".into(),
                kind: EventKind::ScopeViolation,
                date: "2026-03-25".into(),
                paths: vec!["src/auth.rs".into()],
                context: "contract abc".into(),
                risk_tier: None,
                why: "touched auth".into(),
                source: EventSource::Auto,
                replacement: None,
            },
        )
        .unwrap();

        append_event(
            tmp.path(),
            &Event {
                id: "ev-2".into(),
                kind: EventKind::AuditFail,
                date: "2026-03-25".into(),
                paths: vec!["src/billing.rs".into()],
                context: "contract def".into(),
                risk_tier: None,
                why: "billing bug".into(),
                source: EventSource::Audit,
                replacement: None,
            },
        )
        .unwrap();

        let results = recall(tmp.path(), "auth", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "ev-1");
    }

    #[test]
    fn recall_empty_is_silent() {
        let tmp = TempDir::new().unwrap();
        setup(&tmp);
        let results = recall(tmp.path(), "nonexistent", 10);
        assert!(results.is_empty());
        assert!(render_recall(&results).is_empty());
    }

    #[test]
    fn remember_creates_invariant() {
        let tmp = TempDir::new().unwrap();
        setup(&tmp);

        let event = remember(
            tmp.path(),
            "never store tokens in localStorage",
            Some("security"),
        )
        .unwrap();
        assert_eq!(event.kind, EventKind::Invariant);
        assert_eq!(event.source, EventSource::Human);

        let loaded = load_all(tmp.path());
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].context.contains("localStorage"));
    }

    #[test]
    fn capture_helpers() {
        let tmp = TempDir::new().unwrap();
        setup(&tmp);

        capture_scope_violation(tmp.path(), &["a.rs".into()], "c1");
        capture_audit_fail(tmp.path(), "c2", "AUTO_BLOCK", 3);
        capture_contract_reject(tmp.path(), "c3", "user rejected");

        let events = load_all(tmp.path());
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].kind, EventKind::ScopeViolation);
        assert_eq!(events[1].kind, EventKind::AuditFail);
        assert_eq!(events[2].kind, EventKind::ContractReject);
    }

    #[test]
    fn invariant_boost_in_recall() {
        let tmp = TempDir::new().unwrap();
        setup(&tmp);

        append_event(
            tmp.path(),
            &Event {
                id: "ev-1".into(),
                kind: EventKind::ScopeViolation,
                date: "2026-03-25".into(),
                paths: vec!["auth.rs".into()],
                context: "auth issue".into(),
                risk_tier: None,
                why: "scope".into(),
                source: EventSource::Auto,
                replacement: None,
            },
        )
        .unwrap();

        append_decision(
            tmp.path(),
            &Event {
                id: "inv-1".into(),
                kind: EventKind::Invariant,
                date: "2026-03-25".into(),
                paths: vec![],
                context: "auth must use JWT".into(),
                risk_tier: None,
                why: "policy".into(),
                source: EventSource::Human,
                replacement: None,
            },
        )
        .unwrap();

        let results = recall(tmp.path(), "auth", 10);
        assert_eq!(results.len(), 2);
        // Invariant should be first (1.5x boost)
        assert_eq!(results[0].kind, EventKind::Invariant);
    }

    #[test]
    fn event_roundtrip() {
        let event = Event {
            id: "test".into(),
            kind: EventKind::Decision,
            date: "2026-03-25".into(),
            paths: vec!["a.rs".into()],
            context: "ctx".into(),
            risk_tier: Some("T2".into()),
            why: "reason".into(),
            source: EventSource::Ci,
            replacement: Some("use X instead".into()),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, EventKind::Decision);
        assert_eq!(back.replacement, Some("use X instead".into()));
    }

    #[test]
    fn render_output() {
        let events = vec![Event {
            id: "ev-1".into(),
            kind: EventKind::AuditFail,
            date: "2026-03-25T10:00:00Z".into(),
            paths: vec!["src/auth.rs".into()],
            context: "contract abc".into(),
            risk_tier: None,
            why: "3 critical findings".into(),
            source: EventSource::Audit,
            replacement: None,
        }];
        let out = render_recall(&events);
        assert!(out.contains("1 relevant"));
        assert!(out.contains("AuditFail"));
        assert!(out.contains("src/auth.rs"));
    }
}
