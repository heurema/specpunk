use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single execution attempt of a Task. Tasks can have multiple Runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub run_id: String,
    pub task_id: String,
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_of: Option<String>,
    pub slot_id: u32,
    pub agent: String,
    pub model: String,
    pub invoke_tier: InvokeTier,
    pub status: RunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_type: Option<ErrorType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub termination_reason: Option<TerminationReason>,
    pub claimed_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub exit_code: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_path: Option<String>,
}

impl Run {
    pub fn new(task_id: &str, attempt: u32, slot_id: u32, agent: &str, model: &str) -> Self {
        let run_id = format!("{task_id}-{attempt}");
        Self {
            run_id,
            task_id: task_id.to_string(),
            attempt,
            retry_of: None,
            slot_id,
            agent: agent.to_string(),
            model: model.to_string(),
            invoke_tier: InvokeTier::Cli,
            status: RunStatus::Claimed,
            error_type: None,
            termination_reason: None,
            claimed_at: Utc::now(),
            started_at: None,
            finished_at: None,
            duration_ms: 0,
            exit_code: 0,
            pid: None,
            stdout_path: None,
            stderr_path: None,
        }
    }

    pub fn mark_started(&mut self, pid: u32) {
        self.status = RunStatus::Running;
        self.started_at = Some(Utc::now());
        self.pid = Some(pid);
    }

    pub fn mark_success(&mut self, duration_ms: u64) {
        self.status = RunStatus::Success;
        self.finished_at = Some(Utc::now());
        self.duration_ms = duration_ms;
        self.exit_code = 0;
    }

    pub fn mark_failed(&mut self, exit_code: i32, duration_ms: u64, reason: TerminationReason) {
        self.status = RunStatus::Failure;
        self.finished_at = Some(Utc::now());
        self.duration_ms = duration_ms;
        self.exit_code = exit_code;
        self.error_type = Some(reason.error_type());
        self.termination_reason = Some(reason);
    }

    pub fn mark_timeout(&mut self, duration_ms: u64) {
        self.mark_failed(124, duration_ms, TerminationReason::Timeout);
        self.status = RunStatus::Timeout;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Claimed,
    Running,
    Success,
    Failure,
    Timeout,
    Killed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InvokeTier {
    Cli,
    OauthApi,
    PaidApi,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    Transient,
    Permanent,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminationReason {
    Provider429,
    Provider529,
    ProviderOverloaded,
    Timeout,
    AdapterCrash,
    DaemonCrashRecovery,
    AuthExpired,
    BudgetExceeded,
    PromptTooLarge,
    AdapterNotFound,
    ProjectNotFound,
    UserCancel,
    ExitNonzero,
    Sigkill,
}

impl TerminationReason {
    pub fn error_type(&self) -> ErrorType {
        match self {
            Self::Provider429
            | Self::Provider529
            | Self::ProviderOverloaded
            | Self::Timeout
            | Self::AdapterCrash
            | Self::DaemonCrashRecovery => ErrorType::Transient,

            Self::AuthExpired
            | Self::BudgetExceeded
            | Self::PromptTooLarge
            | Self::AdapterNotFound
            | Self::ProjectNotFound
            | Self::UserCancel
            | Self::Sigkill => ErrorType::Permanent,

            Self::ExitNonzero => ErrorType::Unknown,
        }
    }

    pub fn is_retryable(&self) -> bool {
        self.error_type() != ErrorType::Permanent
    }
}

/// Classify an exit code + stderr into a TerminationReason.
pub fn classify_failure(exit_code: i32, stderr: &str) -> TerminationReason {
    // Exit 124 = timeout (GNU timeout convention)
    if exit_code == 124 {
        return TerminationReason::Timeout;
    }
    // Exit 137 = SIGKILL (128 + 9)
    if exit_code == 137 {
        return TerminationReason::Sigkill;
    }

    let stderr_lower = stderr.to_lowercase();

    // Auth errors
    if stderr_lower.contains("auth")
        || stderr_lower.contains("401")
        || stderr_lower.contains("403")
        || stderr_lower.contains("credentials")
        || stderr_lower.contains("token expired")
        || stderr_lower.contains("invalid api key")
    {
        return TerminationReason::AuthExpired;
    }

    // Rate limiting
    if stderr_lower.contains("429") || stderr_lower.contains("rate limit") {
        return TerminationReason::Provider429;
    }

    // Server errors
    if stderr_lower.contains("529")
        || stderr_lower.contains("503")
        || stderr_lower.contains("502")
    {
        return TerminationReason::Provider529;
    }

    if stderr_lower.contains("overloaded") {
        return TerminationReason::ProviderOverloaded;
    }

    TerminationReason::ExitNonzero
}

// --- Retry Policy ---

/// Retry decision for a failed run.
#[derive(Debug, PartialEq, Eq)]
pub enum RetryDecision {
    Retry { delay_s: u64 },
    Exhausted,
    NotRetryable,
}

/// Evaluate whether a failed run should be retried.
pub fn should_retry(
    reason: &TerminationReason,
    attempt: u32,
    max_attempts: u32,
    backoff_base_s: u64,
    backoff_multiplier: u64,
    backoff_max_s: u64,
) -> RetryDecision {
    if !reason.is_retryable() {
        return RetryDecision::NotRetryable;
    }

    if attempt >= max_attempts {
        return RetryDecision::Exhausted;
    }

    // Exponential backoff: base * multiplier^(attempt-1), capped at max
    let delay = backoff_base_s * backoff_multiplier.pow(attempt.saturating_sub(1));
    let delay = delay.min(backoff_max_s);

    RetryDecision::Retry { delay_s: delay }
}

// --- Circuit Breaker ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreaker {
    pub provider: String,
    pub state: CircuitState,
    pub consecutive_failures: u32,
    pub failure_threshold: u32,
    pub cooldown_s: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened_at: Option<DateTime<Utc>>,
}

impl CircuitBreaker {
    pub fn new(provider: &str) -> Self {
        Self {
            provider: provider.to_string(),
            state: CircuitState::Closed,
            consecutive_failures: 0,
            failure_threshold: 3,
            cooldown_s: 300,
            opened_at: None,
        }
    }

    /// Check if requests should be allowed through.
    pub fn allows(&self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true, // allow 1 probe
            CircuitState::Open => {
                // Check if cooldown has elapsed
                if let Some(opened) = self.opened_at {
                    let elapsed = Utc::now().signed_duration_since(opened).num_seconds();
                    elapsed >= self.cooldown_s as i64
                } else {
                    true
                }
            }
        }
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.state = CircuitState::Closed;
        self.opened_at = None;
    }

    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= self.failure_threshold {
            self.state = CircuitState::Open;
            self.opened_at = Some(Utc::now());
        }
    }

