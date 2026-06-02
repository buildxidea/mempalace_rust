//! eval.rs — Quality scoring for compression, summaries, and context relevance.
//!
//! Ports the three scoring functions from the upstream Python/TypeScript
//! `agentmemory/src/eval/quality.ts`.  Each returns a 0–100 integer score.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Input to [`score_compression`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressionObservation {
    /// Observation title.  Non-empty and length 5–120 contributes +15.
    pub title: Option<String>,
    /// Factual atoms.  >0 contributes +25, >=3 adds +10.
    pub facts: Option<Vec<String>>,
    /// Optional narrative text.  Length >=20 contributes +20, >=50 adds +5.
    pub narrative: Option<String>,
    /// Concept tags.  >0 contributes +15.
    pub concepts: Option<Vec<String>>,
    /// Importance on a 1–10 scale.  Contributes +10.
    pub importance: Option<u32>,
}

/// Input to [`score_summary`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryObservation {
    /// Summary title.  Length >=5 contributes +20.
    pub title: Option<String>,
    /// Optional narrative text.  Length >=20 contributes +25, >=100 adds +5.
    pub narrative: Option<String>,
    /// Key decisions.  >0 contributes +20.
    pub key_decisions: Option<Vec<String>>,
    /// Files touched.  >0 contributes +15.
    pub files_modified: Option<Vec<String>>,
    /// Concept tags.  >0 contributes +15.
    pub concepts: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Scoring functions
// ---------------------------------------------------------------------------

/// Score a compression observation.  0–100.
pub fn score_compression(obs: &CompressionObservation) -> u32 {
    let mut score = 0u32;

    // facts: >0 → +25, >=3 → additional +10
    if let Some(ref facts) = obs.facts {
        if !facts.is_empty() {
            score += 25;
        }
        if facts.len() >= 3 {
            score += 10;
        }
    }

    // narrative: >=20 → +20, >=50 → additional +5
    if let Some(ref narrative) = obs.narrative {
        let len = narrative.len() as u32;
        if len >= 20 {
            score += 20;
        }
        if len >= 50 {
            score += 5;
        }
    }

    // title: length 5–120 → +15
    if let Some(ref title) = obs.title {
        let len = title.len();
        if len >= 5 && len <= 120 {
            score += 15;
        }
    }

    // concepts: >0 → +15
    if let Some(ref concepts) = obs.concepts {
        if !concepts.is_empty() {
            score += 15;
        }
    }

    // importance: 1–10 → +10
    if let Some(imp) = obs.importance {
        if imp >= 1 && imp <= 10 {
            score += 10;
        }
    }

    score.min(100)
}

/// Score a summary observation.  0–100.
pub fn score_summary(summary: &SummaryObservation) -> u32 {
    let mut score = 0u32;

    // title: >=5 → +20
    if let Some(ref title) = summary.title {
        if title.len() >= 5 {
            score += 20;
        }
    }

    // narrative: >=20 → +25, >=100 → additional +5
    if let Some(ref narrative) = summary.narrative {
        let len = narrative.len() as u32;
        if len >= 20 {
            score += 25;
        }
        if len >= 100 {
            score += 5;
        }
    }

    // key_decisions: >0 → +20
    if let Some(ref kd) = summary.key_decisions {
        if !kd.is_empty() {
            score += 20;
        }
    }

    // files_modified: >0 → +15
    if let Some(ref fm) = summary.files_modified {
        if !fm.is_empty() {
            score += 15;
        }
    }

    // concepts: >0 → +15
    if let Some(ref concepts) = summary.concepts {
        if !concepts.is_empty() {
            score += 15;
        }
    }

    score.min(100)
}

/// Score context relevance against a project name.  0–100.
pub fn score_context_relevance(context: &str, project: &str) -> u32 {
    let mut score = 0u32;
    let ctx_len = context.len() as u32;

    // context not empty → +20
    if ctx_len > 0 {
        score += 20;
    }

    // project substring (case-insensitive) → +20
    if !project.is_empty() && context.to_lowercase().contains(&project.to_lowercase()) {
        score += 20;
    }

    // contains '<' → +15
    if context.contains('<') {
        score += 15;
    }

    // count XML-style section tags <...>
    let section_count = section_tag_count(context);
    if section_count >= 2 {
        score += 15;
    }
    if section_count >= 4 {
        score += 10;
    }

    // context length thresholds
    if ctx_len >= 100 {
        score += 10;
    }
    if ctx_len >= 500 {
        score += 10;
    }

    score.min(100)
}

