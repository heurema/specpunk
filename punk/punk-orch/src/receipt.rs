use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Receipt schema v1 — structured output of every completed task.
/// Append-only to receipts/index.jsonl. Source of truth for cost, status, artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub schema_version: u32,
    pub task_id: String,
    pub status: ReceiptStatus,
    pub agent: String,
    pub model: String,
    pub project: String,
    pub category: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_style: Option<CallStyle>,
    pub tokens_used: u64,
    pub cost_usd: f64,
    pub duration_ms: u64,
    pub exit_code: i32,
    pub artifacts: Vec<String>,
    pub errors: Vec<String>,
    pub summary: String,
    pub created_at: DateTime<Utc>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub punk_check_exit: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatus {
    Success,
    Failure,
    Timeout,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallStyle {
    ToolUse,
    FunctionDeclarations,
    PlainText,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_receipt() -> Receipt {
        Receipt {
            schema_version: 1,
            task_id: "test-001".into(),
            status: ReceiptStatus::Success,
            agent: "claude".into(),
            model: "sonnet".into(),
            project: "signum".into(),
            category: "codegen".into(),
            call_style: None,
            tokens_used: 1234,
            cost_usd: 0.05,
            duration_ms: 30000,
            exit_code: 0,
            artifacts: vec![],
            errors: vec![],
            summary: "test receipt".into(),
            created_at: chrono::Utc::now(),
            parent_task_id: None,
            punk_check_exit: None,
        }
    }

    #[test]
    fn serde_roundtrip() {
        let receipt = sample_receipt();
        let json = serde_json::to_string(&receipt).unwrap();
        let parsed: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.task_id, receipt.task_id);
        assert_eq!(parsed.status, receipt.status);
        assert_eq!(parsed.schema_version, 1);
    }

    #[test]
    fn serde_roundtrip_with_optional_fields() {
        let mut receipt = sample_receipt();
        receipt.call_style = Some(CallStyle::ToolUse);
        receipt.parent_task_id = Some("parent-001".into());
        receipt.punk_check_exit = Some(0);
        let json = serde_json::to_string(&receipt).unwrap();
        let parsed: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.call_style, Some(CallStyle::ToolUse));
        assert_eq!(parsed.parent_task_id.as_deref(), Some("parent-001"));
        assert_eq!(parsed.punk_check_exit, Some(0));
    }

    #[test]
    fn deserialize_bash_supervisor_v1_receipt() {
        // Matches the format punk-dispatch.sh writes
        let json = r#"{
            "schema_version": 1,
            "task_id": "signum-20260327-140000",
            "status": "success",
            "agent": "claude",
            "model": "claude",
            "project": "signum",
            "category": "codegen",
            "call_style": null,
            "tokens_used": 0,
            "cost_usd": 0.0,
            "duration_ms": 31000,
            "exit_code": 0,
            "artifacts": [],
            "errors": [],
            "summary": "",
            "created_at": "2026-03-27T10:00:00Z",
            "parent_task_id": null,
            "punk_check_exit": null
        }"#;
        let receipt: Receipt = serde_json::from_str(json).unwrap();
        assert_eq!(receipt.status, ReceiptStatus::Success);
        assert_eq!(receipt.duration_ms, 31000);
        assert!(receipt.call_style.is_none());
    }

    #[test]
    fn deserialize_missing_optional_fields() {
        // Optional fields omitted entirely (not null)
        let json = r#"{
            "schema_version": 1,
            "task_id": "test-002",
            "status": "failure",
            "agent": "codex",
            "model": "gpt-5",
            "project": "mycel",
            "category": "review",
            "tokens_used": 500,
            "cost_usd": 0.01,
            "duration_ms": 5000,
            "exit_code": 1,
            "artifacts": [],
            "errors": ["compile error"],
            "summary": "build failed",
            "created_at": "2026-03-27T12:00:00Z"
        }"#;
        let receipt: Receipt = serde_json::from_str(json).unwrap();
        assert_eq!(receipt.status, ReceiptStatus::Failure);
        assert!(receipt.call_style.is_none());
        assert!(receipt.parent_task_id.is_none());
        assert!(receipt.punk_check_exit.is_none());
    }

    #[test]
    fn status_enum_values() {
        assert_eq!(
            serde_json::to_string(&ReceiptStatus::Success).unwrap(),
            "\"success\""
        );
        assert_eq!(
            serde_json::to_string(&ReceiptStatus::Timeout).unwrap(),
            "\"timeout\""
        );
    }

    #[test]
    fn call_style_enum_values() {
        assert_eq!(
            serde_json::to_string(&CallStyle::ToolUse).unwrap(),
            "\"tool_use\""
        );
        assert_eq!(
            serde_json::to_string(&CallStyle::FunctionDeclarations).unwrap(),
            "\"function_declarations\""
        );
    }

    #[test]
    fn rejects_unknown_status() {
        let json = r#"{"schema_version":1,"task_id":"t","status":"completed","agent":"x","model":"x","project":"x","category":"codegen","tokens_used":0,"cost_usd":0,"duration_ms":0,"exit_code":0,"artifacts":[],"errors":[],"summary":"","created_at":"2026-03-27T10:00:00Z"}"#;
        assert!(serde_json::from_str::<Receipt>(json).is_err());
    }
}
