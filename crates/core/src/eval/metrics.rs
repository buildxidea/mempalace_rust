//! Per-function quality tracking for the evaluation framework.
//!
//! Stores quality scores keyed by function name (e.g. "compress", "summarize"),
//! enabling trend analysis and threshold-based alerting.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A single quality measurement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityMeasurement {
    /// Score in 0-100 range.
    pub score: u8,
    /// ISO-8601 timestamp of when this measurement was taken.
    pub timestamp: String,
    /// Optional human-readable note about this measurement.
    pub note: Option<String>,
}

/// Per-function quality tracking store.
///
/// Thread-safe via `Arc<Mutex<...>>`. All scores are clamped to 0-100.
pub struct MetricsStore {
    inner: Arc<Mutex<MetricsStoreInner>>,
}

struct MetricsStoreInner {
    /// function_name -> list of measurements (most recent last).
    measurements: HashMap<String, Vec<QualityMeasurement>>,
    /// Function-level alert thresholds (0-100). When the rolling average
    /// drops below this value for a function, `check_threshold` returns
    /// the function name.
    thresholds: HashMap<String, u8>,
    /// Maximum measurements to keep per function (ring-buffer cap).
    max_per_function: usize,
}

impl MetricsStore {
    /// Create a new empty metrics store.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MetricsStoreInner {
                measurements: HashMap::new(),
                thresholds: HashMap::new(),
                max_per_function: 100,
            })),
        }
    }

    /// Create a metrics store with a custom ring-buffer cap.
    pub fn with_max_per_function(max: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(MetricsStoreInner {
                measurements: HashMap::new(),
                thresholds: HashMap::new(),
                max_per_function: max,
            })),
        }
    }

    /// Record a quality score for a named function.
    pub fn record(&self, function_name: &str, score: u8, note: Option<String>) {
        let score = score.min(100);
        let measurement = QualityMeasurement {
            score,
            timestamp: chrono::Utc::now().to_rfc3339(),
            note,
        };
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entries = inner
            .measurements
            .entry(function_name.to_string())
            .or_default();
        entries.push(measurement);
        // Ring-buffer: drop oldest when exceeding cap.
        let cap = inner.max_per_function;
        if entries.len() > cap {
            let drain_count = entries.len() - cap;
            entries.drain(..drain_count);
        }
    }

    /// Get all measurements for a function.
    pub fn get_measurements(&self, function_name: &str) -> Vec<QualityMeasurement> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .measurements
            .get(function_name)
            .cloned()
            .unwrap_or_default()
    }

    /// Get the rolling average score for a function over the last N measurements.
    pub fn rolling_average(&self, function_name: &str, window: usize) -> Option<f64> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let measurements = inner.measurements.get(function_name)?;
        if measurements.is_empty() {
            return None;
        }
        let window = window.min(measurements.len());
        let recent = &measurements[measurements.len() - window..];
        let sum: u64 = recent.iter().map(|m| m.score as u64).sum();
        Some(sum as f64 / window as f64)
    }

    /// Get the latest score for a function, if any.
    pub fn latest(&self, function_name: &str) -> Option<u8> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .measurements
            .get(function_name)?
            .last()
            .map(|m| m.score)
    }

    /// Get summary stats for all tracked functions.
    pub fn summary(&self) -> HashMap<String, FunctionStats> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut result = HashMap::new();
        for (name, measurements) in &inner.measurements {
            if measurements.is_empty() {
                continue;
            }
            let count = measurements.len();
            let sum: u64 = measurements.iter().map(|m| m.score as u64).sum();
            let avg = sum as f64 / count as f64;
            let min = measurements.iter().map(|m| m.score).min().unwrap_or(0);
            let max = measurements.iter().map(|m| m.score).max().unwrap_or(0);
            let latest = measurements.last().map(|m| m.score).unwrap_or(0);
            result.insert(
                name.clone(),
                FunctionStats {
                    count,
                    average: avg,
                    min,
                    max,
                    latest,
                },
            );
        }
        result
    }

    /// Set an alert threshold for a function. When the rolling average
    /// (over the last 10 measurements) drops below this value,
    /// `check_thresholds` will report it.
    pub fn set_threshold(&self, function_name: &str, threshold: u8) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .thresholds
            .insert(function_name.to_string(), threshold);
    }

    /// Check all functions against their thresholds.
    /// Returns a list of (function_name, current_average, threshold) for
    /// functions that are below threshold.
    pub fn check_thresholds(&self) -> Vec<(String, f64, u8)> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut alerts = Vec::new();
        for (name, threshold) in &inner.thresholds {
            if let Some(measurements) = inner.measurements.get(name) {
                if measurements.is_empty() {
                    continue;
                }
                let window = 10.min(measurements.len());
                let recent = &measurements[measurements.len() - window..];
                let sum: u64 = recent.iter().map(|m| m.score as u64).sum();
                let avg = sum as f64 / window as f64;
                if avg < *threshold as f64 {
                    alerts.push((name.clone(), avg, *threshold));
                }
            }
        }
        alerts
    }

    /// Clear all measurements for a function.
    pub fn clear(&self, function_name: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.measurements.remove(function_name);
    }

    /// Clear all measurements and thresholds.
    pub fn clear_all(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.measurements.clear();
        inner.thresholds.clear();
    }
}

