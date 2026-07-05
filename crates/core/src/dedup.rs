//! dedup.rs — Detect and remove near-duplicate drawers.
//!
//! Uses **cosine distance** on word bags (or embedding vectors) to find
//! drawers from the same `source_file` that are too similar. Keeps the
//! longest/richest version and deletes the rest.
//!
//! Configuration
//! =============
//! - `MEMPALACE_DEDUP_THRESHOLD` env var (default 0.15, i.e. ~85% similarity)
//! - `DedupConfig` struct for programmatic use
//!
//! Usage (CLI)
//! ===========
//! ```text
//! mpr dedup --dry-run                          # preview only
//! mpr dedup --threshold 0.10 --apply           # stricter, permanent
//! mpr dedup --wing my_project                  # scope to a wing
//! mpr dedup --stats                            # stats only
//! ```
//!
//! Usage (MCP)
//! ===========
//! ```json
//! {
//!   "method": "tools/call",
//!   "params": {
//!     "name": "mempalace_dedup",
//!     "arguments": {
//!       "threshold": 0.15,
//!       "dry_run": true,
//!       "wing": "my_project"
//!     }
//!   }
//! }
//! ```

use crate::palace_db::{PalaceDb, QueryResult};
use std::collections::HashMap;
use std::path::Path;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Cosine DISTANCE threshold (not similarity). Lower = stricter.
/// 0.15 = ~85% cosine similarity — catches near-identical chunks.
/// For looser dedup of paraphrased content, try 0.3–0.4.
const DEFAULT_THRESHOLD: f64 = 0.15;

/// Minimum number of drawers a source group must have before we check it.
const MIN_DRAWERS_TO_CHECK: usize = 5;

/// Smallest content length worth comparing. Shorter than this -> auto-delete
/// (too short to be meaningful, likely an empty / partial chunk).
const MIN_CONTENT_LENGTH: usize = 20;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the deduplication pass.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct DedupConfig {
    /// Cosine distance threshold. Default: 0.15.
    pub threshold: f64,
    /// When `true`, only print what would be deleted — no mutations.
    pub dry_run: bool,
    /// Scope to a single wing (project). `None` = all wings.
    pub wing: Option<String>,
    /// Filter by source-file substring. `None` = all sources.
    pub source_pattern: Option<String>,
    /// Minimum group size to trigger dedup. Default: 5.
    pub min_drawers_to_check: usize,
}

impl Default for DedupConfig {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
            dry_run: true,
            wing: None,
            source_pattern: None,
            min_drawers_to_check: MIN_DRAWERS_TO_CHECK,
        }
    }
}