    /// Transition from Open -> HalfOpen after cooldown.
    pub fn check_cooldown(&mut self) {
        if self.state == CircuitState::Open && self.allows() {
            self.state = CircuitState::HalfOpen;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_lifecycle() {
        let mut run = Run::new("task-001", 1, 2, "claude-coder", "sonnet");
        assert_eq!(run.status, RunStatus::Claimed);

        run.mark_started(12345);
        assert_eq!(run.status, RunStatus::Running);
        assert_eq!(run.pid, Some(12345));

        run.mark_success(5000);
        assert_eq!(run.status, RunStatus::Success);
        assert_eq!(run.duration_ms, 5000);
    }

    #[test]
    fn run_failure_classification() {
        let mut run = Run::new("task-002", 1, 1, "codex-auto", "gpt-5");
        run.mark_started(99);
        run.mark_failed(1, 3000, TerminationReason::Provider429);

        assert_eq!(run.status, RunStatus::Failure);
        assert_eq!(run.error_type, Some(ErrorType::Transient));
        assert_eq!(run.termination_reason, Some(TerminationReason::Provider429));
    }

    #[test]
    fn classify_stderr() {
        assert_eq!(classify_failure(124, ""), TerminationReason::Timeout);
        assert_eq!(classify_failure(137, ""), TerminationReason::Sigkill);
        assert_eq!(
            classify_failure(1, "Error: 429 rate limit exceeded"),
            TerminationReason::Provider429
        );
        assert_eq!(
            classify_failure(1, "auth token expired"),
            TerminationReason::AuthExpired
        );
        assert_eq!(
            classify_failure(1, "server returned 503"),
            TerminationReason::Provider529
        );
        assert_eq!(
            classify_failure(1, "some random error"),
            TerminationReason::ExitNonzero
        );
    }

    #[test]
    fn retry_policy() {
        // Transient, attempt 1 of 3
        assert_eq!(
            should_retry(&TerminationReason::Provider429, 1, 3, 30, 2, 300),
            RetryDecision::Retry { delay_s: 30 }
        );
        // Attempt 2 of 3
        assert_eq!(
            should_retry(&TerminationReason::Provider429, 2, 3, 30, 2, 300),
            RetryDecision::Retry { delay_s: 60 }
        );
        // Attempt 3 of 3 — exhausted
        assert_eq!(
            should_retry(&TerminationReason::Provider429, 3, 3, 30, 2, 300),
            RetryDecision::Exhausted
        );
        // Permanent — not retryable
        assert_eq!(
            should_retry(&TerminationReason::AuthExpired, 1, 3, 30, 2, 300),
            RetryDecision::NotRetryable
        );
        // Backoff capped at max
        assert_eq!(
            should_retry(&TerminationReason::Provider429, 2, 5, 30, 2, 50),
            RetryDecision::Retry { delay_s: 50 }
        );
    }

    #[test]
    fn circuit_breaker_lifecycle() {
        let mut cb = CircuitBreaker::new("claude");
        assert!(cb.allows());

        // 2 failures, still closed
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state, CircuitState::Closed);
        assert!(cb.allows());

        // 3rd failure opens circuit
        cb.record_failure();
        assert_eq!(cb.state, CircuitState::Open);
        assert!(!cb.allows()); // cooldown not elapsed

        // Success resets
        cb.record_success();
        assert_eq!(cb.state, CircuitState::Closed);
        assert_eq!(cb.consecutive_failures, 0);
    }

    #[test]
    fn serde_roundtrip() {
        let run = Run::new("task-001", 1, 0, "claude-coder", "sonnet");
        let json = serde_json::to_string(&run).unwrap();
        let parsed: Run = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.run_id, "task-001-1");
        assert_eq!(parsed.status, RunStatus::Claimed);
    }

