use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Knowledge event — atomic unit of institutional memory.
/// Append-only to state/knowledge/events.jsonl.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEvent {
    pub id: String,
    pub kind: EventKind,
    pub project: String,
    pub context: String,
    #[serde(default)]
    pub paths: Vec<String>,
    pub risk_tier: String,
    pub source: EventSource,
    #[serde(default)]
    pub evidence: String,
    pub why: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applies_if: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_days: Option<u32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Failure,
    Timeout,
    Rejection,
    Override,
    Revert,
    Invariant,
    Lesson,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    Daemon,
    Receipt,
    Human,
    PunkCheck,
    Ci,
}

fn knowledge_dir(bus: &Path) -> PathBuf {
    bus.parent().unwrap_or(bus).join("knowledge")
}

fn events_path(bus: &Path) -> PathBuf {
    knowledge_dir(bus).join("events.jsonl")
}

/// Append a knowledge event.
pub fn capture(bus: &Path, event: KnowledgeEvent) -> std::io::Result<()> {
    let dir = knowledge_dir(bus);
    fs::create_dir_all(&dir)?;
    let path = events_path(bus);
    let line = serde_json::to_string(&event)
        .map_err(std::io::Error::other)?;
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(f, "{line}")
        })
}

/// Auto-capture from a failed task receipt.
pub fn capture_from_failure(
    bus: &Path,
    task_id: &str,
    project: &str,
    reason: &str,
    stderr_excerpt: &str,
    paths: &[String],
) {
    let kind = if reason.contains("Timeout") {
        EventKind::Timeout
    } else {
        EventKind::Failure
    };

    let event = KnowledgeEvent {
        id: format!("evt-{}-{}", task_id, Utc::now().timestamp()),
        kind,
        project: project.to_string(),
        context: format!("Task {task_id} failed: {reason}"),
        paths: paths.to_vec(),
        risk_tier: "T2".to_string(),
        source: EventSource::Daemon,
        evidence: stderr_excerpt.chars().take(500).collect(),
        why: reason.to_string(),
        replacement: None,
        applies_if: Some(format!("Similar task on project {project}")),
        superseded_by: None,
        ttl_days: Some(90),
        created_at: Utc::now(),
    };

    capture(bus, event).ok();
}

/// Recall: find relevant knowledge events for a given query.
/// Simple keyword matching on context + why + paths.
pub fn recall(bus: &Path, query: &str, project: Option<&str>, limit: usize) -> Vec<KnowledgeEvent> {
    let path = events_path(bus);
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let query_lower = query.to_lowercase();
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();

    let mut scored: Vec<(f64, KnowledgeEvent)> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<KnowledgeEvent>(line).ok())
        .filter(|e| {
            // Filter by project if specified
            if let Some(proj) = project {
                if e.project != proj {
                    return false;
                }
            }
            // Filter expired events
            if let Some(ttl) = e.ttl_days {
                let age_days = (Utc::now() - e.created_at).num_days();
                if age_days > ttl as i64 {
                    return false;
                }
            }
            true
        })
        .map(|e| {
            // Score by keyword match
            let searchable = format!(
                "{} {} {} {}",
                e.context, e.why, e.paths.join(" "), e.evidence
            )
            .to_lowercase();

            let score: f64 = query_words
                .iter()
                .filter(|w| searchable.contains(**w))
                .count() as f64
                / query_words.len().max(1) as f64;

            (score, e)
        })
        .filter(|(score, _)| *score > 0.0)
        .collect();

    // Sort by score descending, then by recency
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.1.created_at.cmp(&a.1.created_at))
    });

    scored.into_iter().take(limit).map(|(_, e)| e).collect()
}

/// Format recall results for display or prompt injection.
pub fn format_recall(events: &[KnowledgeEvent]) -> String {
    if events.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str(&format!(
        "## Recall: {} relevant event(s)\n\n",
        events.len()
    ));

    for (i, e) in events.iter().enumerate() {
        out.push_str(&format!(
            "{}. **[{:?}]** {} ({})\n",
            i + 1,
            e.kind,
            e.context,
            e.created_at.format("%Y-%m-%d")
        ));
        if !e.why.is_empty() {
            out.push_str(&format!("   Why: {}\n", e.why));
        }
        if let Some(ref applies) = e.applies_if {
            out.push_str(&format!("   Applies if: {applies}\n"));
        }
        if !e.paths.is_empty() {
            out.push_str(&format!("   Paths: {}\n", e.paths.join(", ")));
        }
        out.push('\n');
    }

    out
}

/// List all events (for punk-run recall --list).
pub fn list_events(bus: &Path, limit: usize) -> Vec<KnowledgeEvent> {
    let path = events_path(bus);
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut events: Vec<KnowledgeEvent> = content
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    events.reverse(); // newest first
    events.truncate(limit);
    events
}

/// Manually add a knowledge event (human override/lesson).
pub fn add_manual(
    bus: &Path,
    project: &str,
    kind: EventKind,
    context: &str,
    why: &str,
) -> std::io::Result<()> {
    let event = KnowledgeEvent {
        id: format!("evt-manual-{}", Utc::now().timestamp()),
        kind,
        project: project.to_string(),
        context: context.to_string(),
        paths: vec![],
        risk_tier: "T2".to_string(),
        source: EventSource::Human,
        evidence: String::new(),
        why: why.to_string(),
        replacement: None,
        applies_if: None,
        superseded_by: None,
        ttl_days: None, // manual events don't expire
        created_at: Utc::now(),
    };

    capture(bus, event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn capture_and_recall() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        capture_from_failure(&bus, "task-1", "signum", "Provider429", "rate limit exceeded", &["src/api.rs".into()]);
        capture_from_failure(&bus, "task-2", "signum", "Timeout", "process killed after 600s", &[]);
        capture_from_failure(&bus, "task-3", "mycel", "AuthExpired", "token expired", &[]);

        // Recall for signum
        let results = recall(&bus, "rate limit api", Some("signum"), 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, EventKind::Failure);

        // Recall without project filter
        let results = recall(&bus, "expired", None, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].project, "mycel");

        // Recall timeout
        let results = recall(&bus, "timeout killed", Some("signum"), 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, EventKind::Timeout);
    }

    #[test]
    fn manual_event() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        add_manual(&bus, "signum", EventKind::Invariant, "Never modify .env files", "Production secrets leaked in PR #42").unwrap();

        let events = list_events(&bus, 10);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::Invariant);
    }

    #[test]
    fn format_output() {
        let events = vec![KnowledgeEvent {
            id: "evt-1".into(),
            kind: EventKind::Failure,
            project: "signum".into(),
            context: "Build failed due to missing dependency".into(),
            paths: vec!["Cargo.toml".into()],
            risk_tier: "T2".into(),
            source: EventSource::Daemon,
            evidence: "error[E0433]: failed to resolve".into(),
            why: "Missing crate in Cargo.toml".into(),
            replacement: None,
            applies_if: Some("Adding new dependencies".into()),
            superseded_by: None,
            ttl_days: Some(90),
            created_at: Utc::now(),
        }];

        let output = format_recall(&events);
        assert!(output.contains("Failure"));
        assert!(output.contains("Missing crate"));
        assert!(output.contains("Cargo.toml"));
    }
}
