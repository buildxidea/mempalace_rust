use crate::types::{Action, ActionStatus};
use chrono::Utc;

pub struct FrontierEntry {
    pub action: Action,
    pub score: f64,
}

pub fn compute_frontier(actions: &[Action], agent_id: Option<&str>, active_leases: &[(String, String)]) -> Vec<FrontierEntry> {
    let mut entries: Vec<FrontierEntry> = actions
        .iter()
        .filter(|a| should_include(a, agent_id, active_leases))
        .map(|a| FrontierEntry {
            action: a.clone(),
            score: compute_score(a),
        })
        .collect();

    entries.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    entries
}

fn should_include(action: &Action, agent_id: Option<&str>, active_leases: &[(String, String)]) -> bool {
    match action.status {
        ActionStatus::Completed | ActionStatus::Cancelled | ActionStatus::Blocked => false,
        ActionStatus::Pending | ActionStatus::InProgress | ActionStatus::Failed => {
            if let Some(aid) = agent_id {
                !active_leases.iter().any(|(action_id, holder)| action_id == &action.id && holder != aid)
            } else {
                true
            }
        }
    }
}

fn compute_score(action: &Action) -> f64 {
    let priority_score = (5.0 - action.priority as f64) * 10.0;

    let age_hours = (Utc::now() - action.created_at).num_seconds() as f64 / 3600.0;
    let age_score = (age_hours * 0.5).min(20.0);

    let mut score = priority_score + age_score;

    if action.status == ActionStatus::InProgress {
        score += 15.0;
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ActionStatus;
    use std::collections::HashMap;

    fn test_action(id: &str, priority: u8, status: ActionStatus) -> Action {
        Action {
            id: id.to_string(),
            title: format!("Action {}", id),
            description: "Test".to_string(),
            status,
            priority,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: None,
            assigned_to: None,
            project: "test".to_string(),
            tags: vec![],
            source_observation_ids: vec![],
            source_memory_ids: vec![],
            result: None,
            parent_id: None,
            metadata: HashMap::new(),
            sketch_id: None,
            crystallized_into: None,
        }
    }

    #[test]
    fn test_frontier_excludes_completed() {
        let actions = vec![
            test_action("a-1", 1, ActionStatus::Pending),
            test_action("a-2", 2, ActionStatus::Completed),
            test_action("a-3", 1, ActionStatus::InProgress),
        ];
        let frontier = compute_frontier(&actions, None, &[]);
        assert_eq!(frontier.len(), 2);
        assert!(frontier.iter().all(|e| e.action.id != "a-2"));
    }

    #[test]
    fn test_frontier_excludes_blocked() {
        let actions = vec![
            test_action("a-1", 1, ActionStatus::Pending),
            test_action("a-2", 1, ActionStatus::Blocked),
        ];
        let frontier = compute_frontier(&actions, None, &[]);
        assert_eq!(frontier.len(), 1);
        assert_eq!(frontier[0].action.id, "a-1");
    }

    #[test]
    fn test_frontier_priority_ordering() {
        let actions = vec![
            test_action("a-low", 4, ActionStatus::Pending),
            test_action("a-high", 1, ActionStatus::Pending),
        ];
        let frontier = compute_frontier(&actions, None, &[]);
        assert_eq!(frontier[0].action.id, "a-high");
        assert!(frontier[0].score > frontier[1].score);
    }

    #[test]
    fn test_frontier_in_progress_bonus() {
        let actions = vec![
            test_action("a-1", 2, ActionStatus::Pending),
            test_action("a-2", 2, ActionStatus::InProgress),
        ];
        let frontier = compute_frontier(&actions, None, &[]);
        assert_eq!(frontier[0].action.id, "a-2");
    }

    #[test]
    fn test_frontier_lease_exclusion() {
        let actions = vec![
            test_action("a-1", 1, ActionStatus::Pending),
            test_action("a-2", 1, ActionStatus::Pending),
        ];
        let leases = vec![("a-1".to_string(), "other-agent".to_string())];
        let frontier = compute_frontier(&actions, Some("my-agent"), &leases);
        assert_eq!(frontier.len(), 1);
        assert_eq!(frontier[0].action.id, "a-2");
    }

    #[test]
    fn test_compute_score_formula() {
        let action = test_action("a-1", 1, ActionStatus::Pending);
        let score = compute_score(&action);
        let priority_component = (5 - 1) as f64 * 10.0;
        assert!(score >= priority_component);
    }
}
