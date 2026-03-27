use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::receipt::Receipt;

/// Task — a unit of work assigned to an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub project: String,
    pub agent: String,
    pub prompt: String,
    pub category: TaskCategory,
    pub priority: Priority,
    pub risk_tier: RiskTier,
    pub budget_usd: f64,
    pub timeout_s: u64,
    pub state: TaskState,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub worktree: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskCategory {
    Codegen,
    Research,
    Fix,
    Review,
    Content,
    Audit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    P0,
    P1,
    P2,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RiskTier {
    T1,
    T2,
    T3,
}

/// Enum FSM for task lifecycle. Serializable, exhaustive match.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskState {
    Queued {
        enqueued_at: DateTime<Utc>,
    },
    Claimed {
        slot_id: u32,
        claimed_at: DateTime<Utc>,
    },
    Running {
        pid: u32,
        started_at: DateTime<Utc>,
    },
    Done {
        receipt: Box<Receipt>,
    },
    Failed {
        error: String,
        attempts: u8,
    },
}

#[derive(Debug, Clone)]
pub enum TaskEvent {
    Claimed { slot_id: u32 },
    Started { pid: u32 },
    Completed { receipt: Box<Receipt> },
    Failed { error: String },
}

#[derive(Debug, thiserror::Error)]
#[error("invalid transition from {from} on {event}")]
pub struct InvalidTransition {
    pub from: String,
    pub event: String,
}

impl TaskState {
    pub fn transition(self, event: TaskEvent) -> Result<Self, InvalidTransition> {
        match (self, event) {
            (Self::Queued { .. }, TaskEvent::Claimed { slot_id }) => Ok(Self::Claimed {
                slot_id,
                claimed_at: Utc::now(),
            }),
            (Self::Claimed { .. }, TaskEvent::Started { pid }) => Ok(Self::Running {
                pid,
                started_at: Utc::now(),
            }),
            (Self::Running { .. }, TaskEvent::Completed { receipt }) => Ok(Self::Done {
                receipt,
            }),
            (Self::Running { .. }, TaskEvent::Failed { error }) => Ok(Self::Failed {
                error,
                attempts: 1,
            }),
            (state, event) => Err(InvalidTransition {
                from: format!("{state:?}"),
                event: format!("{event:?}"),
            }),
        }
    }
}
