use crate::types::CompressedObservation;
use chrono::{DateTime, Utc};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct WorkingMemoryEntry {
    pub observation: CompressedObservation,
    pub importance: u8,
    pub access_count: usize,
    pub last_accessed: DateTime<Utc>,
}

pub struct WorkingMemory {
    entries: VecDeque<WorkingMemoryEntry>,
    max_token_budget: usize,
    current_token_count: usize,
}

impl WorkingMemory {
    pub fn new(max_token_budget: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            max_token_budget,
            current_token_count: 0,
        }
    }

    pub fn add(&mut self, observation: CompressedObservation) {
        let token_cost = estimate_tokens(&observation);
        self.entries.push_back(WorkingMemoryEntry {
            observation,
            importance: 5,
            access_count: 0,
            last_accessed: Utc::now(),
        });
        self.current_token_count += token_cost;
        self.evict_if_needed();
    }

    pub fn access(&mut self, index: usize) -> Option<&CompressedObservation> {
        if index < self.entries.len() {
            let entry = &mut self.entries[index];
            entry.access_count += 1;
            entry.last_accessed = Utc::now();
            Some(&entry.observation)
        } else {
            None
        }
    }

    pub fn evict_if_needed(&mut self) {
        while self.current_token_count > self.max_token_budget && !self.entries.is_empty() {
            let lowest_idx = self.find_lowest_score_index();
            if let Some(entry) = self.entries.remove(lowest_idx) {
                self.current_token_count -= estimate_tokens(&entry.observation);
            }
        }
    }

    fn find_lowest_score_index(&self) -> usize {
        let mut lowest_idx = 0;
        let mut lowest_score = f64::MAX;

        for (i, entry) in self.entries.iter().enumerate() {
            let score = self.compute_eviction_score(entry);
            if score < lowest_score {
                lowest_score = score;
                lowest_idx = i;
            }
        }
        lowest_idx
    }

    fn compute_eviction_score(&self, entry: &WorkingMemoryEntry) -> f64 {
        let importance_score = (entry.importance as f64 / 10.0) * 0.5;

        let days_since_access = (Utc::now() - entry.last_accessed).num_seconds() as f64 / 86400.0;
        let recency_score = (1.0 / (1.0 + days_since_access * 0.1)) * 0.3;

        let access_score = ((entry.access_count as f64 + 1.0).log2() / 10.0) * 0.2;

        importance_score + recency_score + access_score
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &VecDeque<WorkingMemoryEntry> {
        &self.entries
    }

    pub fn token_usage(&self) -> (usize, usize) {
        (self.current_token_count, self.max_token_budget)
    }
}

fn estimate_tokens(obs: &CompressedObservation) -> usize {
    let total_chars = obs.title.len()
        + obs.narrative.len()
        + obs.facts.iter().map(|f| f.len()).sum::<usize>()
        + obs.concepts.iter().map(|c| c.len()).sum::<usize>()
        + obs.files.iter().map(|f| f.len()).sum::<usize>();
    (total_chars + 2) / 3
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ObservationType;

    fn test_observation(id: &str, importance: u8, narrative_len: usize) -> CompressedObservation {
        CompressedObservation {
            id: id.to_string(),
            session_id: "s-1".to_string(),
            timestamp: Utc::now(),
            observation_type: ObservationType::FileEdit,
            title: format!("Title {}", id),
            subtitle: None,
            facts: vec![format!("Fact {}", id)],
            narrative: "x".repeat(narrative_len),
            concepts: vec![],
            files: vec![],
            importance,
            confidence: 0.8,
            image_ref: None,
            image_description: None,
            modality: "text".to_string(),
            agent_id: None,
        }
    }

    #[test]
    fn test_add_to_working_memory() {
        let mut wm = WorkingMemory::new(1000);
        wm.add(test_observation("o-1", 5, 100));
        assert_eq!(wm.len(), 1);
    }

    #[test]
    fn test_eviction_on_budget_exceeded() {
        let mut wm = WorkingMemory::new(100);
        wm.add(test_observation("o-1", 5, 100));
        wm.add(test_observation("o-2", 3, 100));
        wm.add(test_observation("o-3", 1, 100));
        assert!(wm.len() <= 2);
    }

    #[test]
    fn test_access_updates_recency() {
        let mut wm = WorkingMemory::new(1000);
        wm.add(test_observation("o-1", 1, 50));
        wm.add(test_observation("o-2", 5, 50));

        wm.access(0);
        let entry = &wm.entries()[0];
        assert_eq!(entry.access_count, 1);
    }

    #[test]
    fn test_eviction_score_formula() {
        let wm = WorkingMemory::new(1000);
        let entry = WorkingMemoryEntry {
            observation: test_observation("o-1", 5, 100),
            importance: 5,
            access_count: 0,
            last_accessed: Utc::now(),
        };
        let score = wm.compute_eviction_score(&entry);
        let importance_component = (5.0 / 10.0) * 0.5;
        let recency_component = (1.0 / (1.0 + 0.0 * 0.1)) * 0.3;
        let access_component = ((0.0_f64 + 1.0).log2() / 10.0) * 0.2;
        let expected = importance_component + recency_component + access_component;
        assert!((score - expected).abs() < 0.01);
    }

    #[test]
    fn test_token_usage_tracking() {
        let mut wm = WorkingMemory::new(1000);
        wm.add(test_observation("o-1", 5, 300));
        let (used, max) = wm.token_usage();
        assert!(used > 0);
        assert_eq!(max, 1000);
    }

    #[test]
    fn test_empty_working_memory() {
        let wm = WorkingMemory::new(1000);
        assert!(wm.is_empty());
        assert_eq!(wm.len(), 0);
    }
}
