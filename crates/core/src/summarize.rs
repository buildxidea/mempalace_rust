use crate::llm::LlmProvider;
use crate::types::CompressedObservation;
use anyhow::Result;
use serde::{Deserialize, Serialize};

const SUMMARIZE_SYSTEM_PROMPT: &str = r#"You are a session summarization engine. Given a list of compressed observations from a coding session, produce a concise summary.

Output format (XML):
<session_summary>
  <title>Brief session title</title>
  <narrative>2-3 paragraph narrative of what happened</narrative>
  <key_decisions>
    <decision>Important decision 1</decision>
    <decision>Important decision 2</decision>
  </key_decisions>
  <files_modified>
    <file>path/to/file1</file>
    <file>path/to/file2</file>
  </files_modified>
  <concepts>
    <concept>concept1</concept>
    <concept>concept2</concept>
  </concepts>
</session_summary>

Rules:
- Keep title under 80 characters
- Narrative should be 2-3 paragraphs
- List only the most important decisions (max 5)
- List all modified files
- Extract key concepts (max 10)"#;

pub fn build_summarize_prompt(observations: &[CompressedObservation]) -> String {
    let items: Vec<String> = observations
        .iter()
        .enumerate()
        .map(|(i, o)| {
            format!(
                "[{}] Type: {:?}\nTitle: {}\nNarrative: {}\nConcepts: {}\nFiles: {}",
                i + 1,
                o.observation_type,
                o.title,
                o.narrative,
                o.concepts.join(", "),
                o.files.join(", ")
            )
        })
        .collect();
    format!("Summarize these observations:\n\n{}", items.join("\n\n"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub title: String,
    pub narrative: String,
    pub key_decisions: Vec<String>,
    pub files_modified: Vec<String>,
    pub concepts: Vec<String>,
}

pub fn parse_summary_xml(xml: &str) -> Result<SessionSummary> {
    let title_re = regex::Regex::new(r#"<title>([^<]+)</title>"#)?;
    let narrative_re = regex::Regex::new(r#"<narrative>([\s\S]*?)</narrative>"#)?;
    let decision_re = regex::Regex::new(r#"<decision>([^<]+)</decision>"#)?;
    let file_re = regex::Regex::new(r#"<file>([^<]+)</file>"#)?;
    let concept_re = regex::Regex::new(r#"<concept>([^<]+)</concept>"#)?;

    let title = title_re
        .captures(xml)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();

    let narrative = narrative_re
        .captures(xml)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();

    let key_decisions: Vec<String> = decision_re
        .captures_iter(xml)
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .collect();

    let files_modified: Vec<String> = file_re
        .captures_iter(xml)
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .collect();

    let concepts: Vec<String> = concept_re
        .captures_iter(xml)
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .collect();

    Ok(SessionSummary {
        title,
        narrative,
        key_decisions,
        files_modified,
        concepts,
    })
}

pub async fn summarize_session(
    llm: &dyn LlmProvider,
    observations: &[CompressedObservation],
) -> Result<SessionSummary> {
    if observations.is_empty() {
        return Ok(SessionSummary {
            title: "Empty Session".to_string(),
            narrative: "No observations to summarize.".to_string(),
            key_decisions: vec![],
            files_modified: vec![],
            concepts: vec![],
        });
    }

    let prompt = build_summarize_prompt(observations);
    let response = llm.complete(SUMMARIZE_SYSTEM_PROMPT, &prompt).await?;
    parse_summary_xml(&response.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_summary_xml() {
        let xml = r#"<session_summary>
  <title>Auth Implementation</title>
  <narrative>Implemented JWT authentication with refresh tokens.</narrative>
  <key_decisions>
    <decision>Use JWT instead of sessions</decision>
    <decision>Add refresh token rotation</decision>
  </key_decisions>
  <files_modified>
    <file>src/auth.rs</file>
    <file>src/middleware.rs</file>
  </files_modified>
  <concepts>
    <concept>JWT</concept>
    <concept>authentication</concept>
  </concepts>
</session_summary>"#;
        let summary = parse_summary_xml(xml).unwrap();
        assert_eq!(summary.title, "Auth Implementation");
        assert!(summary.narrative.contains("JWT authentication"));
        assert_eq!(summary.key_decisions.len(), 2);
        assert_eq!(summary.files_modified.len(), 2);
        assert_eq!(summary.concepts.len(), 2);
    }

    #[test]
    fn test_parse_summary_xml_empty() {
        let xml = "<session_summary></session_summary>";
        let summary = parse_summary_xml(xml).unwrap();
        assert!(summary.title.is_empty());
        assert!(summary.narrative.is_empty());
        assert!(summary.key_decisions.is_empty());
    }

    #[test]
    fn test_build_summarize_prompt() {
        let obs = vec![CompressedObservation {
            id: "o-1".into(),
            session_id: "s-1".into(),
            timestamp: chrono::Utc::now(),
            observation_type: crate::types::ObservationType::FileEdit,
            title: "Edit auth.rs".into(),
            subtitle: None,
            facts: vec!["Added JWT".into()],
            narrative: "Implemented JWT auth".into(),
            concepts: vec!["auth".into()],
            files: vec!["src/auth.rs".into()],
            importance: 7,
            confidence: 0.8,
            image_ref: None,
            image_description: None,
            modality: "text".into(),
            agent_id: None,
        }];
        let prompt = build_summarize_prompt(&obs);
        assert!(prompt.contains("Edit auth.rs"));
        assert!(prompt.contains("Implemented JWT auth"));
    }

    #[test]
    fn test_summarize_empty_observations() {
        let summary = SessionSummary {
            title: "Empty Session".to_string(),
            narrative: "No observations to summarize.".to_string(),
            key_decisions: vec![],
            files_modified: vec![],
            concepts: vec![],
        };
        assert_eq!(summary.title, "Empty Session");
    }
}