    #[test]
    fn termination_reason_retryable() {
        assert!(TerminationReason::Provider429.is_retryable());
        assert!(TerminationReason::Timeout.is_retryable());
        assert!(!TerminationReason::AuthExpired.is_retryable());
        assert!(!TerminationReason::BudgetExceeded.is_retryable());
        assert!(TerminationReason::ExitNonzero.is_retryable()); // unknown = maybe
    }

    // --- Adversarial tests ---

    #[test]
    fn adversarial_classify_failure_empty_stderr() {
        // Empty stderr with non-special exit codes — should return ExitNonzero, not panic
        let result = classify_failure(0, "");
        // exit_code=0 is not 124 or 137, stderr is empty — no pattern matches
        assert_eq!(result, TerminationReason::ExitNonzero);

        let result = classify_failure(1, "");
        assert_eq!(result, TerminationReason::ExitNonzero);

        // Boundary exit codes
        let result = classify_failure(i32::MAX, "");
        assert_eq!(result, TerminationReason::ExitNonzero);

        let result = classify_failure(i32::MIN, "");
        assert_eq!(result, TerminationReason::ExitNonzero);
    }

    #[test]
    fn adversarial_classify_failure_very_long_stderr() {
        // 10K chars of stderr — should not hang, OOM, or panic
        let long_stderr = "error message ".repeat(750); // ~10.5K chars
        assert!(long_stderr.len() >= 10_000);

        let result = classify_failure(1, &long_stderr);
        // No patterns match in random text → ExitNonzero
        assert_eq!(result, TerminationReason::ExitNonzero);
    }

    #[test]
    fn adversarial_classify_failure_stderr_with_all_patterns() {
        // stderr containing multiple matching patterns — first match wins
        // Order: auth > 429 > 529/503/502 > overloaded
        let mixed = "auth token 429 rate limit 529 overloaded";
        let result = classify_failure(1, mixed);
        // "auth" appears first in check order
        assert_eq!(result, TerminationReason::AuthExpired);
    }

    #[test]
    fn adversarial_classify_failure_unicode_stderr() {
        // Unicode in stderr — to_lowercase() on unicode is well-defined, no panic expected
        let unicode_stderr = "Ошибка аутентификации: token expired 认证失败";
        let result = classify_failure(1, unicode_stderr);
        // "token expired" substring is present after lowercase
        assert_eq!(result, TerminationReason::AuthExpired);

        // Pure unicode without any known patterns
        let pure_unicode = "错误: 服务器过载 🔥🚨";
        let result = classify_failure(1, pure_unicode);
        assert_eq!(result, TerminationReason::ExitNonzero);
    }

