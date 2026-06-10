/// Followup tracking for smart_search.
///
/// When an agent issues a second smart_search within
/// `FOLLOWUP_WINDOW_SECONDS` (default 30s) and the new result set has
/// ZERO overlap with the prior, we count this as a directional signal
/// that "first results didn't satisfy" the agent.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Duration window (in seconds) for detecting a followup search.
pub const FOLLOWUP_WINDOW_SECONDS: i64 = 30;

/// Per-agent/project state for the last search.
#[derive(Debug, Clone)]
pub struct FollowupState {
    pub last_query: String,
    pub last_result_ids: Vec<String>,
    pub last_timestamp: DateTime<Utc>,
}

/// A recorded zero-overlap followup event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowupEvent {
    pub at: DateTime<Utc>,
    pub query_a: String,
    pub query_b: String,
    pub overlap: usize,
}

/// Tracks follow-up smart_search calls to detect when initial results
/// don't satisfy the agent.
pub struct FollowupTracker {
    last_results: HashMap<String, FollowupState>,
    events: Vec<FollowupEvent>,
    total_followups: u64,
}

impl FollowupTracker {
    /// Create a new, empty tracker.
    pub fn new() -> Self {
        Self {
            last_results: HashMap::new(),
            events: Vec::new(),
            total_followups: 0,
        }
    }

    fn key(agent_id: &str, project: &str) -> String {
        format!("{}:{}", agent_id, project)
    }

    /// Record a smart_search call.
    ///
    /// Returns `Some(FollowupEvent)` if this search is a followup
    /// within the time window whose result set has **zero overlap**
    /// with the immediately preceding search for this `(agent_id, project)`.
    pub fn record_search(
        &mut self,
        agent_id: &str,
        project: &str,
        query: &str,
        result_ids: &[String],
    ) -> Option<FollowupEvent> {
        let k = Self::key(agent_id, project);
        let now = Utc::now();

        let event = if let Some(prior) = self.last_results.get(&k) {
            let elapsed = (now - prior.last_timestamp).num_seconds();
            if elapsed < FOLLOWUP_WINDOW_SECONDS {
                let overlap = prior
                    .last_result_ids
                    .iter()
                    .filter(|id| result_ids.contains(id))
                    .count();
                if overlap == 0 {
                    Some(FollowupEvent {
                        at: now,
                        query_a: prior.last_query.clone(),
                        query_b: query.to_string(),
                        overlap: 0,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Always update the last-results state.
        self.last_results.insert(
            k,
            FollowupState {
                last_query: query.to_string(),
                last_result_ids: result_ids.to_vec(),
                last_timestamp: now,
            },
        );

        if let Some(ref ev) = event {
            self.total_followups += 1;
            self.events.push(ev.clone());
            // Cap events to prevent unbounded memory growth.
            if self.events.len() > 1000 {
                self.events.remove(0);
            }
        }

        event
    }

    /// Return the current followup metrics snapshot.
    pub fn metrics(&self) -> FollowupMetric {
        FollowupMetric {
            total_followups: self.total_followups,
            recent_followups: self.events.clone(),
        }
    }
}

/// Snapshot of followup diagnostic metrics, serialisable for the REST API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowupMetric {
    pub total_followups: u64,
    pub recent_followups: Vec<FollowupEvent>,
}
