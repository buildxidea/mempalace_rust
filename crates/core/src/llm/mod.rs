//! LLM provider abstraction layer.
//!
//! Provides:
//! - `LlmProvider` trait for async text/image completion
//! - `CircuitBreaker` for provider resilience (3-failure threshold)
//! - `FallbackChain` for automatic provider failover
//! - Concrete providers: OpenAI-compatible, Anthropic, Noop

pub mod anthropic_provider;
pub mod circuit_breaker;
pub mod fallback_chain;
pub mod minimax_provider;
pub mod noop_provider;
pub mod openai_compat_provider;
pub mod provider;

pub use anthropic_provider::{AnthropicConfig, AnthropicProvider};
pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
pub use fallback_chain::FallbackChain;
pub use minimax_provider::{MinimaxConfig, MinimaxProvider};
pub use noop_provider::NoopProvider;
pub use openai_compat_provider::{OpenAICompatConfig, OpenAICompatProvider};
pub use provider::{LlmCompletion, LlmError, LlmProvider, LlmUsage};

use std::sync::Arc;

/// Create an LLM provider from environment variables.
///
/// Priority order:
/// 1. Anthropic (if ANTHROPIC_API_KEY is set)
/// 2. OpenAI-compatible (if OPENAI_API_KEY or OPENAI_BASE_URL is set)
/// 3. Noop (fallback — returns empty completions)
///
/// Returns `Arc<dyn LlmProvider>` for use behind `Arc`.
pub fn create_llm_provider_from_env() -> Arc<dyn LlmProvider> {
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        Arc::new(AnthropicProvider::from_env())
    } else if std::env::var("OPENAI_API_KEY").is_ok() || std::env::var("OPENAI_BASE_URL").is_ok() {
        Arc::new(OpenAICompatProvider::from_env())
    } else {
        Arc::new(NoopProvider::new())
    }
}

/// Create a fallback chain from environment-configured providers.
///
/// Tries Anthropic first, then OpenAI-compatible, then Noop.
pub fn create_fallback_chain_from_env() -> FallbackChain {
    let mut providers: Vec<Arc<dyn LlmProvider>> = Vec::new();

    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        providers.push(Arc::new(AnthropicProvider::from_env()));
    }

    if std::env::var("OPENAI_API_KEY").is_ok() || std::env::var("OPENAI_BASE_URL").is_ok() {
        providers.push(Arc::new(OpenAICompatProvider::from_env()));
    }

    // Always include noop as last resort
    providers.push(Arc::new(NoopProvider::new()));

    FallbackChain::new(providers)
}
