use anyhow::Result;
use serde::{Deserialize, Serialize};

pub struct DiskSizeManager {
    current_bytes: i64,
    max_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskSizeResult {
    pub success: bool,
    pub current_total: i64,
}

impl DiskSizeManager {
    pub fn new(max_bytes: u64) -> Self {
        Self {
            current_bytes: 0,
            max_bytes,
        }
    }

    pub fn apply_delta(&mut self, delta_bytes: i64) -> DiskSizeResult {
        let new_total = self.current_bytes + delta_bytes;
        self.current_bytes = new_total.max(0);

        DiskSizeResult {
            success: true,
            current_total: self.current_bytes,
        }
    }

    pub fn is_over_quota(&self) -> bool {
        self.current_bytes > 0 && (self.current_bytes as u64) > self.max_bytes
    }

    pub fn current_bytes(&self) -> i64 {
        self.current_bytes
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    pub fn remaining_bytes(&self) -> i64 {
        ((self.max_bytes as i64) - self.current_bytes).max(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_delta_positive() {
        let mut manager = DiskSizeManager::new(1000);
        let result = manager.apply_delta(500);
        assert!(result.success);
        assert_eq!(result.current_total, 500);
    }

    #[test]
    fn test_apply_delta_negative() {
        let mut manager = DiskSizeManager::new(1000);
        manager.apply_delta(500);
        let result = manager.apply_delta(-200);
        assert_eq!(result.current_total, 300);
    }

    #[test]
    fn test_apply_delta_below_zero() {
        let mut manager = DiskSizeManager::new(1000);
        let result = manager.apply_delta(-500);
        assert_eq!(result.current_total, 0);
    }

    #[test]
    fn test_is_over_quota() {
        let mut manager = DiskSizeManager::new(100);
        manager.apply_delta(200);
        assert!(manager.is_over_quota());
    }

    #[test]
    fn test_is_under_quota() {
        let mut manager = DiskSizeManager::new(1000);
        manager.apply_delta(500);
        assert!(!manager.is_over_quota());
    }

    #[test]
    fn test_remaining_bytes() {
        let mut manager = DiskSizeManager::new(1000);
        manager.apply_delta(300);
        assert_eq!(manager.remaining_bytes(), 700);
    }

    #[test]
    fn test_remaining_bytes_negative() {
        let mut manager = DiskSizeManager::new(100);
        manager.apply_delta(200);
        assert_eq!(manager.remaining_bytes(), 0);
    }
}
