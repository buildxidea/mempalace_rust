use crate::knowledge_graph::{EntityQueryResult, KnowledgeGraph};
use anyhow::Result;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone)]
pub struct GraphRetrievalResult {
    pub entities: Vec<String>,
    pub relationships: Vec<RelationshipInfo>,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub struct RelationshipInfo {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub confidence: Option<f64>,
    pub current: bool,
}

impl From<EntityQueryResult> for RelationshipInfo {
    fn from(r: EntityQueryResult) -> Self {
        Self {
            subject: r.subject,
            predicate: r.predicate,
            object: r.object,
            confidence: r.confidence,
            current: r.current,
        }
    }
}

pub fn search_by_entities(
    kg: &KnowledgeGraph,
    entity_names: &[&str],
    depth: usize,
    limit: usize,
) -> Result<GraphRetrievalResult> {
    let max_depth = depth.min(5);
    let mut visited_entities = HashSet::new();
    let mut visited_edges = HashSet::new();
    let mut relationships = Vec::new();
    let mut queue: VecDeque<(String, usize)> =
        entity_names.iter().map(|n| (n.to_string(), 0)).collect();

    for name in entity_names {
        visited_entities.insert(name.to_lowercase().replace(' ', "_").replace('\'', ""));
    }

    while let Some((entity_name, current_depth)) = queue.pop_front() {
        if current_depth > max_depth {
            continue;
        }

        let results = kg.query_entity(&entity_name, None, None, "both")?;
        for r in &results {
            let edge_key = format!("{}|{}|{}", r.subject, r.predicate, r.object);
            if !visited_edges.contains(&edge_key) {
                visited_edges.insert(edge_key);
                relationships.push(RelationshipInfo::from(r.clone()));

                let other_id = if r.direction == "outgoing" {
                    r.object.clone()
                } else {
                    r.subject.clone()
                };
                let other_lower = other_id.to_lowercase();
                if !visited_entities.contains(&other_lower) && current_depth < max_depth {
                    visited_entities.insert(other_lower);
                    queue.push_back((other_id, current_depth + 1));
                }
            }
        }

        if relationships.len() >= limit {
            break;
        }
    }

    relationships.truncate(limit);
    let entities: Vec<String> = visited_entities.into_iter().collect();

    Ok(GraphRetrievalResult {
        entities,
        relationships,
        depth: max_depth,
    })
}

pub fn expand_from_chunks(
    kg: &KnowledgeGraph,
    top_observation_ids: &[String],
    depth: usize,
    limit: usize,
) -> Result<GraphRetrievalResult> {
    let mut all_results = GraphRetrievalResult {
        entities: Vec::new(),
        relationships: Vec::new(),
        depth: 0,
    };

    for obs_id in top_observation_ids {
        let result = search_by_entities(kg, &[obs_id], depth.min(1), limit)?;
        for e in result.entities {
            if !all_results.entities.contains(&e) {
                all_results.entities.push(e);
            }
        }
        all_results.relationships.extend(result.relationships);
        if all_results.relationships.len() >= limit {
            break;
        }
    }

    all_results.relationships.truncate(limit);
    Ok(all_results)
}

pub fn query_by_predicate(
    kg: &KnowledgeGraph,
    predicate: &str,
    as_of: Option<&str>,
) -> Result<Vec<RelationshipInfo>> {
    let triples = kg.query_relationship(predicate, as_of, None)?;
    Ok(triples
        .into_iter()
        .map(|t| RelationshipInfo {
            subject: t.subject,
            predicate: t.predicate,
            object: t.object,
            confidence: t.confidence,
            current: t.current,
        })
        .collect())
}

pub fn graph_stats(kg: &KnowledgeGraph) -> Result<HashMap<String, usize>> {
    let stats = kg.stats()?;
    let mut map = HashMap::new();
    map.insert("total_entities".to_string(), stats.total_entities);
    map.insert("total_triples".to_string(), stats.total_triples);
    map.insert("current_facts".to_string(), stats.current_facts);
    map.insert("expired_facts".to_string(), stats.expired_facts);
    map.insert(
        "relationship_types".to_string(),
        stats.relationship_types.len(),
    );
    Ok(map)
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
    fn test_search_by_entities_single_entity() {
        let mut kg = test_kg();
        kg.add_triple(
            "Alice",
            "works_at",
            "Acme",
            None,
            None,
            Some(0.9),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        kg.add_triple(
            "Alice",
            "knows",
            "Bob",
            None,
            None,
            Some(0.7),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let result = search_by_entities(&kg, &["Alice"], 2, 10).unwrap();
        assert!(!result.entities.is_empty());
        assert!(result.relationships.len() >= 2);
    }

    #[test]
    fn test_search_by_entities_with_depth() {
        let mut kg = test_kg();
        kg.add_triple(
            "A",
            "knows",
            "B",
            None,
            None,
            Some(0.8),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        kg.add_triple(
            "B",
            "knows",
            "C",
            None,
            None,
            Some(0.8),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        kg.add_triple(
            "C",
            "knows",
            "D",
            None,
            None,
            Some(0.8),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let result = search_by_entities(&kg, &["A"], 1, 10).unwrap();
        assert!(result.relationships.len() >= 1);
        let result_deep = search_by_entities(&kg, &["A"], 3, 10).unwrap();
        assert!(result_deep.relationships.len() >= result.relationships.len());
    }

    #[test]
    fn test_search_by_entities_with_limit() {
        let mut kg = test_kg();
        for i in 0..10 {
            kg.add_triple(
                "Root",
                &format!("rel_{}", i),
                &format!("Target{}", i),
                None,
                None,
                Some(0.5),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        }

        let result = search_by_entities(&kg, &["Root"], 1, 3).unwrap();
        assert!(result.relationships.len() <= 3);
    }

    #[test]
    fn test_expand_from_chunks() {
        let mut kg = test_kg();
        kg.add_triple(
            "obs-1",
            "mentions",
            "Alice",
            None,
            None,
            Some(0.8),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        kg.add_triple(
            "obs-2",
            "mentions",
            "Bob",
            None,
            None,
            Some(0.8),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let result =
            expand_from_chunks(&kg, &["obs-1".to_string(), "obs-2".to_string()], 1, 10).unwrap();
        assert!(!result.entities.is_empty());
    }

    #[test]
    fn test_query_by_predicate() {
        let mut kg = test_kg();
        kg.add_triple(
            "Alice",
            "works_at",
            "Acme",
            None,
            None,
            Some(0.9),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        kg.add_triple(
            "Bob",
            "works_at",
            "NewCo",
            None,
            None,
            Some(0.9),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let results = query_by_predicate(&kg, "works_at", None).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.predicate == "works_at"));
    }

    #[test]
    fn test_graph_stats() {
        let mut kg = test_kg();
        kg.add_entity("Alice", "person", None).unwrap();
        kg.add_triple(
            "Alice", "knows", "Bob", None, None, None, None, None, None, None,
        )
        .unwrap();

        let stats = graph_stats(&kg).unwrap();
        assert_eq!(stats["total_entities"], 2);
        assert_eq!(stats["total_triples"], 1);
        assert_eq!(stats["current_facts"], 1);
    }
}
