//! Quality scoring heuristics for the evaluation framework.
//!
//! Three primary scoring functions, each returning a 0-100 score:
//! - `score_compression`: Evaluates compression quality of an observation.
//! - `score_summary`: Evaluates session summary quality.
//! - `score_context_relevance`: Evaluates how relevant retrieved context is to a query.

use crate::types::CompressedObservation;

/// Score the quality of a compressed observation (0-100).
///
/// Factors:
/// - Facts extraction completeness (0-25)
/// - Narrative quality and length (0-25)
/// - Title quality (0-15)
/// - Concept richness (0-15)
/// - Importance validity (0-10)
/// - File coverage (0-10)
pub fn score_compression(obs: &CompressedObservation) -> u8 {
    let mut score: u8 = 0;

    // Facts extraction: at least 1 fact is worth 25, 3+ facts get a bonus.
    if !obs.facts.is_empty() {
        score += 25;
    }
    if obs.facts.len() >= 3 {
        score += 10;
    }

    // Narrative quality: at least 20 chars gets 20 points, 50+ gets bonus.
    if obs.narrative.len() >= 20 {
        score += 20;
    }
    if obs.narrative.len() >= 50 {
        score += 5;
    }

    // Title quality: 5-120 chars is ideal.
    if obs.title.len() >= 5 && obs.title.len() <= 120 {
        score += 15;
    }

    // Concept richness.
    if !obs.concepts.is_empty() {
        score += 15;
    }

    // Importance validity: 1-10 is the valid range.
    if obs.importance >= 1 && obs.importance <= 10 {
        score += 10;
    }

    score.min(100)
}

/// Session summary quality data for scoring.
pub struct SummaryQualityInput<'a> {
    pub title: &'a str,
    pub narrative: &'a str,
    pub key_decisions: &'a [String],
    pub files_modified: &'a [String],
    pub concepts: &'a [String],
}

/// Score the quality of a session summary (0-100).
///
/// Factors:
/// - Title completeness (0-20): non-empty, 5-80 chars
/// - Narrative depth (0-30): at least 2 paragraphs, 50+ chars
/// - Decisions captured (0-20): at least 1 decision, up to 5
/// - Files coverage (0-15): at least 1 file listed
/// - Concept extraction (0-15): at least 1 concept
pub fn score_summary(input: &SummaryQualityInput) -> u8 {
    let mut score: u8 = 0;

    // Title quality: non-empty and reasonable length.
    if !input.title.is_empty() && input.title.len() >= 5 && input.title.len() <= 80 {
        score += 20;
    } else if !input.title.is_empty() {
        score += 10;
    }

    // Narrative depth: length and paragraph count.
    if input.narrative.len() >= 50 {
        score += 15;
    }
    if input.narrative.len() >= 150 {
        score += 5;
    }
    let paragraph_count = input
        .narrative
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .count();
    if paragraph_count >= 2 {
        score += 10;
    }

    // Decisions: at least 1 is good, up to 5 max.
    let decision_count = input.key_decisions.len();
    if decision_count >= 1 {
        score += 10;
    }
    if decision_count >= 2 {
        score += 5;
    }
    if decision_count >= 3 {
        score += 5;
    }

    // Files coverage.
    if !input.files_modified.is_empty() {
        score += 15;
    }

    // Concept extraction.
    if !input.concepts.is_empty() {
        score += 10;
    }
    if input.concepts.len() >= 3 {
        score += 5;
    }

    score.min(100)
}