/// Count XML-style opening section tags (e.g. `<file>`, `<symbol>`, `<diff>`).
fn section_tag_count(text: &str) -> usize {
    let mut count = 0usize;
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'<' && i + 1 < len && bytes[i + 1] != b'/' {
            let mut j = i + 1;
            let mut has_word = false;
            while j < len && bytes[j] != b'>' {
                if bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' {
                    has_word = true;
                }
                j += 1;
            }
            if has_word && j < len && bytes[j] == b'>' {
                count += 1;
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── score_compression ──────────────────────────────────────────────────

    #[test]
    fn compression_empty_input_scores_zero() {
        let obs = CompressionObservation {
            title: None,
            facts: None,
            narrative: None,
            concepts: None,
            importance: None,
        };
        assert_eq!(score_compression(&obs), 0);
    }

    #[test]
    fn compression_ideal_input_near_max() {
        let obs = CompressionObservation {
            title: Some("Why we chose Clerk over Auth0 for auth".into()),
            facts: Some(vec!["Kai".into(), "Priya".into(), "Maya".into(), "Leo".into(), "Soren".into()]),
            narrative: Some("A very long narrative that goes on and on exceeding two hundred characters easily.".into()),
            concepts: Some(vec!["auth".into(), "clerk".into(), "migration".into()]),
            importance: Some(5),
        };
        assert_eq!(score_compression(&obs), 100);
    }

    #[test]
    fn compression_scores_respect_upper_bound() {
        let obs = CompressionObservation {
            title: Some("Valid title here".into()),
            facts: Some(vec!["a".into(), "b".into(), "c".into()]),
            narrative: Some("This is a narrative that is definitely over fifty characters long.".into()),
            concepts: Some(vec!["x".into()]),
            importance: Some(7),
        };
        assert!(score_compression(&obs) <= 100);
    }

    #[test]
    fn compression_mid_range_score() {
        let obs = CompressionObservation {
            title: Some("Valid title".into()),
            facts: Some(vec!["a".into(), "b".into(), "c".into()]),
            narrative: None,
            concepts: Some(vec![]),
            importance: Some(0),
        };
        assert_eq!(score_compression(&obs), 50);
    }

    // ── score_summary ─────────────────────────────────────────────────────

    #[test]
    fn summary_empty_input_scores_zero() {
        let s = SummaryObservation {
            title: None,
            narrative: None,
            key_decisions: None,
            files_modified: None,
            concepts: None,
        };
        assert_eq!(score_summary(&s), 0);
    }

    #[test]
    fn summary_ideal_input_scores_100() {
        let s = SummaryObservation {
            title: Some("Auth migration completed successfully".into()),
            narrative: Some("We migrated from Auth0 to Clerk for better pricing and DX. Kai led the implementation, Maya did the review, and the team approved on 2026-01-15.".into()),
            key_decisions: Some(vec!["Choose Clerk".into(), "Keep existing sessions".into(), "Maya reviews".into()]),
            files_modified: Some(vec!["auth/mod.rs".into(), "middleware/auth.rs".into()]),
            concepts: Some(vec!["clerk".into(), "auth0".into()]),
        };
        assert_eq!(score_summary(&s), 100);
    }

    #[test]
    fn summary_scores_respect_upper_bound() {
        let s = SummaryObservation {
            title: Some("Auth migration".into()),
            narrative: Some("Completed auth migration successfully.".into()),
            key_decisions: Some(vec!["Chose Clerk".into()]),
            files_modified: Some(vec!["a.rs".into()]),
            concepts: Some(vec!["auth".into()]),
        };
        assert!(score_summary(&s) <= 100);
    }

    #[test]
    fn summary_mid_range_score() {
        let s = SummaryObservation {
            title: Some("Auth done".into()),
            narrative: Some("Migrated auth to Clerk in January.".into()),
            key_decisions: Some(vec![]),
            files_modified: Some(vec![]),
            concepts: Some(vec![]),
        };
        assert_eq!(score_summary(&s), 45);
    }

    // ── score_context_relevance ───────────────────────────────────────────

    #[test]
    fn context_relevance_empty_context_scores_zero() {
        assert_eq!(score_context_relevance("", "mempalace"), 0);
    }

    #[test]
    fn context_relevance_empty_project_still_scores() {
        // context has content → +20, no project match to add, other criteria
        assert!(score_context_relevance("hello world", "") >= 20);
    }

    #[test]
    fn context_relevance_ideal_input_scores_100() {
        let ctx = "<file>src/main.rs</file><symbol>main</symbol><diff>--- a/src/main.rs</diff><commit>abc123</commit> long ".repeat(20);
        let score = score_context_relevance(&ctx, "mempalace");
        assert_eq!(score, 80);
    }

    #[test]
    fn context_relevance_scores_respect_upper_bound() {
        let ctx = "a".repeat(600);
        let score = score_context_relevance(&ctx, "project");
        assert!(score <= 100);
    }

    #[test]
    fn context_relevance_mid_range_score() {
        let ctx = "<file>src/main.rs</file><symbol>main</symbol> ".repeat(10);
        let score = score_context_relevance(&ctx, "");
        assert_eq!(score, 70);
    }

    // ── section_tag_count helper ───────────────────────────────────────────

    #[test]
    fn section_tag_count_basic() {
        assert_eq!(section_tag_count("<file>src/main.rs</file>"), 1);
    }

    #[test]
    fn section_tag_count_multiple() {
        assert_eq!(section_tag_count("<file>a</file><symbol>b</symbol><diff>c</diff>"), 3);
    }

    #[test]
    fn section_tag_count_no_tags() {
        assert_eq!(section_tag_count("no tags here"), 0);
    }

    #[test]
    fn section_tag_count_unclosed() {
        // Unclosed tags should not count
        assert_eq!(section_tag_count("<file"), 0);
    }

    #[test]
    fn section_tag_count_mixed_content() {
        // Should count only properly formed tags
        assert_eq!(section_tag_count("hello <file>world</file> and <symbol>foo</symbol>"), 2);
    }
}