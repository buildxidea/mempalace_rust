//! Validation and self-correction retry logic for the evaluation framework.
//!
//! Validates compressed observations and session summaries, then applies
//! heuristic fixes when validation fails. Retries up to a configurable limit.

use crate::types::CompressedObservation;

/// Maximum number of self-correction retries.
const MAX_RETRIES: usize = 2;

/// Minimum acceptable quality score (0-100).
const MIN_QUALITY_THRESHOLD: u8 = 30;

/// Result of a validation pass.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether the input passed validation.
    pub valid: bool,
    /// Quality score before any correction (0-100).
    pub initial_score: u8,
    /// Quality score after correction, if applied (0-100).
    pub final_score: u8,
    /// Number of self-correction retries performed.
    pub retries: usize,
    /// Issues found during validation.
    pub issues: Vec<String>,
}

impl ValidationResult {
    pub fn passed(score: u8) -> Self {
        Self {
            valid: true,
            initial_score: score,
            final_score: score,
            retries: 0,
            issues: Vec::new(),
        }
    }

    pub fn failed(initial_score: u8, final_score: u8, retries: usize, issues: Vec<String>) -> Self {
        Self {
            valid: false,
            initial_score,
            final_score,
            retries,
            issues,
        }
    }
}

/// Validate a compressed observation and attempt self-correction.
///
/// Checks:
/// - Title is non-empty and reasonable length
/// - Narrative is non-empty and has minimum length
/// - Importance is in valid range (1-10)
/// - At least one fact or concept is present
///
/// If validation fails, applies heuristic fixes:
/// - Truncates overly long titles
/// - Pads short narratives
/// - Clamps importance to valid range
/// - Retries validation up to MAX_RETRIES times
pub fn validate_and_correct_compression(obs: &mut CompressedObservation) -> ValidationResult {
    let mut issues = Vec::new();
    let mut retries = 0;
    let score_before = super::quality::score_compression(obs);

    // First validation pass.
    issues = validate_compression(obs);

    if issues.is_empty() {
        return ValidationResult::passed(score_before);
    }

    // Self-correction loop.
    for _ in 0..MAX_RETRIES {
        retries += 1;
        apply_compression_fixes(obs);
        issues = validate_compression(obs);
        if issues.is_empty() {
            break;
        }
    }

    let score_after = super::quality::score_compression(obs);

    if score_after >= MIN_QUALITY_THRESHOLD {
        ValidationResult {
            valid: true,
            initial_score: score_before,
            final_score: score_after,
            retries,
            issues,
        }
    } else {
        ValidationResult::failed(score_before, score_after, retries, issues)
    }
}

/// Validate a session summary (title, narrative, etc.) and attempt self-correction.
///
/// Returns the validation result with any issues found.
pub fn validate_and_correct_summary(
    title: &mut String,
    narrative: &mut String,
    key_decisions: &mut Vec<String>,
    files_modified: &mut Vec<String>,
    concepts: &mut Vec<String>,
) -> ValidationResult {
    let input = super::quality::SummaryQualityInput {
        title,
        narrative,
        key_decisions,
        files_modified,
        concepts,
    };
    let score_before = super::quality::score_summary(&input);
    let mut issues = validate_summary(title, narrative, key_decisions, concepts);

    if issues.is_empty() {
        return ValidationResult::passed(score_before);
    }

    // Self-correction loop.
    let mut retries = 0;
    for _ in 0..MAX_RETRIES {
        retries += 1;
        let input = super::quality::SummaryQualityInput {
            title,
            narrative,
            key_decisions,
            files_modified,
            concepts,
        };
        apply_summary_fixes(title, narrative, key_decisions, concepts);
        let input = super::quality::SummaryQualityInput {
            title,
            narrative,
            key_decisions,
            files_modified,
            concepts,
        };
        issues = validate_summary(title, narrative, key_decisions, concepts);
        if issues.is_empty() {
            break;
        }
    }

    let input = super::quality::SummaryQualityInput {
        title,
        narrative,
        key_decisions,
        files_modified,
        concepts,
    };
    let score_after = super::quality::score_summary(&input);

    if score_after >= MIN_QUALITY_THRESHOLD {
        ValidationResult {
            valid: true,
            initial_score: score_before,
            final_score: score_after,
            retries: MAX_RETRIES.min(retries),
            issues,
        }
    } else {
        ValidationResult::failed(score_before, score_after, retries, issues)
    }
}

