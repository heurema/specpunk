use std::fs;
use std::path::Path;

/// Extract follow-up actions from agent stdout and queue them as tasks.
pub fn extract_and_queue(
    bus: &Path,
    task_id: &str,
    project: &str,
    stdout_path: &Path,
) -> Vec<String> {
    let content = match fs::read_to_string(stdout_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let items = extract_action_items(&content);
    if items.is_empty() {
        return vec![];
    }

    let mut queued = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let followup_id = format!("{task_id}-followup-{}", i + 1);
        let task_json = serde_json::json!({
            "project": project,
            "model": "claude",
            "prompt": item,
            "category": infer_followup_category(item),
            "timeout_seconds": 600,
            "parent_task_id": task_id,
        });

        let queue_path = bus.join("new/p2").join(format!("{followup_id}.json"));
        if let Ok(data) = serde_json::to_string_pretty(&task_json) {
            if fs::write(&queue_path, data).is_ok() {
                queued.push(followup_id);
            }
        }
    }

    queued
}

/// Extract action items from agent output text.
fn extract_action_items(text: &str) -> Vec<String> {
    let mut items = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();

        // Pattern: "TODO: ..." or "FIXME: ..."
        for prefix in &["TODO:", "FIXME:", "HACK:", "XXX:"] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let item = rest.trim();
                if item.len() > 10 {
                    items.push(item.to_string());
                }
            }
        }

        // Pattern: "- [ ] ..." (unchecked markdown checkbox)
        if let Some(rest) = trimmed.strip_prefix("- [ ]") {
            let item = rest.trim();
            if item.len() > 10 {
                items.push(item.to_string());
            }
        }

        // Pattern: "Follow-up: ..." or "Also need to: ..."
        let lower = trimmed.to_lowercase();
        for prefix in &[
            "follow-up:",
            "follow up:",
            "also need to:",
            "next step:",
            "action item:",
        ] {
            if lower.starts_with(prefix) {
                let item = &trimmed[prefix.len()..].trim();
                if item.len() > 10 {
                    items.push(item.to_string());
                }
            }
        }
    }

    // Also check JSON result field
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(result) = v.get("result").and_then(|r| r.as_str()) {
            let sub_items = extract_action_items(result);
            items.extend(sub_items);
        }
    }

    // Dedup
    items.sort();
    items.dedup();
    // Cap at 5 follow-ups per task
    items.truncate(5);
    items
}

fn infer_followup_category(item: &str) -> &'static str {
    let lower = item.to_lowercase();
    if lower.contains("test") || lower.contains("spec") {
        "codegen"
    } else if lower.contains("doc") || lower.contains("readme") || lower.contains("update api") {
        "content"
    } else if lower.contains("fix") || lower.contains("migration") || lower.contains("patch") {
        "fix"
    } else {
        "codegen"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_todos() {
        let text = "Fixed the auth bug.\nTODO: update the migration for old tokens\nTODO: add tests for refresh flow\nDone.";
        let items = extract_action_items(text);
        assert_eq!(items.len(), 2);
        assert!(items[0].contains("migration") || items[0].contains("tests"));
    }

    #[test]
    fn extract_checkboxes() {
        let text = "## Remaining\n- [x] Fix auth\n- [ ] Update API documentation for new endpoints\n- [ ] Add rate limiting tests";
        let items = extract_action_items(text);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn extract_followup_prefix() {
        let text = "All done.\nFollow-up: need to update the deployment script for the new env vars\nAlso need to: migrate the database schema for v2";
        let items = extract_action_items(text);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn skip_short_items() {
        let text = "TODO: fix\nTODO: this is a real actionable follow-up item";
        let items = extract_action_items(text);
        assert_eq!(items.len(), 1); // "fix" is too short (<10 chars)
    }

    #[test]
    fn cap_at_5() {
        let text = (0..10)
            .map(|i| format!("TODO: follow-up action item number {i} needs to be done"))
            .collect::<Vec<_>>()
            .join("\n");
        let items = extract_action_items(&text);
        assert!(items.len() <= 5);
    }
}