impl DedupConfig {
    /// Load threshold from environment variable `MEMPALACE_DEDUP_THRESHOLD`.
    pub fn threshold_from_env() -> f64 {
        std::env::var("MEMPALACE_DEDUP_THRESHOLD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(DEFAULT_THRESHOLD)
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Deduplication statistics returned after a pass.
#[derive(Debug, Clone, serde::Serialize)]
#[non_exhaustive]
pub struct DedupStats {
    /// Number of source groups checked (i.e. sources with enough drawers).
    pub sources_checked: usize,
    /// Number of drawers kept.
    pub total_kept: usize,
    /// Number of drawers deleted (or marked for deletion in dry-run).
    pub total_deleted: usize,
    /// Total palette size before the pass.
    pub palace_size_before: usize,
    /// Total palette size after the pass (same as `palace_size_before`
    /// when `dry_run` is true).
    pub palace_size_after: usize,
    /// IDs of deleted drawers (empty in dry-run or if none deleted).
    #[serde(default)]
    pub deleted_ids: Vec<String>,
    /// Per-source summaries.
    #[serde(default)]
    pub source_summaries: Vec<SourceSummary>,
}

/// Per-source-file deduplication summary.
#[derive(Debug, Clone, serde::Serialize)]
#[non_exhaustive]
pub struct SourceSummary {
    pub source: String,
    pub before: usize,
    pub after: usize,
    pub deleted: usize,
}

// ---------------------------------------------------------------------------
// Main entry point — dedup on file path
// ---------------------------------------------------------------------------

/// Run a deduplication pass on the palace at `palace_path`.
///
/// When `config.dry_run` is `true`, no mutations are made. Returns a
/// [`DedupStats`] describing what would happen (or what happened).
pub fn dedup_palace(
    palace_path: Option<&Path>,
    config: &DedupConfig,
) -> anyhow::Result<DedupStats> {
    let cfg = crate::config::Config::load()?;
    let palace_path = palace_path.unwrap_or(cfg.palace_path.as_path());
    let mut palace_db = PalaceDb::open(palace_path)?;

    info!(
        "dedup: palace={}, drawers={}, threshold={}, dry_run={}, wing={:?}",
        palace_path.display(),
        palace_db.count(),
        config.threshold,
        config.dry_run,
        config.wing,
    );

    let stats = dedup_db(&mut palace_db, config)?;

    Ok(stats)
}

// ---------------------------------------------------------------------------
// Main entry point — dedup on open PalaceDb
// ---------------------------------------------------------------------------

/// Run dedup targeting a specific `PalaceDb` instance (for MCP tool usage).
///
/// Returns a [`DedupStats`] describing what happened.
pub fn dedup_db(
    db: &mut PalaceDb,
    config: &DedupConfig,
) -> anyhow::Result<DedupStats> {
    let total_before = db.count();
    let all_entries = db.get_all(config.wing.as_deref(), None, usize::MAX);

    // Group by source_file.
    let mut source_groups: HashMap<String, Vec<QueryResult>> = HashMap::new();
    for entry in &all_entries {
        let source = entry
            .metadatas
            .first()
            .and_then(|m| m.get("source_file"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        // Apply source_pattern filter.
        if let Some(ref pattern) = config.source_pattern {
            if !source.to_lowercase().contains(&pattern.to_lowercase()) {
                continue;
            }
        }
        source_groups
            .entry(source.to_string())
            .or_default()
            .push(entry.clone());
    }

    // Filter to groups with enough drawers.
    let groups: Vec<(String, Vec<QueryResult>)> = source_groups
        .into_iter()
        .filter(|(_, entries)| entries.len() >= config.min_drawers_to_check)
        .collect();

    let mut total_kept = 0usize;
    let mut total_deleted = 0usize;
    let mut deleted_ids: Vec<String> = Vec::new();
    let mut source_summaries: Vec<SourceSummary> = Vec::new();

    for (source, entries) in &groups {
        let before = entries.len();
        let (kept_ids, delete_ids) = dedup_source_group_inner(entries, config.threshold);
        let after = kept_ids.len();
        let deleted = delete_ids.len();

        total_kept += after;
        total_deleted += deleted;

        source_summaries.push(SourceSummary {
            source: source.clone(),
            before,
            after,
            deleted,
        });

        if deleted > 0 {
            info!(
                "dedup: {}  {} -> {}  (-{})",
                &source[..source.len().min(50)],
                before,
                after,
                deleted,
            );
            if !config.dry_run {
                deleted_ids.extend(delete_ids);
            }
        }
    }

    // Apply deletions if not dry-run.
    if !config.dry_run && !deleted_ids.is_empty() {
        for id in &deleted_ids {
            if let Err(e) = db.delete_id(id) {
                warn!("dedup: failed to delete {}: {}", id, e);
            }
        }
        info!("dedup: deleted {} drawers", deleted_ids.len());
    }

    let total_after = if config.dry_run { total_before } else { db.count() };

    Ok(DedupStats {
        sources_checked: groups.len(),
        total_kept,
        total_deleted,
        palace_size_before: total_before,
        palace_size_after: total_after,
        deleted_ids,
        source_summaries,
    })
}

// ---------------------------------------------------------------------------
// Core dedup logic
// ---------------------------------------------------------------------------

/// Greedy dedup for one source group. Returns `(kept_ids, delete_ids)`.
///
/// Strategy:
/// 1. Sort by content length (longest first).
/// 2. For each candidate, compute cosine distance against every already-kept
///    drawer. If *any* kept drawer is closer than `threshold`, mark as dup.
/// 3. Keep the first (longest) drawer, delete shorter near-duplicates.
fn dedup_source_group_inner(
    entries: &[QueryResult],
    threshold: f64,
) -> (Vec<String>, Vec<String>) {
    // Build items with content length for sorting.
    let mut items: Vec<(usize, &QueryResult)> = entries
        .iter()
        .map(|e| {
            let content = e.documents.first().map(|s| s.as_str()).unwrap_or("");
            let len = content.len();
            (len, e)
        })
        .collect();
    items.sort_by_key(|b| std::cmp::Reverse(b.0));

    let mut kept_ids: Vec<String> = Vec::new();
    let mut kept_contents: Vec<String> = Vec::new();
    let mut delete_ids: Vec<String> = Vec::new();

    for (_, entry) in &items {
        let content = match entry.documents.first() {
            Some(s) => s.as_str(),
            None => {
                delete_ids.extend(entry.ids.clone());
                continue;
            }
        };

        // Skip empty/tiny drawers.
        if content.len() < MIN_CONTENT_LENGTH {
            delete_ids.extend(entry.ids.clone());
            continue;
        }

        if kept_ids.is_empty() {
            kept_ids.extend(entry.ids.clone());
            kept_contents.push(content.to_string());
            continue;
        }

        // Check against every kept item.
        let mut is_dup = false;
        for kept_content in &kept_contents {
            let dist = cosine_distance(content, kept_content);
            if dist < threshold {
                is_dup = true;
                break;
            }
        }

        if is_dup {
            delete_ids.extend(entry.ids.clone());
        } else {
            kept_ids.extend(entry.ids.clone());
            kept_contents.push(content.to_string());
        }
    }

    (kept_ids, delete_ids)
}

// ---------------------------------------------------------------------------
// Cosine distance (word-bag frequency vectors)
// ---------------------------------------------------------------------------

/// Compute cosine distance between two text strings using word-frequency
/// vectors (case-insensitive).
///
/// `distance = 1.0 - cosine_similarity`
///
/// Returns a value in `[0.0, 1.0]`. Lower means more similar.
pub fn cosine_distance(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 1.0;
    }

    let words_a: Vec<&str> = a.split_whitespace().collect();
    let words_b: Vec<&str> = b.split_whitespace().collect();

    if words_a.is_empty() || words_b.is_empty() {
        return 1.0;
    }

    // Build frequency maps (case-insensitive keys).
    let mut freq_a: HashMap<String, f64> = HashMap::new();
    for w in &words_a {
        *freq_a.entry(w.to_lowercase()).or_insert(0.0) += 1.0;
    }
    let mut freq_b: HashMap<String, f64> = HashMap::new();
    for w in &words_b {
        *freq_b.entry(w.to_lowercase()).or_insert(0.0) += 1.0;
    }

    // Collect vocabulary from the union of both sets of keys.
    let vocab: std::collections::HashSet<String> =
        freq_a.keys().cloned().chain(freq_b.keys().cloned()).collect();

    let mut dot = 0.0_f64;
    let mut mag_a = 0.0_f64;
    let mut mag_b = 0.0_f64;

    for w in &vocab {
        let fa = freq_a.get(w).copied().unwrap_or(0.0);
        let fb = freq_b.get(w).copied().unwrap_or(0.0);
        dot += fa * fb;
        mag_a += fa * fa;
        mag_b += fb * fb;
    }

    let norm_a = mag_a.sqrt();
    let norm_b = mag_b.sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 1.0;
    }

    let similarity = dot / (norm_a * norm_b);
    1.0 - similarity.clamp(0.0, 1.0)
}

/// Compute cosine distance between two dense vectors (embedding-based).
///
/// `distance = 1.0 - cosine_similarity`
///
/// Returns a value in `[0.0, 1.0]`. Lower = more similar.
pub fn cosine_distance_vectors(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 1.0;
    }

    let mut dot = 0.0_f64;
    let mut mag_a = 0.0_f64;
    let mut mag_b = 0.0_f64;

    for i in 0..a.len() {
        let fa = a[i] as f64;
        let fb = b[i] as f64;
        dot += fa * fb;
        mag_a += fa * fa;
        mag_b += fb * fb;
    }

    let norm_a = mag_a.sqrt();
    let norm_b = mag_b.sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 1.0;
    }

    let similarity = dot / (norm_a * norm_b);
    1.0 - similarity.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Statistics-only function
// ---------------------------------------------------------------------------

/// Show duplication statistics without making changes.
/// Prints human-readable output to stdout.
pub fn show_stats(palace_path: Option<&Path>) -> anyhow::Result<()> {
    let cfg = crate::config::Config::load()?;
    let palace_path = palace_path.unwrap_or(cfg.palace_path.as_path());
    let palace_db = PalaceDb::open(palace_path)?;

    let all_entries = palace_db.get_all(None, None, usize::MAX);
    let mut source_groups: HashMap<String, Vec<&QueryResult>> = HashMap::new();
    for entry in &all_entries {
        let source = entry
            .metadatas
            .first()
            .and_then(|m| m.get("source_file"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        source_groups
            .entry(source.to_string())
            .or_default()
            .push(entry);
    }

    let groups: Vec<(&String, &Vec<&QueryResult>)> = source_groups
        .iter()
        .filter(|(_, entries)| entries.len() >= MIN_DRAWERS_TO_CHECK)
        .collect();

    let total_drawers: usize = groups.iter().map(|(_, e)| e.len()).sum();

    println!(
        "\n  Sources with {}+ drawers: {}",
        MIN_DRAWERS_TO_CHECK,
        groups.len()
    );
    println!("  Total drawers in those sources: {}", total_drawers);

    println!("\n  Top 15 by drawer count:");
    let mut sorted: Vec<_> = groups.into_iter().collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.1.len()));
    for (src, entries) in sorted.iter().take(15) {
        println!("    {:4}  {}", entries.len(), &src[..src.len().min(65)]);
    }

    let estimated_dups: usize = sorted
        .iter()
        .filter(|(_, e)| e.len() > 20)
        .map(|(_, e)| (e.len() as f64 * 0.4) as usize)
        .sum();
    println!(
        "\n  Estimated duplicates (groups > 20): ~{}",
        estimated_dups
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Audit entry helpers
// ---------------------------------------------------------------------------

/// Build an audit-trail details map for a dedup operation.
pub fn build_audit_details(stats: &DedupStats) -> HashMap<String, serde_json::Value> {
    let mut details = HashMap::new();
    details.insert(
        "sources_checked".to_string(),
        serde_json::Value::Number(serde_json::Number::from(stats.sources_checked as u64)),
    );
    details.insert(
        "total_kept".to_string(),
        serde_json::Value::Number(serde_json::Number::from(stats.total_kept as u64)),
    );
    details.insert(
        "total_deleted".to_string(),
        serde_json::Value::Number(serde_json::Number::from(stats.total_deleted as u64)),
    );
    details.insert(
        "palace_size_before".to_string(),
        serde_json::Value::Number(serde_json::Number::from(stats.palace_size_before as u64)),
    );
    details.insert(
        "palace_size_after".to_string(),
        serde_json::Value::Number(serde_json::Number::from(stats.palace_size_after as u64)),
    );
    details
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // cosine_distance tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_cosine_distance_identical() {
        let d = cosine_distance("hello world", "hello world");
        assert!(
            d < 0.001,
            "identical strings should have near-zero distance, got {d}"
        );
    }

    #[test]
    fn test_cosine_distance_very_different() {
        let d = cosine_distance("abcdef", "ghijkl");
        assert!(
            d > 0.9,
            "very different strings should have high distance, got {d}"
        );
    }

    #[test]
    fn test_cosine_distance_partial_overlap() {
        let d = cosine_distance("hello world foo", "hello world bar");
        // 2/3 words in common -> dist should be ~0.2-0.4
        assert!(d > 0.1, "distance should be non-zero, got {d}");
        assert!(d < 0.8, "some overlap means distance < 0.8, got {d}");
    }

    #[test]
    fn test_cosine_distance_empty_strings() {
        assert_eq!(cosine_distance("", "hello"), 1.0);
        assert_eq!(cosine_distance("hello", ""), 1.0);
        assert_eq!(cosine_distance("", ""), 1.0);
    }

    #[test]
    fn test_cosine_distance_case_insensitive() {
        let d = cosine_distance("Hello World", "hello world");
        assert!(
            d < 0.001,
            "case difference should still match, got {d}"
        );
    }

    #[test]
    fn test_cosine_distance_repeated_words() {
        // Same vocabulary with different frequencies.
        let d = cosine_distance("foo foo bar", "foo bar bar");
        assert!(d > 0.0, "different frequencies should have non-zero distance");
        assert!(d < 1.0, "partial overlap should be < 1.0, got {d}");
    }

    // -----------------------------------------------------------------------
    // cosine_distance_vectors tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_cosine_distance_vectors_identical() {
        let v = vec![1.0, 0.0, 0.0];
        let d = cosine_distance_vectors(&v, &v);
        assert!(d < 0.001, "identical vectors should have distance ~0, got {d}");
    }

    #[test]
    fn test_cosine_distance_vectors_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let d = cosine_distance_vectors(&a, &b);
        assert!(
            (d - 1.0).abs() < 0.001,
            "orthogonal vectors should have distance 1, got {d}"
        );
    }

    #[test]
    fn test_cosine_distance_vectors_same_direction() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![2.0, 4.0, 6.0];
        let d = cosine_distance_vectors(&a, &b);
        assert!(
            d < 0.001,
            "parallel vectors should have distance ~0, got {d}"
        );
    }

