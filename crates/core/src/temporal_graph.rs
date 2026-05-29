use crate::knowledge_graph::{KnowledgeGraph, Triple};
use anyhow::Result;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct TemporalQuery {
    pub entity_name: String,
    pub as_of: Option<DateTime<Utc>>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub include_history: bool,
}

#[derive(Debug, Clone)]
pub struct TemporalState {
    pub entity: String,
    pub current_edges: Vec<TripleInfo>,
    pub historical_edges: Vec<TripleInfo>,
    pub timeline: Vec<TimelineEntry>,
}

#[derive(Debug, Clone)]
pub struct TripleInfo {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub confidence: Option<f64>,
    pub current: bool,
}

#[derive(Debug, Clone)]
pub struct TimelineEntry {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub t_created: Option<String>,
    pub t_expired: Option<String>,
}

impl From<Triple> for TripleInfo {
    fn from(t: Triple) -> Self {
        Self {
            subject: t.subject,
            predicate: t.predicate,
            object: t.object,
            valid_from: t.valid_from,
            valid_to: t.valid_to,
            confidence: t.confidence,
            current: t.current,
        }
    }
}

impl From<Triple> for TimelineEntry {
    fn from(t: Triple) -> Self {
        Self {
            subject: t.subject,
            predicate: t.predicate,
            object: t.object,
            valid_from: t.valid_from,
            valid_to: t.valid_to,
            t_created: t.t_created,
            t_expired: t.t_expired,
        }
    }
}

pub fn temporal_query(
    kg: &KnowledgeGraph,
    query: &TemporalQuery,
) -> Result<TemporalState> {
    let current_edges = if query.include_history {
        kg.query_entity(&query.entity_name, None, None, "both")?
            .into_iter()
            .map(|r| TripleInfo {
                subject: r.subject,
                predicate: r.predicate,
                object: r.object,
                valid_from: r.valid_from,
                valid_to: r.valid_to,
                confidence: r.confidence,
                current: r.current,
            })
            .collect()
    } else {
        kg.query_entity(&query.entity_name, None, None, "both")?
            .into_iter()
            .filter(|r| r.current)
            .map(|r| TripleInfo {
                subject: r.subject,
                predicate: r.predicate,
                object: r.object,
                valid_from: r.valid_from,
                valid_to: r.valid_to,
                confidence: r.confidence,
                current: r.current,
            })
            .collect()
    };

    let historical_edges = if let Some(as_of) = query.as_of {
        let as_of_str = as_of.to_rfc3339();
        kg.query_entity(&query.entity_name, Some(&as_of_str), None, "both")?
            .into_iter()
            .map(|r| TripleInfo {
                subject: r.subject,
                predicate: r.predicate,
                object: r.object,
                valid_from: r.valid_from,
                valid_to: r.valid_to,
                confidence: r.confidence,
                current: r.current,
            })
            .collect()
    } else {
        Vec::new()
    };

    let timeline = kg.timeline(Some(&query.entity_name))?
        .into_iter()
        .map(TimelineEntry::from)
        .collect();

    Ok(TemporalState {
        entity: query.entity_name.clone(),
        current_edges,
        historical_edges,
        timeline,
    })
}

pub fn query_at_transaction_time(
    kg: &KnowledgeGraph,
    entity_name: &str,
    tt_as_of: &str,
) -> Result<Vec<TimelineEntry>> {
    let triples = kg.timeline_for_transaction_time(Some(entity_name), Some(tt_as_of))?;
    Ok(triples.into_iter().map(TimelineEntry::from).collect())
}

pub fn query_range(
    kg: &KnowledgeGraph,
    entity_name: &str,
    from: &DateTime<Utc>,
    to: &DateTime<Utc>,
) -> Result<Vec<TimelineEntry>> {
    let from_str = from.to_rfc3339();
    let to_str = to.to_rfc3339();

    let from_results = kg.query_entity(entity_name, Some(&from_str), None, "both")?;
    let to_results = kg.query_entity(entity_name, Some(&to_str), None, "both")?;

    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();

    for r in from_results.into_iter().chain(to_results.into_iter()) {
        let key = format!("{}|{}|{}", r.subject, r.predicate, r.object);
        if seen.insert(key) {
            entries.push(TimelineEntry {
                subject: r.subject,
                predicate: r.predicate,
                object: r.object,
                valid_from: r.valid_from,
                valid_to: r.valid_to,
                t_created: r.t_created,
                t_expired: r.t_expired,
            });
        }
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_graph::KnowledgeGraph;
    use std::path::Path;

    fn test_kg() -> KnowledgeGraph {
        KnowledgeGraph::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_temporal_query_current_state() {
        let mut kg = test_kg();
        kg.add_triple("Alice", "works_at", "Acme", None, None, Some(0.9), None, None, None, None).unwrap();

        let query = TemporalQuery {
            entity_name: "Alice".to_string(),
            as_of: None,
            from: None,
            to: None,
            include_history: false,
        };
        let result = temporal_query(&kg, &query).unwrap();
        assert_eq!(result.entity, "Alice");
        assert!(!result.current_edges.is_empty());
        assert_eq!(result.current_edges[0].object, "Acme");
    }

    #[test]
    fn test_temporal_query_as_of() {
        let mut kg = test_kg();
        kg.add_triple("Alice", "works_at", "Acme", Some("2020-01-01"), Some("2023-01-01"), Some(0.9), None, None, None, None).unwrap();
        kg.add_triple("Alice", "works_at", "NewCo", Some("2023-01-01"), None, Some(0.9), None, None, None, None).unwrap();

        let query = TemporalQuery {
            entity_name: "Alice".to_string(),
            as_of: Some(chrono::NaiveDate::from_ymd_opt(2021, 6, 1).unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc()),
            from: None,
            to: None,
            include_history: true,
        };
        let result = temporal_query(&kg, &query).unwrap();
        assert!(!result.historical_edges.is_empty());
    }

    #[test]
    fn test_temporal_query_with_timeline() {
        let mut kg = test_kg();
        kg.add_triple("Alice", "works_at", "Acme", Some("2020-01-01"), None, None, None, None, None, None).unwrap();
        kg.add_triple("Alice", "lives_in", "NYC", Some("2021-01-01"), None, None, None, None, None, None).unwrap();

        let query = TemporalQuery {
            entity_name: "Alice".to_string(),
            as_of: None,
            from: None,
            to: None,
            include_history: true,
        };
        let result = temporal_query(&kg, &query).unwrap();
        assert_eq!(result.timeline.len(), 2);
    }

    #[test]
    fn test_query_at_transaction_time() {
        let mut kg = test_kg();
        kg.add_triple("Alice", "works_at", "Acme", None, None, None, None, None, None, None).unwrap();

        let now = chrono::Utc::now().to_rfc3339();
        let entries = query_at_transaction_time(&kg, "Alice", &now).unwrap();
        assert!(!entries.is_empty());
        assert_eq!(entries[0].object, "Acme");
    }

    #[test]
    fn test_query_range() {
        let mut kg = test_kg();
        kg.add_triple("Alice", "works_at", "Acme", Some("2020-01-01"), None, None, None, None, None, None).unwrap();

        let from = chrono::NaiveDate::from_ymd_opt(2019, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc();
        let to = chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc();
        let entries = query_range(&kg, "Alice", &from, &to).unwrap();
        assert!(!entries.is_empty());
    }
}