impl Default for MetricsStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary statistics for a single function's quality measurements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionStats {
    pub count: usize,
    pub average: f64,
    pub min: u8,
    pub max: u8,
    pub latest: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_latest() {
        let store = MetricsStore::new();
        store.record("compress", 85, None);
        store.record("compress", 90, Some("improved".to_string()));
        assert_eq!(store.latest("compress"), Some(90));
    }

    #[test]
    fn test_record_clamps_score() {
        let store = MetricsStore::new();
        store.record("compress", 150, None);
        assert_eq!(store.latest("compress"), Some(100));
    }

    #[test]
    fn test_rolling_average() {
        let store = MetricsStore::new();
        store.record("compress", 80, None);
        store.record("compress", 90, None);
        store.record("compress", 100, None);
        let avg = store.rolling_average("compress", 3).unwrap();
        assert!((avg - 90.0).abs() < 0.01);
    }

    #[test]
    fn test_rolling_average_window_larger_than_data() {
        let store = MetricsStore::new();
        store.record("compress", 70, None);
        let avg = store.rolling_average("compress", 10).unwrap();
        assert!((avg - 70.0).abs() < 0.01);
    }

    #[test]
    fn test_rolling_average_empty() {
        let store = MetricsStore::new();
        assert!(store.rolling_average("compress", 10).is_none());
    }

    #[test]
    fn test_summary() {
        let store = MetricsStore::new();
        store.record("compress", 80, None);
        store.record("compress", 90, None);
        store.record("summarize", 70, None);
        let summary = store.summary();
        assert_eq!(summary.len(), 2);
        assert_eq!(summary["compress"].count, 2);
        assert!((summary["compress"].average - 85.0).abs() < 0.01);
        assert_eq!(summary["summarize"].latest, 70);
    }

    #[test]
    fn test_threshold_alerts() {
        let store = MetricsStore::new();
        store.set_threshold("compress", 80);
        store.record("compress", 50, None);
        store.record("compress", 60, None);
        let alerts = store.check_thresholds();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].0, "compress");
    }

    #[test]
    fn test_threshold_no_alert_when_above() {
        let store = MetricsStore::new();
        store.set_threshold("compress", 50);
        store.record("compress", 80, None);
        store.record("compress", 90, None);
        let alerts = store.check_thresholds();
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_ring_buffer_cap() {
        let store = MetricsStore::with_max_per_function(3);
        store.record("compress", 10, None);
        store.record("compress", 20, None);
        store.record("compress", 30, None);
        store.record("compress", 40, None);
        let measurements = store.get_measurements("compress");
        assert_eq!(measurements.len(), 3);
        assert_eq!(measurements[0].score, 20);
        assert_eq!(measurements[2].score, 40);
    }

    #[test]
    fn test_clear() {
        let store = MetricsStore::new();
        store.record("compress", 80, None);
        store.clear("compress");
        assert!(store.get_measurements("compress").is_empty());
        assert!(store.latest("compress").is_none());
    }

    #[test]
    fn test_clear_all() {
        let store = MetricsStore::new();
        store.record("compress", 80, None);
        store.record("summarize", 90, None);
        store.clear_all();
        assert!(store.summary().is_empty());
    }

    #[test]
    fn test_get_measurements_unknown() {
        let store = MetricsStore::new();
        assert!(store.get_measurements("unknown").is_empty());
    }
}