    #[test]
    fn test_cosine_distance_vectors_empty() {
        assert_eq!(cosine_distance_vectors(&[], &[]), 1.0);
        assert_eq!(cosine_distance_vectors(&[1.0], &[]), 1.0);
    }

    #[test]
    fn test_cosine_distance_vectors_mismatched_length() {
        let d = cosine_distance_vectors(&[1.0, 0.0], &[1.0]);
        assert_eq!(d, 1.0, "mismatched dimensions should return 1.0");
    }

    // -----------------------------------------------------------------------
    // dedup_source_group_inner tests
    // -----------------------------------------------------------------------

    fn make_query_result(id: &str, content: &str) -> QueryResult {
        QueryResult {
            ids: vec![id.to_string()],
            documents: vec![content.to_string()],
            distances: vec![0.0],
            metadatas: vec![HashMap::new()],
        }
    }

    #[test]
    fn test_dedup_empty_group() {
        let (kept, deleted) = dedup_source_group_inner(&[], 0.15);
        assert!(kept.is_empty());
        assert!(deleted.is_empty());
    }

    #[test]
    fn test_dedup_single_entry() {
        let entries = vec![make_query_result(
            "id1",
            "hello world this is a test entry",
        )];
        let (kept, deleted) = dedup_source_group_inner(&entries, 0.15);
        assert_eq!(kept, vec!["id1"]);
        assert!(deleted.is_empty());
    }