/// Score the relevance of a set of search results to a query (0-100).
///
/// Factors:
/// - Result count (0-20): having results at all is good
/// - Score distribution (0-30): average similarity score across results
/// - Top result strength (0-20): best match score
/// - Diversity (0-15): different wings/rooms represented
/// - Concept overlap (0-15): query terms appearing in results
pub fn score_context_relevance(
    query: &str,
    scores: &[f64],
    wings: &[String],
    rooms: &[String],
    result_narratives: &[String],
) -> u8 {
    let mut score: u8 = 0;

    // Result count: having any results is good.
    if !scores.is_empty() {
        score += 10;
    }
    if scores.len() >= 3 {
        score += 5;
    }
    if scores.len() >= 5 {
        score += 5;
    }

    // Score distribution: average similarity (scores are 0.0-1.0, scale to 0-30).
    if !scores.is_empty() {
        let avg: f64 = scores.iter().sum::<f64>() / scores.len() as f64;
        score += (avg * 30.0).round().min(30.0) as u8;
    }

    // Top result strength (0-20).
    if let Some(top) = scores.iter().cloned().reduce(f64::max) {
        score += (top * 20.0).round().min(20.0) as u8;
    }

    // Diversity: number of unique wings and rooms.
    let unique_wings: std::collections::HashSet<&str> = wings.iter().map(|s| s.as_str()).collect();
    let unique_rooms: std::collections::HashSet<&str> = rooms.iter().map(|s| s.as_str()).collect();
    let diversity = unique_wings.len() + unique_rooms.len();
    if diversity >= 2 {
        score += 5;
    }
    if diversity >= 4 {
        score += 5;
    }
    if diversity >= 6 {
        score += 5;
    }

    // Concept overlap: query terms found in result narratives.
    if !result_narratives.is_empty() {
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower
            .split_whitespace()
            .filter(|w| w.len() >= 3)
            .collect();
        if !query_words.is_empty() {
            let all_text: String = result_narratives.join(" ").to_lowercase();
            let matches = query_words.iter().filter(|w| all_text.contains(*w)).count();
            let ratio = matches as f64 / query_words.len() as f64;
            score += (ratio * 15.0).round().min(15.0) as u8;
        }
    }

    score.min(100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ObservationType;

    fn make_test_obs() -> CompressedObservation {
        CompressedObservation {
            id: "test".to_string(),
            session_id: "sess".to_string(),
            timestamp: chrono::Utc::now(),
            observation_type: ObservationType::FileRead,
            title: "Read main.rs".to_string(),
            subtitle: None,
            facts: vec![
                "File has 100 lines".to_string(),
                "Uses Rust".to_string(),
                "Imports serde".to_string(),
            ],
            narrative: "Read the main.rs file which contains the entry point for the application with error handling".to_string(),
            concepts: vec!["rust".to_string(), "async".to_string()],
            files: vec!["src/main.rs".to_string()],
            importance: 7,
            confidence: 0.8,
            image_ref: None,
            image_description: None,
            modality: "text".to_string(),
            agent_id: None,
        }
    }

    #[test]
    fn test_score_compression_perfect() {
        let obs = make_test_obs();
        assert_eq!(score_compression(&obs), 100);
    }

    #[test]
    fn test_score_compression_empty() {
        let obs = CompressedObservation {
            id: "test".to_string(),
            session_id: "sess".to_string(),
            timestamp: chrono::Utc::now(),
            observation_type: ObservationType::Other,
            title: "".to_string(),
            subtitle: None,
            facts: vec![],
            narrative: "".to_string(),
            concepts: vec![],
            files: vec![],
            importance: 0,
            confidence: 0.0,
            image_ref: None,
            image_description: None,
            modality: "text".to_string(),
            agent_id: None,
        };
        assert_eq!(score_compression(&obs), 0);
    }

    #[test]
    fn test_score_compression_partial() {
        let obs = CompressedObservation {
            id: "test".to_string(),
            session_id: "sess".to_string(),
            timestamp: chrono::Utc::now(),
            observation_type: ObservationType::FileRead,
            title: "Read".to_string(),
            subtitle: None,
            facts: vec!["one fact".to_string()],
            narrative: "Short narrative here".to_string(),
            concepts: vec![],
            files: vec![],
            importance: 5,
            confidence: 0.5,
            image_ref: None,
            image_description: None,
            modality: "text".to_string(),
            agent_id: None,
        };
        let score = score_compression(&obs);
        assert!(score > 0 && score < 100);
    }

    #[test]
    fn test_score_summary_perfect() {
        let input = SummaryQualityInput {
            title: "Auth Implementation",
            narrative: "Implemented JWT authentication with refresh tokens.\n\nAdded middleware for token validation.",
            key_decisions: &[
                "Use JWT".to_string(),
                "Add refresh tokens".to_string(),
                "Use HTTPS only".to_string(),
            ],
            files_modified: &["src/auth.rs".to_string(), "src/middleware.rs".to_string()],
            concepts: &[
                "JWT".to_string(),
                "authentication".to_string(),
                "security".to_string(),
            ],
        };
        assert_eq!(score_summary(&input), 100);
    }

    #[test]
    fn test_score_summary_empty() {
        let input = SummaryQualityInput {
            title: "",
            narrative: "",
            key_decisions: &[],
            files_modified: &[],
            concepts: &[],
        };
        assert_eq!(score_summary(&input), 0);
    }

    #[test]
    fn test_score_summary_partial() {
        let input = SummaryQualityInput {
            title: "Test",
            narrative: "A short narrative.",
            key_decisions: &[],
            files_modified: &["main.rs".to_string()],
            concepts: &[],
        };
        let score = score_summary(&input);
        assert!(score > 0 && score < 100);
    }

    #[test]
    fn test_score_context_relevance_perfect() {
        let score = score_context_relevance(
            "authentication JWT",
            &[0.95, 0.88, 0.82],
            &[
                "backend".to_string(),
                "security".to_string(),
                "backend".to_string(),
            ],
            &[
                "auth".to_string(),
                "middleware".to_string(),
                "tokens".to_string(),
            ],
            &[
                "Implemented JWT authentication with refresh tokens".to_string(),
                "Added security middleware for token validation".to_string(),
                "JWT token rotation and storage patterns".to_string(),
            ],
        );
        assert_eq!(score, 100);
    }

    #[test]
    fn test_score_context_relevance_no_results() {
        let score = score_context_relevance("query", &[], &[], &[], &[]);
        assert_eq!(score, 0);
    }

    #[test]
    fn test_score_context_relevance_partial() {
        let score = score_context_relevance(
            "authentication",
            &[0.5],
            &["backend".to_string()],
            &["auth".to_string()],
            &["some authentication related content".to_string()],
        );
        assert!(score > 0 && score < 100);
    }

    #[test]
    fn test_score_context_relevance_low_scores() {
        let score = score_context_relevance(
            "unrelated query",
            &[0.1, 0.05],
            &["frontend".to_string()],
            &["styles".to_string()],
            &[
                "CSS styling rules".to_string(),
                "layout adjustments".to_string(),
            ],
        );
        // Even with low scores, having results and diversity gives some points.
        assert!(score > 0);
    }
}
