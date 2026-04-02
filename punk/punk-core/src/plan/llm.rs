use std::time::Duration;

/// Default LLM request timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Errors from LLM provider calls.
#[derive(Debug)]
pub enum LlmError {
    Timeout,
    Http(String),
    MalformedResponse(String),
    Io(std::io::Error),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::Timeout => write!(f, "LLM request timed out"),
            LlmError::Http(s) => write!(f, "HTTP error: {s}"),
            LlmError::MalformedResponse(s) => write!(f, "malformed LLM response: {s}"),
            LlmError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for LlmError {}

impl From<std::io::Error> for LlmError {
    fn from(e: std::io::Error) -> Self {
        LlmError::Io(e)
    }
}

/// Trait that all LLM providers must implement.
/// Using a trait allows tests to inject a `MockProvider` without real API calls.
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// Generate a response string given a prompt.
    async fn generate(&self, prompt: &str) -> Result<String, LlmError>;
}

// ---------------------------------------------------------------------------
// MockProvider — for tests
// ---------------------------------------------------------------------------

/// A mock LLM provider that returns a pre-configured response.
pub struct MockProvider {
    response: Result<String, LlmError>,
}

impl MockProvider {
    /// Create a mock that always returns a successful response.
    pub fn success(response: impl Into<String>) -> Self {
        MockProvider {
            response: Ok(response.into()),
        }
    }

    /// Create a mock that always times out.
    pub fn timeout() -> Self {
        MockProvider {
            response: Err(LlmError::Timeout),
        }
    }

    /// Create a mock that returns a malformed (non-JSON) response.
    pub fn malformed() -> Self {
        MockProvider {
            response: Err(LlmError::MalformedResponse("not valid JSON".to_string())),
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for MockProvider {
    async fn generate(&self, _prompt: &str) -> Result<String, LlmError> {
        match &self.response {
            Ok(s) => Ok(s.clone()),
            Err(LlmError::Timeout) => Err(LlmError::Timeout),
            Err(LlmError::MalformedResponse(s)) => Err(LlmError::MalformedResponse(s.clone())),
            Err(LlmError::Http(s)) => Err(LlmError::Http(s.clone())),
            Err(LlmError::Io(e)) => Err(LlmError::Io(std::io::Error::new(e.kind(), e.to_string()))),
        }
    }
}

// ---------------------------------------------------------------------------
// HttpProvider — stub (actual API call format is configurable via env/config)
// ---------------------------------------------------------------------------

/// HTTP-based LLM provider. Uses reqwest with rustls-tls.
/// Endpoint and API key are read from environment variables:
///   PUNK_LLM_ENDPOINT — full URL (e.g. https://api.anthropic.com/v1/messages)
///   PUNK_LLM_API_KEY  — bearer token / x-api-key
pub struct HttpProvider {
    endpoint: String,
    api_key: String,
    timeout: Duration,
    client: reqwest::Client,
}

impl HttpProvider {
    /// Create from explicit endpoint and key.
    pub fn new(endpoint: impl Into<String>, api_key: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()
            .unwrap_or_default();
        HttpProvider {
            endpoint: endpoint.into(),
            api_key: api_key.into(),
            timeout: DEFAULT_TIMEOUT,
            client,
        }
    }

    /// Create from environment variables `PUNK_LLM_ENDPOINT` and `PUNK_LLM_API_KEY`.
    pub fn from_env() -> Option<Self> {
        let endpoint = std::env::var("PUNK_LLM_ENDPOINT").ok()?;
        let api_key = std::env::var("PUNK_LLM_API_KEY").ok()?;
        Some(Self::new(endpoint, api_key))
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait::async_trait]
impl LlmProvider for HttpProvider {
    async fn generate(&self, prompt: &str) -> Result<String, LlmError> {
        use std::collections::HashMap;

        // Generic JSON body: {"prompt": "..."}
        // Real providers would customise this per their API shape.
        let mut body: HashMap<&str, serde_json::Value> = HashMap::new();
        body.insert("prompt", serde_json::Value::String(prompt.to_string()));

        let resp = tokio::time::timeout(self.timeout, async {
            self.client
                .post(&self.endpoint)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await
        })
        .await
        .map_err(|_| LlmError::Timeout)?
        .map_err(|e| LlmError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(LlmError::Http(format!("HTTP {}", resp.status())));
        }

        let text = resp
            .text()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;

        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn llm_provider_trait() {
        let provider = MockProvider::success(r#"{"goal":"test"}"#);
        let result = provider.generate("test prompt").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), r#"{"goal":"test"}"#);
    }

    #[tokio::test]
    async fn llm_timeout() {
        let provider = MockProvider::timeout();
        let result = provider.generate("test prompt").await;
        assert!(matches!(result, Err(LlmError::Timeout)));
    }

    #[tokio::test]
    async fn llm_malformed_response() {
        let provider = MockProvider::malformed();
        let result = provider.generate("test prompt").await;
        assert!(matches!(result, Err(LlmError::MalformedResponse(_))));
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.contains("malformed"),
            "error should say malformed: {err_str}"
        );
    }
}