    #[test]
    fn test_dedup_short_content_auto_deleted() {
        let entries = vec![make_query_result("id1", "short")]; // < MIN_CONTENT_LENGTH
        let (kept, deleted) = dedup_source_group_inner(&entries, 0.15);
        assert!(kept.is_empty());
        assert_eq!(deleted, vec!["id1"]);
    }

    #[test]
    fn test_dedup_near_duplicates() {
        // Two very similar documents; the longest (first) should be kept.
        let entries = vec![
            make_query_result(
                "id1",
                "this is a long document with quite a few words in it",
            ),
            make_query_result("id2", "this is a long document with quite a few words"),
        ];
        let (kept, deleted) = dedup_source_group_inner(&entries, 0.15);
        assert_eq!(kept, vec!["id1"]);
        assert_eq!(deleted, vec!["id2"]);
    }

    #[test]
    fn test_dedup_distinct_content_all_kept() {
        let entries = vec![
            make_query_result(
                "id1",
                "the quick brown fox jumps over the lazy dog",
            ),
            make_query_result(
                "id2",
                "python is a programming language that is widely used",
            ),
        ];
        let (kept, deleted) = dedup_source_group_inner(&entries, 0.15);
        assert_eq!(kept.len(), 2);
        assert!(deleted.is_empty());
    }

