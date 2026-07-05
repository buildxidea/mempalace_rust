//! Evaluation framework for MemPalace quality tracking.
//!
//! Provides per-function quality metrics, scoring heuristics for
//! compression / summary / context-relevance, and validation with
//! self-correction retry.
//!
//! # Module layout
//!
//! - [`metrics`] — `MetricsStore`: thread-safe per-function score tracking
//! - [`quality`] — scoring heuristics (`score_compression`, `score_summary`, `score_context_relevance`)
//! - [`validator`] — validation + self-correction retry logic
//!
//! # Integration
//!
//! The compress path calls `quality::score_compression` after every
//! compression and feeds the result into `MetricsStore::record`. The
//! summarize path does the same with `quality::score_summary`.
//!
//! The MCP server exposes three tools:
//! - `mempalace_eval_record` — record a quality score
//! - `mempalace_eval_summary` — get summary stats for all functions
//! - `mempalace_eval_check` — check thresholds and return alerts

pub mod metrics;
pub mod quality;
pub mod validator;

use std::sync::{Arc, OnceLock};

pub use metrics::{FunctionStats, MetricsStore, QualityMeasurement};
pub use quality::{score_compression, score_context_relevance, score_summary, SummaryQualityInput};
pub use validator::{
    validate_and_correct_compression, validate_and_correct_summary, ValidationResult,
};

/// Global singleton metrics store, initialized once.
static GLOBAL_METRICS: OnceLock<MetricsStore> = OnceLock::new();

/// Get or initialize the global metrics store.
///
/// Uses `OnceLock` so the store is created exactly once, on first access.
/// Subsequent calls return the same instance.
pub fn global_metrics() -> &'static MetricsStore {
    GLOBAL_METRICS.get_or_init(MetricsStore::new)
}

/// Convenience: record a quality score on the global metrics store.
pub fn record_score(function_name: &str, score: u8, note: Option<String>) {
    global_metrics().record(function_name, score, note);
}

/// Convenience: get summary stats from the global metrics store.
pub fn metrics_summary() -> std::collections::HashMap<String, FunctionStats> {
    global_metrics().summary()
}

/// Convenience: check thresholds on the global metrics store.
pub fn check_thresholds() -> Vec<(String, f64, u8)> {
    global_metrics().check_thresholds()
}

/// Evaluate a compressed observation: score it, record it, and validate it.
///
/// Returns `(score, validation_result)`. The score is also recorded on
/// the global metrics store under the key `"compress"`.
pub fn evaluate_compression(obs: &mut crate::types::CompressedObservation) -> (u8, ValidationResult) {
    let score = score_compression(obs);
    record_score("compress", score, None);
    let validation = validate_and_correct_compression(obs);
    (score, validation)
}

/// Evaluate a session summary: score it, record it, and validate it.
///
/// Returns `(score, validation_result)`. The score is also recorded on
/// the global metrics store under the key `"summarize"`.
pub fn evaluate_summary(
    title: &mut String,
    narrative: &mut String,
    key_decisions: &mut Vec<String>,
    files_modified: &mut Vec<String>,
    concepts: &mut Vec<String>,
) -> (u8, ValidationResult) {
    let input = SummaryQualityInput {
        title,
        narrative,
        key_decisions,
        files_modified,
        concepts,
    };
    let score = score_summary(&input);
    record_score("summarize", score, None);
    let validation = validate_and_correct_summary(title, narrative, key_decisions, files_modified, concepts);
    (score, validation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CompressedObservation, ObservationType};

    #[test]
    fn test_global_metrics_singleton() {
        let a = global_metrics();
        let b = global_metrics();
        // Both should be the same static reference.
        let ptr_a = a as *const MetricsStore;
        let ptr_b = b as *const MetricsStore;
        assert_eq!(ptr_a, ptr_b);
    }

    #[test]
    fn test_record_and_check() {
        let store = MetricsStore::new();
        store.record("test_fn", 80, None);
        assert_eq!(store.latest("test_fn"), Some(80));
    }

    #[test]
    fn test_evaluate_compression() {
        let mut obs = CompressedObservation {
            id: "test".to_string(),
            session_id: "sess".to_string(),
            timestamp: chrono::Utc::now(),
            observation_type: ObservationType::FileRead,
            title: "Read main.rs".to_string(),
            subtitle: None,
            facts: vec!["File has 100 lines".to_string()],
            narrative: "Read the main.rs file which contains the entry point for the application".to_string(),
            concepts: vec!["rust".to_string()],
            files: vec!["src/main.rs".to_string()],
            importance: 7,
            confidence: 0.8,
            image_ref: None,
            image_description: None,
            modality: "text".to_string(),
            agent_id: None,
        };
        let (score, validation) = evaluate_compression(&mut obs);
        assert!(score > 0);
        assert!(validation.valid);
    }

    #[test]
    fn test_evaluate_summary() {
        let mut title = "Auth Implementation".to_string();
        let mut narrative = "Implemented JWT auth with refresh tokens.\n\nAdded middleware.".to_string();
        let mut decisions = vec!["Use JWT".to_string()];
        let mut files = vec!["src/auth.rs".to_string()];
        let mut concepts = vec!["auth".to_string()];
        let (score, validation) = evaluate_summary(
            &mut title,
            &mut narrative,
            &mut decisions,
            &mut files,
            &mut concepts,
        );
        assert!(score > 0);
        assert!(validation.valid);
    }

    #[test]
    fn test_evaluate_compression_with_empty_obs_fixes_it() {
        let mut obs = CompressedObservation {
            id: "test".to_string(),
            session_id: "sess".to_string(),
            timestamp: chrono::Utc::now(),
            observation_type: ObservationType::Other,
            title: String::new(),
            subtitle: None,
            facts: vec![],
            narrative: String::new(),
            concepts: vec![],
            files: vec![],
            importance: 0,
            confidence: 0.0,
            image_ref: None,
            image_description: None,
            modality: "text".to_string(),
            agent_id: None,
        };
        let (_, validation) = evaluate_compression(&mut obs);
        // After self-correction, the obs should be improved.
        assert!(!obs.title.is_empty());
        assert!(!obs.narrative.is_empty());
    }
}