/// Validate a compressed observation and return a list of issues.
fn validate_compression(obs: &CompressedObservation) -> Vec<String> {
    let mut issues = Vec::new();

    if obs.title.is_empty() {
        issues.push("Title is empty".to_string());
    } else if obs.title.len() > 200 {
        issues.push(format!("Title too long: {} chars (max 200)", obs.title.len()));
    }

    if obs.narrative.is_empty() {
        issues.push("Narrative is empty".to_string());
    } else if obs.narrative.len() < 10 {
        issues.push(format!(
            "Narrative too short: {} chars (min 10)",
            obs.narrative.len()
        ));
    }

    if obs.importance == 0 || obs.importance > 10 {
        issues.push(format!(
            "Importance out of range: {} (expected 1-10)",
            obs.importance
        ));
    }

    if obs.facts.is_empty() && obs.concepts.is_empty() {
        issues.push("No facts or concepts extracted".to_string());
    }

    issues
}

/// Validate a session summary and return a list of issues.
fn validate_summary(
    title: &str,
    narrative: &str,
    key_decisions: &[String],
    concepts: &[String],
) -> Vec<String> {
    let mut issues = Vec::new();

    if title.is_empty() {
        issues.push("Summary title is empty".to_string());
    } else if title.len() > 120 {
        issues.push(format!(
            "Summary title too long: {} chars (max 120)",
            title.len()
        ));
    }

    if narrative.is_empty() {
        issues.push("Summary narrative is empty".to_string());
    } else if narrative.len() < 20 {
        issues.push(format!(
            "Summary narrative too short: {} chars (min 20)",
            narrative.len()
        ));
    }

    if key_decisions.is_empty() && concepts.is_empty() {
        issues.push("No decisions or concepts in summary".to_string());
    }

    issues
}

/// Apply heuristic fixes to a compressed observation.
fn apply_compression_fixes(obs: &mut CompressedObservation) {
    // Truncate overly long title.
    if obs.title.len() > 200 {
        obs.title.truncate(197);
        obs.title.push_str("...");
    }

    // Generate a minimal title if empty.
    if obs.title.is_empty() {
        obs.title = format!("{:?}", obs.observation_type);
        obs.title = obs.title.to_lowercase().replace('_', " ");
    }

    // Pad short narrative.
    if obs.narrative.is_empty() {
        obs.narrative = format!(
            "{:?} operation performed",
            obs.observation_type
        );
    } else if obs.narrative.len() < 10 {
        obs.narrative = format!("{} (minimal observation)", obs.narrative);
    }

    // Clamp importance.
    if obs.importance == 0 {
        obs.importance = 5;
    } else if obs.importance > 10 {
        obs.importance = 10;
    }

    // Add a default concept if both facts and concepts are empty.
    if obs.facts.is_empty() && obs.concepts.is_empty() {
        obs.concepts
            .push(format!("{:?}", obs.observation_type).to_lowercase());
    }
}