    #[test]
    fn test_dedup_longest_kept_first() {
        // The longest entry should always be kept.
        let entries = vec![
            make_query_result("short", "short text"),
            make_query_result(
                "long",
                "this is a much longer document with a lot more words \
                 and should be preferred as the canonical copy \
                 because it contains more information",
            ),
        ];
        let (kept, _) = dedup_source_group_inner(&entries, 0.15);
        // Short one is < 20 chars -> auto-deleted
        assert!(!kept.contains(&"short".to_string()));
    }

    #[test]
    fn test_dedup_three_entries_two_kept() {
        let entries = vec![
            make_query_result("id1", "apple banana cherry date"),
            make_query_result(
                "id2",
                "apple banana cherry date elderberry fig",
            ),
            make_query_result(
                "id3",
                "the distant ocean waves crash against the rocky shoreline at dusk",
            ),
        ];
        let (kept, deleted) = dedup_source_group_inner(&entries, 0.3);
        // id2 is longest, id1 is near-dup of id2, id3 is distinct
        assert_eq!(
            kept.len(),
            2,
            "should keep id2 and id3, got {:?}",
            kept
        );
        assert_eq!(
            deleted.len(),
            1,
            "should delete id1, got {:?}",
            deleted
        );
    }

    // -----------------------------------------------------------------------
    // DedupConfig tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_dedup_config_default() {
        let cfg = DedupConfig::default();
        assert!((cfg.threshold - 0.15).abs() < 1e-10);
        assert!(cfg.dry_run);
        assert!(cfg.wing.is_none());
        assert_eq!(cfg.min_drawers_to_check, 5);
    }

    #[test]
    fn test_dedup_config_threshold_from_env() {
        let _guard = crate::test_env_lock().lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("MEMPALACE_DEDUP_THRESHOLD", "0.25");
        assert!((DedupConfig::threshold_from_env() - 0.25).abs() < 1e-10);
        std::env::remove_var("MEMPALACE_DEDUP_THRESHOLD");
    }

    #[test]
    fn test_dedup_config_threshold_from_env_fallback() {
        let _guard = crate::test_env_lock().lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("MEMPALACE_DEDUP_THRESHOLD");
        assert!((DedupConfig::threshold_from_env() - DEFAULT_THRESHOLD).abs() < 1e-10);
    }

    // -----------------------------------------------------------------------
    // DedupStats / build_audit_details tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_audit_details() {
        let stats = DedupStats {
            sources_checked: 3,
            total_kept: 10,
            total_deleted: 5,
            palace_size_before: 100,
            palace_size_after: 95,
            deleted_ids: vec![],
            source_summaries: vec![],
        };
        let details = build_audit_details(&stats);
        assert_eq!(
            details.get("sources_checked").and_then(|v| v.as_u64()),
            Some(3)
        );
        assert_eq!(
            details.get("total_deleted").and_then(|v| v.as_u64()),
            Some(5)
        );
    }
}
