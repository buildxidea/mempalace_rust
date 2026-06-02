//! No-op LLM provider — returns empty strings.
//!
//! Used when no LLM API keys are configured.

use super::provider::{LlmCompletion, LlmError, LlmProvider, LlmUsage};
use async_trait::async_trait;

/// No-op provider that always returns empty completions.
#[derive(Debug, Default)]
pub struct NoopProvider {
    model: String,
}

impl NoopProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_model(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
        }
    }
}

#[async_trait]
impl LlmProvider for NoopProvider {
    fn name(&self) -> &str {
        "noop"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, _system: &str, _user: &str) -> Result<LlmCompletion, LlmError> {
        // Return empty completion without error — this is intentional behavior
        Ok(LlmCompletion {
            text: String::new(),
            model: self.model().to_string(),
            provider: self.name().to_string(),
            usage: Some(LlmUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            }),
        })
    }

    async fn check_available(&self) -> Result<(), String> {
        // Noop is always available
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_noop_returns_empty() {
        let provider = NoopProvider::default();
        let result = provider.complete("system", "user").await.unwrap();
        assert_eq!(result.text, "");
        assert_eq!(result.provider, "noop");
    }

    #[tokio::test]
    async fn test_noop_always_available() {
        let provider = NoopProvider::default();
        assert!(provider.check_available().await.is_ok());
    }
}