/// Apply heuristic fixes to a session summary.
fn apply_summary_fixes(
    title: &mut String,
    narrative: &mut String,
    key_decisions: &mut Vec<String>,
    concepts: &mut Vec<String>,
) {
    // Truncate overly long title.
    if title.len() > 120 {
        title.truncate(117);
        title.push_str("...");
    }

    // Generate minimal title if empty.
    if title.is_empty() {
        *title = "Session Summary".to_string();
    }

    // Pad short narrative.
    if narrative.is_empty() {
        *narrative = "Session completed with operations performed.".to_string();
    } else if narrative.len() < 20 {
        *narrative = format!("{} (minimal summary)", narrative);
    }

    // Add a default concept if both are empty.
    if key_decisions.is_empty() && concepts.is_empty() {
        concepts.push("session".to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ObservationType;

    fn good_obs() -> CompressedObservation {
        CompressedObservation {
            id: "test".to_string(),
            session_id: "sess".to_string(),
            timestamp: chrono::Utc::now(),
            observation_type: ObservationType::FileRead,
            title: "Read main.rs".to_string(),
            subtitle: None,
            facts: vec!["File has 100 lines".to_string()],
            narrative: "Read the main.rs file which contains the entry point".to_string(),
            concepts: vec!["rust".to_string()],
            files: vec!["src/main.rs".to_string()],
            importance: 5,
            confidence: 0.8,
            image_ref: None,
            image_description: None,
            modality: "text".to_string(),
            agent_id: None,
        }
    }

    #[test]
    fn test_validate_good_observation() {
        let mut obs = good_obs();
        let result = validate_and_correct_compression(&mut obs);
        assert!(result.valid);
        assert_eq!(result.retries, 0);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_validate_empty_title_gets_fixed() {
        let mut obs = good_obs();
        obs.title = String::new();
        let result = validate_and_correct_compression(&mut obs);
        assert!(result.valid, "should self-correct: {:?}", result.issues);
        assert!(!obs.title.is_empty());
    }

    #[test]
    fn test_validate_empty_narrative_gets_fixed() {
        let mut obs = good_obs();
        obs.narrative = String::new();
        let result = validate_and_correct_compression(&mut obs);
        assert!(result.valid, "should self-correct: {:?}", result.issues);
        assert!(!obs.narrative.is_empty());
    }

    #[test]
    fn test_validate_importance_clamped() {
        let mut obs = good_obs();
        obs.importance = 25;
        let result = validate_and_correct_compression(&mut obs);
        assert!(result.valid);
        assert!(obs.importance <= 10);
    }

    #[test]
    fn test_validate_importance_zero_gets_fixed() {
        let mut obs = good_obs();
        obs.importance = 0;
        let result = validate_and_correct_compression(&mut obs);
        assert!(result.valid);
        assert!(obs.importance >= 1);
    }

    #[test]
    fn test_validate_empty_facts_and_concepts_gets_fixed() {
        let mut obs = good_obs();
        obs.facts.clear();
        obs.concepts.clear();
        let result = validate_and_correct_compression(&mut obs);
        assert!(result.valid, "should self-correct: {:?}", result.issues);
        assert!(!obs.concepts.is_empty());
    }

    #[test]
    fn test_validate_long_title_truncated() {
        let mut obs = good_obs();
        obs.title = "x".repeat(300);
        let result = validate_and_correct_compression(&mut obs);
        assert!(result.valid);
        assert!(obs.title.len() <= 200);
    }

    #[test]
    fn test_validate_summary_good() {
        let mut title = "Auth Implementation".to_string();
        let mut narrative = "Implemented JWT auth.\n\nAdded middleware.".to_string();
        let mut decisions = vec!["Use JWT".to_string()];
        let mut files = vec!["src/auth.rs".to_string()];
        let mut concepts = vec!["auth".to_string()];
        let result = validate_and_correct_summary(
            &mut title,
            &mut narrative,
            &mut decisions,
            &mut files,
            &mut concepts,
        );
        assert!(result.valid);
        assert_eq!(result.retries, 0);
    }

    #[test]
    fn test_validate_summary_empty_gets_fixed() {
        let mut title = String::new();
        let mut narrative = String::new();
        let mut decisions = Vec::new();
        let mut files = Vec::new();
        let mut concepts = Vec::new();
        let result = validate_and_correct_summary(
            &mut title,
            &mut narrative,
            &mut decisions,
            &mut files,
            &mut concepts,
        );
        assert!(result.valid, "should self-correct: {:?}", result.issues);
        assert!(!title.is_empty());
        assert!(!narrative.is_empty());
    }

    #[test]
    fn test_validate_result_failed_shape() {
        let result = ValidationResult::failed(20, 25, 2, vec!["bad score".to_string()]);
        assert!(!result.valid);
        assert_eq!(result.initial_score, 20);
        assert_eq!(result.final_score, 25);
        assert_eq!(result.retries, 2);
        assert_eq!(result.issues.len(), 1);
    }
}