    #[test]
    fn adversarial_classify_failure_case_insensitive_patterns() {
        // All pattern matching uses to_lowercase(), verify case variants
        assert_eq!(classify_failure(1, "AUTH FAILURE"), TerminationReason::AuthExpired);
        assert_eq!(classify_failure(1, "RATE LIMIT EXCEEDED"), TerminationReason::Provider429);
        assert_eq!(classify_failure(1, "SERVER OVERLOADED"), TerminationReason::ProviderOverloaded);
    }

    #[test]
    fn adversarial_retry_zero_max_attempts() {
        // max_attempts = 0: attempt (1) >= 0 is always true → Exhausted
        // But is_retryable check comes first
        assert_eq!(
            should_retry(&TerminationReason::Provider429, 1, 0, 30, 2, 300),
            RetryDecision::Exhausted
        );
    }

    #[test]
    fn adversarial_retry_overflow_backoff() {
        // Large attempt number could cause overflow in backoff_multiplier.pow(attempt-1)
        // attempt=30, multiplier=2: 2^29 = 536870912, * base=30 = ~16B which overflows u64
        // saturating_sub(1) prevents underflow on attempt=0
        // But the pow could overflow u64...
        let result = std::panic::catch_unwind(|| {
            should_retry(&TerminationReason::Provider429, 30, 100, 30, 2, 300)
        });
        // Should not panic — if overflow occurs, result is still capped by backoff_max_s
        // However u64 overflow in pow wraps or panics in debug mode
        // This test documents the behavior
        match result {
            Ok(decision) => {
                // If no panic: result should be Retry with delay capped at backoff_max_s=300
                assert_eq!(decision, RetryDecision::Retry { delay_s: 300 });
            }
            Err(_) => {
                // Panic due to u64 overflow in debug mode — this is a real bug
                // Leave test failing to document it
                panic!("backoff calculation overflowed u64 with large attempt count");
            }
        }
    }

    #[test]
    fn adversarial_circuit_breaker_no_opened_at() {
        // Open circuit with opened_at=None (shouldn't happen normally, but test robustness)
        let mut cb = CircuitBreaker::new("test");
        cb.state = CircuitState::Open;
        cb.opened_at = None;
        // allows() returns true when opened_at is None (else branch)
        assert!(cb.allows(), "Open circuit with no opened_at should allow (else branch)");
    }

    #[test]
    fn adversarial_circuit_breaker_overflow_consecutive_failures() {
        let mut cb = CircuitBreaker::new("test");
        cb.consecutive_failures = u32::MAX;
        // record_failure does checked add? No — it's just += 1, which wraps in release or panics in debug
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cb.record_failure();
        }));
        // In debug mode, u32 overflow panics. In release, wraps to 0.
        // This test documents the behavior — if it panics, it's a real gap.
        match result {
            Ok(_) => {
                // Wrapped: consecutive_failures became 0, state may be inconsistent
                // No assertion — just documenting that wraparound occurs
            }
            Err(_) => {
                // Expected in debug builds — overflow panic
                // This is a real bug if the circuit breaker can receive u32::MAX failures
            }
        }
    }

    #[test]
    fn adversarial_run_mark_timeout_sets_correct_exit_code() {
        let mut run = Run::new("task-1", 1, 1, "claude", "sonnet");
        run.mark_started(100);
        run.mark_timeout(60_000);

        // mark_timeout calls mark_failed(124, ...) then overrides status to Timeout
        assert_eq!(run.exit_code, 124);
        assert_eq!(run.status, RunStatus::Timeout);
        assert_eq!(run.termination_reason, Some(TerminationReason::Timeout));
        // error_type should be Transient (Timeout is transient)
        assert_eq!(run.error_type, Some(ErrorType::Transient));
    }

    #[test]
    fn adversarial_run_mark_success_after_failure() {
        // State machine: can mark_success be called after mark_failed? No guard.
        let mut run = Run::new("task-1", 1, 1, "claude", "sonnet");
        run.mark_started(100);
        run.mark_failed(1, 1000, TerminationReason::ExitNonzero);
        assert_eq!(run.status, RunStatus::Failure);

        // Call mark_success after failure — no state guard, should overwrite
        run.mark_success(2000);
        assert_eq!(run.status, RunStatus::Success);
        assert_eq!(run.exit_code, 0);
        // error_type remains from mark_failed — stale state
        assert!(run.error_type.is_some(), "error_type not cleared by mark_success");
    }
}
