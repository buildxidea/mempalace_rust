use crate::knowledge_graph::KnowledgeGraph;
use crate::types::MemoryRelation;
use anyhow::Result;
use std::collections::{HashSet, VecDeque};

pub fn create_relation(
    kg: &mut KnowledgeGraph,
    from_id: &str,
    to_id: &str,
    relation_type: &str,
    weight: f64,
) -> Result<String> {
    let predicate = format!("{}_{:.2}", relation_type, weight);
    kg.add_triple(from_id, &predicate, to_id, None, None, Some(weight), None, None, None, None)
}

pub fn get_relations(
    kg: &KnowledgeGraph,
    memory_id: &str,
) -> Result<Vec<MemoryRelation>> {
    let outgoing = kg.query_entity(memory_id, None, None, "outgoing")?;
    let mut relations = Vec::new();

    for r in outgoing {
        let parts: Vec<&str> = r.predicate.rsplitn(2, '_').collect();
        let relation_type = if parts.len() == 2 { parts[1] } else { &r.predicate };
        let weight = if parts.len() == 2 {
            parts[0].parse::<f64>().unwrap_or(1.0)
        } else {
            1.0
        };

        relations.push(MemoryRelation {
            from_id: memory_id.to_string(),
            to_id: r.object,
            relation_type: relation_type.to_string(),
            weight,
        });
    }

    Ok(relations)
}

pub fn get_related(
    kg: &KnowledgeGraph,
    memory_id: &str,
    max_hops: usize,
    min_confidence: f64,
) -> Result<Vec<MemoryRelation>> {
    let mut visited = HashSet::new();
    let mut relations = Vec::new();
    let mut queue: VecDeque<(String, usize)> = VecDeque::new();

    queue.push_back((memory_id.to_string(), 0));
    visited.insert(memory_id.to_string());

    while let Some((current_id, depth)) = queue.pop_front() {
        if depth > max_hops {
            continue;
        }

        let outgoing = kg.query_entity(&current_id, None, None, "outgoing")?;
        for r in outgoing {
            let confidence = r.confidence.unwrap_or(1.0);
            if confidence < min_confidence {
                continue;
            }

            let parts: Vec<&str> = r.predicate.rsplitn(2, '_').collect();
            let relation_type = if parts.len() == 2 { parts[1] } else { &r.predicate };
            let weight = if parts.len() == 2 {
                parts[0].parse::<f64>().unwrap_or(1.0)
            } else {
                1.0
            };

            relations.push(MemoryRelation {
                from_id: current_id.clone(),
                to_id: r.object.clone(),
                relation_type: relation_type.to_string(),
                weight,
            });

            let obj_lower = r.object.to_lowercase();
            if !visited.contains(&obj_lower) && depth < max_hops {
                visited.insert(obj_lower);
                queue.push_back((r.object, depth + 1));
            }
        }
    }

    Ok(relations)
}

pub fn delete_relation(
    kg: &mut KnowledgeGraph,
    from_id: &str,
    to_id: &str,
    relation_type: &str,
) -> Result<()> {
    let relations = get_relations(kg, from_id)?;
    for r in relations {
        if r.to_id == to_id && r.relation_type == relation_type {
            let predicate = format!("{}_{:.2}", relation_type, r.weight);
            return kg.invalidate(from_id, &predicate, to_id, None);
        }
    }
    Ok(())
}

pub fn update_relation_weight(
    kg: &mut KnowledgeGraph,
    from_id: &str,
    to_id: &str,
    relation_type: &str,
    new_weight: f64,
) -> Result<String> {
    let relations = get_relations(kg, from_id)?;
    for r in relations {
        if r.to_id == to_id && r.relation_type == relation_type {
            let old_predicate = format!("{}_{:.2}", relation_type, r.weight);
            kg.invalidate(from_id, &old_predicate, to_id, None)?;
        }
    }
    create_relation(kg, from_id, to_id, relation_type, new_weight)
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
    fn test_create_relation() {
        let mut kg = test_kg();
        let id = create_relation(&mut kg, "mem-1", "mem-2", "supersedes", 0.8).unwrap();
        assert!(!id.is_empty());

        let relations = get_relations(&kg, "mem-1").unwrap();
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].to_id, "mem-2");
    }

    #[test]
    fn test_get_relations_multiple() {
        let mut kg = test_kg();
        create_relation(&mut kg, "mem-1", "mem-2", "supersedes", 0.8).unwrap();
        create_relation(&mut kg, "mem-1", "mem-3", "extends", 0.6).unwrap();
        create_relation(&mut kg, "mem-1", "mem-4", "related_to", 0.4).unwrap();

        let relations = get_relations(&kg, "mem-1").unwrap();
        assert_eq!(relations.len(), 3);
    }

    #[test]
    fn test_get_related_single_hop() {
        let mut kg = test_kg();
        create_relation(&mut kg, "mem-1", "mem-2", "supersedes", 0.8).unwrap();
        create_relation(&mut kg, "mem-2", "mem-3", "extends", 0.7).unwrap();

        let related = get_related(&kg, "mem-1", 1, 0.5).unwrap();
        assert!(related.len() >= 1);
        assert!(related.iter().any(|r| r.to_id == "mem-2"));
    }

    #[test]
    fn test_get_related_multi_hop() {
        let mut kg = test_kg();
        create_relation(&mut kg, "mem-1", "mem-2", "supersedes", 0.8).unwrap();
        create_relation(&mut kg, "mem-2", "mem-3", "extends", 0.7).unwrap();
        create_relation(&mut kg, "mem-3", "mem-4", "derives", 0.6).unwrap();

        let related = get_related(&kg, "mem-1", 3, 0.5).unwrap();
        assert!(related.len() >= 3);
    }

    #[test]
    fn test_get_related_with_confidence_filter() {
        let mut kg = test_kg();
        create_relation(&mut kg, "mem-1", "mem-2", "supersedes", 0.8).unwrap();
        create_relation(&mut kg, "mem-1", "mem-3", "related_to", 0.2).unwrap();

        let related = get_related(&kg, "mem-1", 1, 0.5).unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].to_id, "mem-2");
    }

    #[test]
    fn test_delete_relation() {
        let mut kg = test_kg();
        create_relation(&mut kg, "mem-1", "mem-2", "supersedes", 0.8).unwrap();
        let before = get_relations(&kg, "mem-1").unwrap();
        assert_eq!(before.len(), 1);

        let result = delete_relation(&mut kg, "mem-1", "mem-2", "supersedes");
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_relation_weight() {
        let mut kg = test_kg();
        create_relation(&mut kg, "mem-1", "mem-2", "supersedes", 0.5).unwrap();

        update_relation_weight(&mut kg, "mem-1", "mem-2", "supersedes", 0.9).unwrap();
        let relations = get_relations(&kg, "mem-1").unwrap();
        assert!(relations.iter().any(|r| (r.weight - 0.9).abs() < 0.01));
    }
}
