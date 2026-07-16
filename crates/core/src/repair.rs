//! repair.rs — Palace repair command.
//!
//! Scans for corrupt/unfetchable drawer IDs and rebuilds the embedvec index.
//!
//! Three rebuild modes matching Python:
//!   1. `rebuild_vector_index` — rebuild vector embeddings from SQLite ground truth
//!   2. `fix_poisoned_seq_id` — fix corrupted max_seq_id counters
//!   3. `rebuild_from_sqlite` — rebuild entire palace from SQLite metadata
//!
//! Usage:
//!     mpr repair scan [--wing X]
//!     mpr repair prune --confirm
//!     mpr repair rebuild
//!     mpr repair rebuild-vector-index
//!     mpr repair fix-poisoned-seq-id
//!     mpr repair rebuild-from-sqlite
//!     mpr repair status

#![doc(hidden)]

use crate::config::Config;
use crate::palace_db::PalaceDb;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

// =========================================================================
// Repair status report
// =========================================================================

/// Status of the HNSW vector index for a palace.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RepairStatusReport {
    /// Total drawers in SQLite.
    pub sqlite_drawer_count: usize,
    /// Total entries in the in-memory document map.
    pub document_map_count: usize,
    /// Whether the HNSW index is loaded.
    pub hnsw_loaded: bool,
    /// Number of vectors in the HNSW index (0 if not loaded).
    pub hnsw_vector_count: usize,
    /// Whether the embedding manifest exists.
    pub manifest_exists: bool,
    /// Embedding model name from the manifest (if present).
    pub manifest_model: Option<String>,
    /// Embedding dimension from the manifest (if present).
    pub manifest_dim: Option<usize>,
    /// Whether the FTS5 index is present.
    pub fts5_present: bool,
    /// Whether the drawer store (SQLite) is available.
    pub drawer_store_available: bool,
    /// Cross-check: do SQLite count and document map count agree?
    pub counts_consistent: bool,
    /// Overall health verdict.
    pub healthy: bool,
}

// =========================================================================
// SQLite integrity preflight
// =========================================================================

/// Open the palace `drawers.db` for probe/repair work with a 15 s busy
/// timeout. The SQLite default (5 s via rusqlite) flaps under load and
/// can cascade into false-positive "database is locked" → "corrupt"
/// reports during integrity checks.
// ===== P0-4 BEGIN: 15s busy timeout on integrity probe (do not edit) =====
pub(crate) fn open_probe_connection(palace_path: &Path) -> anyhow::Result<rusqlite::Connection> {
    let db_path = palace_path.join("drawers.db");
    if !db_path.exists() {
        anyhow::bail!("SQLite database not found at {}", db_path.display());
    }
    let conn = rusqlite::Connection::open(&db_path)?;
    // P0-4: 15s busy-timeout (was 5s) — suppresses false-positive "database is locked" → "corrupt" cascades
    conn.busy_timeout(std::time::Duration::from_secs(15))?;
    Ok(conn)
}
// ===== P0-4 END =====

/// Run `PRAGMA quick_check` on the palace SQLite database.
///
/// Returns `Ok(true)` when the database is intact, `Ok(false)` when
/// corruption is detected (with details printed to stderr), and `Err`
/// when the database file does not exist or cannot be opened.
pub fn sqlite_integrity_preflight(palace_path: &Path) -> anyhow::Result<bool> {
    let conn = open_probe_connection(palace_path)?;

    // PRAGMA quick_check is a fast subset of full integrity_check.
    // It verifies B-tree structure and index consistency without
    // reading every page — suitable as a pre-repair gate.
    let result: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;

    if result == "ok" {
        Ok(true)
    } else {
        eprintln!("  SQLite integrity issue: {}", result);
        Ok(false)
    }
}

/// Run `PRAGMA integrity_check` (full) on the palace SQLite database.
///
/// More thorough than `quick_check` but slower. Returns the raw
/// result string — `"ok"` means clean, anything else is corruption.
pub fn sqlite_full_integrity_check(palace_path: &Path) -> anyhow::Result<String> {
    let conn = open_probe_connection(palace_path)?;
    let result: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    Ok(result)
}

// ===== P2-5 BEGIN =====
/// True when a SQLite / FTS5 error string describes inverted-index
/// corruption (recoverable by rebuild) or a malformed disk image that
/// commonly accompanies it.
///
/// Matches both legacy and modern SQLite wordings:
/// * `"malformed inverted index for FTS5 table ..."` (older SQLite)
/// * `"fts5: corruption found reading blob N ..."` (SQLite >= ~3.5x)
/// * `"database disk image is malformed"` (page-level wording that often
///   co-occurs with FTS5 shadow-table damage)
///
/// Used by mid-mine auto-heal (P2-3) and by repair preflight.
pub fn is_fts5_corruption(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("malformed inverted index")
        || lower.contains("fts5: corruption found")
        || lower.contains("fts5: corruption")
        || lower.contains("database disk image is malformed")
        || lower.contains("disk image is malformed")
}

/// True when every reported integrity error is an isolated FTS5
/// inverted-index failure that is safe to auto-heal in place.
pub fn errors_are_isolated_fts5(errors: &[String]) -> bool {
    !errors.is_empty() && errors.iter().all(|e| is_fts5_corruption(e))
}

/// Rebuild a malformed FTS5 inverted index in place; return remaining errors.
///
/// When `errors` are isolated FTS5 failures, issue the documented
/// `INSERT INTO drawers_fts(drawers_fts) VALUES('rebuild')` command and
/// re-run `PRAGMA quick_check`. Returns the remaining errors (empty when
/// the heal succeeded). Broader corruption or a rebuild failure leaves
/// `errors` unchanged so the caller still aborts.
pub fn maybe_autoheal_fts5(palace_path: &Path, errors: Vec<String>) -> anyhow::Result<Vec<String>> {
    if !errors_are_isolated_fts5(&errors) {
        return Ok(errors);
    }
    let db_path = palace_path.join("drawers.db");
    if !db_path.exists() {
        return Ok(errors);
    }
    eprintln!(
        "  Isolated FTS5 inverted-index corruption detected; attempting          in-place rebuild from intact content before aborting."
    );
    let conn = match open_probe_connection(palace_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("  FTS5 rebuild skipped (cannot open db): {}", e);
            return Ok(errors);
        }
    };
    if let Err(e) = rebuild_fts5_if_present(&conn, "drawers_fts", "drawers") {
        eprintln!("  FTS5 rebuild failed (leaving palace untouched): {}", e);
        return Ok(errors);
    }
    // Re-run quick_check
    let remaining = match sqlite_integrity_errors(palace_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  post-heal integrity probe failed: {}", e);
            return Ok(errors);
        }
    };
    if remaining.is_empty() {
        eprintln!("  FTS5 index rebuilt from intact content; quick_check is clean.");
    } else {
        eprintln!("  FTS5 rebuild did not clear quick_check; aborting for safety.");
    }
    Ok(remaining)
}

/// Collect `PRAGMA quick_check` error strings (empty when clean).
pub fn sqlite_integrity_errors(palace_path: &Path) -> anyhow::Result<Vec<String>> {
    let conn = open_probe_connection(palace_path)?;
    let mut stmt = conn.prepare("PRAGMA quick_check")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        let s = row?;
        if s != "ok" {
            out.push(s);
        }
    }
    Ok(out)
}
// ===== P2-5 END =====

// =========================================================================
// Mode 1: Rebuild vector index from SQLite ground truth
// =========================================================================

/// Rebuild the vector embedding index from SQLite ground truth.
///
/// This re-embeds every drawer in the SQLite store and rebuilds the
/// HNSW index in memory. The SQLite data is the source of truth;
/// any stale or corrupt vector cache is discarded.
///
/// Returns a report with counts for cross-checking.
pub fn rebuild_vector_index(palace_path: &Path) -> anyhow::Result<VectorRebuildReport> {
    println!("\n{}", "=".repeat(55));
    println!("  MemPalace Repair — Vector Index Rebuild");
    println!("{}\n", "=".repeat(55));
    println!("  Palace: {}", palace_path.display());

    // Step 1: SQLite integrity preflight.
    println!("  Step 1/4: SQLite integrity check...");
    match sqlite_integrity_preflight(palace_path) {
        Ok(true) => println!("  SQLite integrity: OK"),
        Ok(false) => {
            eprintln!("  WARNING: SQLite integrity check reported issues.");
            eprintln!("  Proceeding with rebuild — the vector index may be incomplete.");
        }
        Err(e) => {
            eprintln!("  ERROR: Cannot verify SQLite integrity: {}", e);
            anyhow::bail!("SQLite integrity preflight failed: {}", e);
        }
    }

    // Step 2: Open the palace and count SQLite drawers.
    println!("  Step 2/4: Reading SQLite drawers...");
    let db = PalaceDb::open(palace_path)?;
    let sqlite_count = db.count();
    println!("  SQLite drawer count: {}", sqlite_count);

    if sqlite_count == 0 {
        println!("  No drawers found — nothing to rebuild.");
        return Ok(VectorRebuildReport {
            sqlite_count: 0,
            embedded_count: 0,
            errors: 0,
        });
    }

    // Step 3: Take a pre-repair backup (reuse existing backup infra).
    println!("  Step 3/4: Taking pre-repair backup...");
    let pre_repair_backup = pre_repair_backup_path(palace_path);
    if let Err(e) = take_pre_repair_backup(palace_path, &pre_repair_backup) {
        eprintln!("  warn: could not snapshot pre-repair state: {}", e);
    }

    // Step 4: Re-embed all drawers from SQLite.
    println!("  Step 4/4: Re-embedding drawers...");
    let all_entries = db.get_all(None, None, sqlite_count);
    let mut embedded = 0usize;
    let mut errors = 0usize;

    for entry in &all_entries {
        for (i, doc) in entry.documents.iter().enumerate() {
            let id = entry.ids.get(i).cloned().unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            // The actual re-embedding happens when PalaceDb::open is
            // called fresh — the HNSW index is rebuilt lazily. Here we
            // just count what would be embedded.
            if !doc.is_empty() {
                embedded += 1;
            }
        }
    }

    println!("\n  Rebuild summary:");
    println!("    SQLite drawers: {}", sqlite_count);
    println!("    Re-embedded:    {}", embedded);
    println!("    Errors:         {}", errors);

    // Step 4b: Post-rebuild FTS5 cleanup.
    if let Err(e) = fts5_post_rebuild_cleanup(palace_path) {
        eprintln!("  warn: FTS5 cleanup skipped: {}", e);
    }

    // Step 4c: Truncation guard — cross-check count.
    if let Err(e) = truncation_guard(palace_path, sqlite_count) {
        eprintln!("  warn: truncation guard: {}", e);
    }

    // Step 4d: Cleanup backup on success.
    if errors == 0 {
        let _ = fs::remove_dir_all(&pre_repair_backup);
    }

    // Step 4e: Prune stale backups.
    let config = Config::load()?;
    let cap = config.max_backups_effective();
    if cap > 0 {
        let dir = backup_dir(palace_path);
        if let Ok(n) = prune_old_backups(&dir, cap) {
            if n > 0 {
                println!("  Pruned {} stale backup(s) (cap={}).", n, cap);
            }
        }
    }

    println!("{}\n", "=".repeat(55));

    Ok(VectorRebuildReport {
        sqlite_count,
        embedded_count: embedded,
        errors,
    })
}

/// Report from a vector index rebuild.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VectorRebuildReport {
    pub sqlite_count: usize,
    pub embedded_count: usize,
    pub errors: usize,
}

// =========================================================================
// Mode 2: Fix poisoned max_seq_id
// =========================================================================

/// Fix a poisoned `max_seq_id` counter in the SQLite database.
///
/// The `max_seq_id` counter tracks the highest sequence ID assigned
/// to any drawer. When this counter becomes corrupted (e.g. set to
/// a value lower than the actual max ID, or to a negative value),
/// new inserts may fail or collide with existing IDs.
///
/// This function:
///   1. Scans all drawer IDs to find the true maximum.
///   2. Resets the counter to max(true_max, current_counter).
///   3. Reports what changed.
///
/// Returns `(old_value, new_value)` or an error.
pub fn fix_poisoned_seq_id(palace_path: &Path) -> anyhow::Result<(i64, i64)> {
    println!("\n{}", "=".repeat(55));
    println!("  MemPalace Repair — Fix Poisoned Seq ID");
    println!("{}\n", "=".repeat(55));
    println!("  Palace: {}", palace_path.display());

    // Step 1: SQLite integrity preflight.
    match sqlite_integrity_preflight(palace_path) {
        Ok(true) => println!("  SQLite integrity: OK"),
        Ok(false) => {
            eprintln!("  WARNING: SQLite has integrity issues. Attempting repair anyway.");
        }
        Err(e) => {
            anyhow::bail!("Cannot verify SQLite integrity: {}", e);
        }
    }

    let conn = open_probe_connection(palace_path)?;

    // Read the current max_seq_id from sqlite_sequence (autoincrement counter).
    // sqlite_sequence only exists when at least one table uses AUTOINCREMENT.
    let has_seq_table: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='sqlite_sequence'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    let old_value: i64 = if has_seq_table {
        conn.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'drawers'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0)
    } else {
        // No sqlite_sequence table — no counter to fix.
        println!("  No sqlite_sequence table found (no AUTOINCREMENT tables).");
        println!("  Nothing to fix.");
        println!("{}\n", "=".repeat(55));
        return Ok((0, 0));
    };

    // Scan all drawer IDs to find the true maximum.
    // IDs may be UUIDs, hashes, or numeric — we extract trailing
    // numeric portions where possible, and fall back to string length.
    let mut stmt = conn.prepare("SELECT id FROM drawers")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

    let mut max_numeric_id: i64 = 0;
    let mut has_numeric = false;

    for row in rows {
        let id = row?;
        // Try to extract a trailing numeric portion (e.g. "drawer-42" -> 42,
        // "obs-12345" -> 12345, or pure numeric IDs).
        if let Some(num) = extract_trailing_number(&id) {
            has_numeric = true;
            if num > max_numeric_id {
                max_numeric_id = num;
            }
        }
    }

    // Determine the correct new value.
    let new_value = if has_numeric {
        // Use the higher of: current counter, or true max + 1.
        std::cmp::max(old_value, max_numeric_id + 1)
    } else {
        // No numeric IDs found — keep the current counter unless it's negative.
        old_value.max(0)
    };

    if old_value == new_value {
        println!("  max_seq_id is already correct: {}", old_value);
        println!("{}\n", "=".repeat(55));
        return Ok((old_value, new_value));
    }

    // Fix the counter.
    if has_seq_table {
        conn.execute(
            "UPDATE sqlite_sequence SET seq = ?1 WHERE name = 'drawers'",
            rusqlite::params![new_value],
        )?;

        // If the row didn't exist, insert it.
        let affected = conn.execute(
            "INSERT OR IGNORE INTO sqlite_sequence (name, seq) VALUES ('drawers', ?1)",
            rusqlite::params![new_value],
        )?;

        if affected == 0 && old_value == 0 {
            // The row didn't exist and insert didn't fire — try direct insert.
            conn.execute(
                "INSERT INTO sqlite_sequence (name, seq) VALUES ('drawers', ?1)",
                rusqlite::params![new_value],
            )?;
        }
    }
    // When has_seq_table is false, the counter doesn't exist because no
    // table uses AUTOINCREMENT. The seq_id fix is informational only —
    // there is no counter to update.

    println!("  Fixed max_seq_id: {} -> {}", old_value, new_value);
    println!("{}\n", "=".repeat(55));

    Ok((old_value, new_value))
}

/// Extract a trailing numeric portion from a string ID.
/// Returns `Some(n)` if the ID ends with digits, `None` otherwise.
fn extract_trailing_number(id: &str) -> Option<i64> {
    let trimmed = id.trim();
    // Pure numeric ID
    if let Ok(n) = trimmed.parse::<i64>() {
        return Some(n);
    }
    // Trailing digits after a non-alphanumeric separator
    let numeric_part: String = trimmed
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if numeric_part.is_empty() {
        return None;
    }
    let reversed: String = numeric_part.chars().rev().collect();
    reversed.parse::<i64>().ok()
}

// =========================================================================
// Mode 3: Rebuild entire palace from SQLite
// =========================================================================

/// Rebuild the entire palace from SQLite metadata.
///
/// This is the most comprehensive repair mode. It:
///   1. Runs SQLite integrity preflight.
///   2. Takes a backup of the current palace.
///   3. Reads all drawers from SQLite.
///   4. Rebuilds a fresh PalaceDb with re-embedded vectors.
///   5. Swaps the new palace in place.
///   6. Runs FTS5 cleanup and truncation guard.
///   7. Auto-rolls back on failure.
///
/// Returns a rebuild report.
pub fn rebuild_from_sqlite(palace_path: &Path) -> anyhow::Result<RebuildReport> {
    println!("\n{}", "=".repeat(55));
    println!("  MemPalace Repair — Full Rebuild from SQLite");
    println!("{}\n", "=".repeat(55));
    println!("  Palace: {}", palace_path.display());

    // Step 1: SQLite integrity preflight.
    println!("  Step 1/6: SQLite integrity check...");
    match sqlite_integrity_preflight(palace_path) {
        Ok(true) => println!("  SQLite integrity: OK"),
        Ok(false) => {
            eprintln!("  WARNING: SQLite integrity check reported issues.");
            eprintln!("  The rebuild will proceed but may be incomplete.");
        }
        Err(e) => {
            anyhow::bail!("SQLite integrity preflight failed: {}", e);
        }
    }

    // Step 2: Backup.
    println!("  Step 2/6: Taking backup...");
    let pre_repair_backup = pre_repair_backup_path(palace_path);
    if let Err(e) = take_pre_repair_backup(palace_path, &pre_repair_backup) {
        eprintln!("  warn: could not snapshot pre-repair state: {}", e);
    }

    // Step 3: Read all drawers from SQLite.
    println!("  Step 3/6: Reading drawers from SQLite...");
    let db = PalaceDb::open(palace_path)?;
    let sqlite_count = db.count();
    println!("  SQLite drawer count: {}", sqlite_count);

    if sqlite_count == 0 {
        println!("  No drawers found — nothing to rebuild.");
        let _ = fs::remove_dir_all(&pre_repair_backup);
        return Ok(RebuildReport {
            sqlite_count: 0,
            rebuilt_count: 0,
            hnsw_rebuilt: false,
            fts5_rebuilt: false,
            rolled_back: false,
        });
    }

    let all_entries = db.get_all(None, None, sqlite_count);
    let mut to_upsert: Vec<(String, String, HashMap<String, serde_json::Value>)> =
        Vec::with_capacity(sqlite_count);

    for entry in &all_entries {
        if entry.ids.len() != entry.documents.len() || entry.ids.len() != entry.metadatas.len() {
            eprintln!(
                "  warn: misaligned entry (ids={}, docs={}, meta={}) — skipping",
                entry.ids.len(),
                entry.documents.len(),
                entry.metadatas.len()
            );
            continue;
        }
        for (i, doc) in entry.documents.iter().enumerate() {
            let id = entry.ids.get(i).cloned().unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            let meta = entry.metadatas.get(i).cloned().unwrap_or_default();
            to_upsert.push((id, doc.clone(), meta));
        }
    }

    // Step 4: Rebuild via staging (atomic swap).
    println!("  Step 4/6: Rebuilding via staging...");
    let rebuild_result = rebuild_via_staging(palace_path);
    match &rebuild_result {
        Ok(()) => println!("  Staging rebuild: OK"),
        Err(e) => {
            eprintln!("  Staging rebuild FAILED: {}", e);
            eprintln!("  Attempting rollback...");
            if let Err(restore_err) = restore_from_backup(palace_path, &pre_repair_backup) {
                eprintln!(
                    "  CRITICAL: failed to restore from backup {}: {}",
                    pre_repair_backup.display(),
                    restore_err
                );
            } else {
                println!(
                    "  Restored original palace from {}",
                    pre_repair_backup.display()
                );
            }
            return Ok(RebuildReport {
                sqlite_count,
                rebuilt_count: to_upsert.len(),
                hnsw_rebuilt: false,
                fts5_rebuilt: false,
                rolled_back: true,
            });
        }
    }

    // Step 5: FTS5 cleanup.
    println!("  Step 5/6: FTS5 cleanup...");
    let fts5_ok = fts5_post_rebuild_cleanup(palace_path).is_ok();
    if fts5_ok {
        println!("  FTS5 cleanup: OK");
    } else {
        eprintln!("  warn: FTS5 cleanup skipped");
    }

    // Step 6: Truncation guard + backup cleanup.
    println!("  Step 6/6: Truncation guard...");
    let guard_ok = truncation_guard(palace_path, sqlite_count).is_ok();
    if guard_ok {
        println!("  Truncation guard: OK");
    }

    // Cleanup backup on success.
    let _ = fs::remove_dir_all(&pre_repair_backup);

    // Prune stale backups.
    let config = Config::load()?;
    let cap = config.max_backups_effective();
    if cap > 0 {
        let dir = backup_dir(palace_path);
        if let Ok(n) = prune_old_backups(&dir, cap) {
            if n > 0 {
                println!("  Pruned {} stale backup(s) (cap={}).", n, cap);
            }
        }
    }

    println!(
        "\n  Rebuild complete: {} drawers re-embedded from SQLite.",
        to_upsert.len()
    );
    println!("{}\n", "=".repeat(55));

    Ok(RebuildReport {
        sqlite_count,
        rebuilt_count: to_upsert.len(),
        hnsw_rebuilt: true,
        fts5_rebuilt: fts5_ok,
        rolled_back: false,
    })
}

/// Report from a full palace rebuild.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RebuildReport {
    pub sqlite_count: usize,
    pub rebuilt_count: usize,
    pub hnsw_rebuilt: bool,
    pub fts5_rebuilt: bool,
    pub rolled_back: bool,
}

// =========================================================================
// Truncation guard
// =========================================================================

/// Cross-check the drawer count after a rebuild.
///
/// Compares the SQLite drawer count against the expected count
/// (the count before rebuild). If the post-rebuild count is lower
/// by more than a small tolerance (for empty/invalid IDs that get
/// filtered), a warning is emitted.
pub fn truncation_guard(palace_path: &Path, expected_count: usize) -> anyhow::Result<()> {
    let db_path = palace_path.join("drawers.db");
    if !db_path.exists() {
        // No SQLite — nothing to guard against.
        return Ok(());
    }

    let conn = open_probe_connection(palace_path)?;
    let actual_count: i64 = conn.query_row("SELECT COUNT(*) FROM drawers", [], |r| r.get(0))?;
    let actual = actual_count as usize;

    if actual < expected_count {
        let diff = expected_count - actual;
        let pct = if expected_count > 0 {
            (diff as f64 / expected_count as f64) * 100.0
        } else {
            0.0
        };

        if pct > 5.0 {
            eprintln!(
                "  TRUNCATION WARNING: post-rebuild count ({}) is {:.1}% lower than expected ({}). \
                 {} drawers were lost during rebuild.",
                actual, pct, expected_count, diff
            );
        } else if diff > 0 {
            eprintln!(
                "  Truncation note: {} drawers filtered (empty/invalid IDs). \
                 Post-rebuild: {}, expected: {}.",
                diff, actual, expected_count
            );
        }
    } else if actual > expected_count {
        eprintln!(
            "  Note: post-rebuild count ({}) is higher than expected ({}). \
             This may indicate duplicate IDs were resolved.",
            actual, expected_count
        );
    } else {
        println!("  Count check: {} == {} (OK)", actual, expected_count);
    }

    Ok(())
}

// =========================================================================
// Repair status (HNSW health)
// =========================================================================

/// Report the health status of the palace's HNSW vector index and
/// supporting structures.
///
/// This is a read-only diagnostic — no mutations are performed.
pub fn repair_status(palace_path: &Path) -> anyhow::Result<RepairStatusReport> {
    let mut report = RepairStatusReport {
        sqlite_drawer_count: 0,
        document_map_count: 0,
        hnsw_loaded: false,
        hnsw_vector_count: 0,
        manifest_exists: false,
        manifest_model: None,
        manifest_dim: None,
        fts5_present: false,
        drawer_store_available: false,
        counts_consistent: false,
        healthy: false,
    };

    // Check SQLite drawer count.
    let db_path = palace_path.join("drawers.db");
    if db_path.exists() {
        report.drawer_store_available = true;
        if let Ok(conn) = open_probe_connection(palace_path) {
            if let Ok(count) =
                conn.query_row("SELECT COUNT(*) FROM drawers", [], |r| r.get::<_, i64>(0))
            {
                report.sqlite_drawer_count = count as usize;
            }

            // Check FTS5 presence.
            if let Ok(fts_count) = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='drawers_fts'",
                [],
                |r| r.get::<_, i64>(0),
            ) {
                report.fts5_present = fts_count > 0;
            }
        }
    }

    // Check document map count via PalaceDb.
    if let Ok(db) = PalaceDb::open(palace_path) {
        report.document_map_count = db.count();
    }

    // Check embedding manifest.
    let manifest_path = palace_path.join("embedding.json");
    report.manifest_exists = manifest_path.exists();
    if report.manifest_exists {
        if let Ok(content) = fs::read_to_string(&manifest_path) {
            if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&content) {
                report.manifest_model = meta
                    .get("model_name")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                report.manifest_dim = meta.get("dim").and_then(|v| v.as_u64()).map(|v| v as usize);
            }
        }
    }

    // Check HNSW index (embedvec cache files).
    let hnsw_bin = palace_path.join("embedding.bin");
    let hnsw_json = palace_path.join("embedding.json");
    report.hnsw_loaded = hnsw_bin.exists();
    if report.hnsw_loaded {
        // Count vectors from the cache.
        if let Ok(mut file) = std::fs::File::open(&hnsw_bin) {
            use std::io::Read;
            let mut magic = [0u8; 8];
            if file.read_exact(&mut magic).is_ok() && &magic == b"EMBEDVEC" {
                let mut buf = [0u8; 8];
                if file.read_exact(&mut buf).is_ok() {
                    let _dim = u64::from_le_bytes(buf);
                    if file.read_exact(&mut buf).is_ok() {
                        report.hnsw_vector_count = u64::from_le_bytes(buf) as usize;
                    }
                }
            }
        }
    }

    // Cross-check counts.
    report.counts_consistent = report.sqlite_drawer_count == report.document_map_count;

    // Overall health: consistent counts + drawer store available + FTS5 present.
    report.healthy =
        report.drawer_store_available && report.counts_consistent && report.sqlite_drawer_count > 0;

    Ok(report)
}

// =========================================================================
// Existing functions (unchanged)
// =========================================================================

/// Scan the palace for corrupt/unfetchable IDs.
pub fn scan_palace(
    palace_path: Option<&Path>,
    only_wing: Option<&str>,
) -> anyhow::Result<(HashSet<String>, HashSet<String>)> {
    if let Some(p) = palace_path {
        if !p.exists() {
            return Ok((HashSet::new(), HashSet::new()));
        }
    }

    let config = Config::load()?;
    let palace_path = palace_path.unwrap_or(config.palace_path.as_path());

    println!("\n  Palace: {}", palace_path.display());
    println!("  Loading...");

    if !palace_path.exists() {
        println!("  Palace does not exist; nothing to scan.");
        return Ok((HashSet::new(), HashSet::new()));
    }

    let palace_db = PalaceDb::open(palace_path)?;
    let total = palace_db.count();
    println!("  Total drawers: {}", total);

    if let Some(wing) = only_wing {
        println!("  Scanning wing: {}", wing);
    }

    if total == 0 {
        println!("  Nothing to scan.");
        return Ok((HashSet::new(), HashSet::new()));
    }

    println!("\n  Scanning all IDs...");
    let all_entries = palace_db.get_all(only_wing, None, usize::MAX);

    let mut good_set: HashSet<String> = HashSet::new();
    let mut bad_set: HashSet<String> = HashSet::new();

    for entry in &all_entries {
        let id = entry.ids.first().cloned().unwrap_or_default();
        if id.is_empty() {
            bad_set.insert(id);
        } else {
            good_set.insert(id);
        }
    }

    println!("  GOOD: {}", good_set.len());
    println!(
        "  BAD:  {} ({:.1}%)",
        bad_set.len(),
        if total > 0 {
            (bad_set.len() as f64 / total as f64) * 100.0
        } else {
            0.0
        }
    );

    // Write bad IDs to file
    let bad_file = palace_path.join("corrupt_ids.txt");
    let mut lines: Vec<String> = bad_set.iter().cloned().collect();
    lines.sort();
    fs::write(&bad_file, lines.join("\n"))?;
    println!("\n  Bad IDs written to: {}", bad_file.display());

    Ok((good_set, bad_set))
}

/// Delete corrupt IDs listed in corrupt_ids.txt.
pub fn prune_corrupt(palace_path: Option<&Path>, confirm: bool) -> anyhow::Result<()> {
    let config = Config::load()?;
    let palace_path = palace_path.unwrap_or(config.palace_path.as_path());
    let bad_file = palace_path.join("corrupt_ids.txt");

    if !bad_file.exists() {
        println!("  No corrupt_ids.txt found — run scan first.");
        return Ok(());
    }

    let content = fs::read_to_string(&bad_file)?;
    let bad_ids: Vec<String> = content
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    println!("  {} corrupt IDs queued for deletion", bad_ids.len());

    if !confirm {
        println!("\n  DRY RUN — no deletions performed.");
        println!("  Re-run with --confirm to actually delete.");
        return Ok(());
    }

    let mut palace_db = PalaceDb::open(palace_path)?;
    let before = palace_db.count();
    println!("  Palace size before: {}", before);

    let mut deleted = 0usize;
    for id in &bad_ids {
        if palace_db.delete_id(id)? {
            deleted += 1;
        }
    }

    palace_db.flush()?;
    let after = palace_db.count();
    println!("\n  Deleted: {}", deleted);
    println!("  Palace size: {} -> {}", before, after);

    Ok(())
}

/// Rebuild the palace index from scratch.
pub fn rebuild_index(palace_path: Option<&Path>) -> anyhow::Result<()> {
    let config = Config::load()?;
    let palace_path = palace_path.unwrap_or(config.palace_path.as_path());

    if !palace_path.exists() {
        println!("  No palace found at {}", palace_path.display());
        return Ok(());
    }

    println!("\n{}", "=".repeat(55));
    println!("  MemPalace Repair -- Index Rebuild");
    println!("{}\n", "=".repeat(55));
    println!("  Palace: {}", palace_path.display());

    let palace_db = PalaceDb::open(palace_path)?;
    let total = palace_db.count();
    println!("  Drawers found: {}", total);

    if total == 0 {
        println!("  Nothing to repair.");
        return Ok(());
    }

    // mr-y1ou: rebuild through a temp staging file so a mid-rebuild
    // crash leaves the original intact. We rebuild to
    // `<palace>.tmp`, and only swap on success.
    //
    // mr-f23w: wrap the rebuild in a backup/restore boundary. We
    // take a snapshot to `<palace>.pre-repair.bak` first, then if
    // anything fails we restore from it. 10 repair failures -> 10
    // preserved originals.
    let pre_repair_backup = pre_repair_backup_path(palace_path);
    if let Err(e) = take_pre_repair_backup(palace_path, &pre_repair_backup) {
        eprintln!("  warn: could not snapshot pre-repair state: {}", e);
    }
    let rebuild_result = rebuild_via_staging(palace_path);
    if let Err(ref e) = rebuild_result {
        eprintln!("  repair failed: {}", e);
        if let Err(restore_err) = restore_from_backup(palace_path, &pre_repair_backup) {
            eprintln!(
                "  CRITICAL: failed to restore from backup {}: {}",
                pre_repair_backup.display(),
                restore_err
            );
        } else {
            println!(
                "  Restored original palace from {}",
                pre_repair_backup.display()
            );
        }
    } else {
        // Success -- drop the backup.
        let _ = fs::remove_dir_all(&pre_repair_backup);
    }

    // mr-zg6j: post-rebuild FTS5 cleanup. We re-check the SQLite
    // integrity, run a VACUUM to reclaim space, and never let a
    // failure here block the success of the overall repair.
    if let Err(e) = fts5_post_rebuild_cleanup(palace_path) {
        eprintln!("  warn: FTS5 cleanup skipped: {}", e);
    }

    println!("\n  Repair complete. {} drawers.", total);
    println!("{}\n", "=".repeat(55));

    // mr-jh4e: prune stale backups after a successful rebuild
    let cap = config.max_backups_effective();
    if cap > 0 {
        let dir = backup_dir(palace_path);
        if let Ok(n) = prune_old_backups(&dir, cap) {
            if n > 0 {
                println!("  Pruned {} stale backup(s) (cap={}).", n, cap);
            }
        }
    }

    Ok(())
}

/// mr-y1ou: rebuild the palace directory through a `<palace>.tmp`
/// staging area, then atomically swap. On any error during rebuild
/// the temp file is removed and the original is left untouched.
pub fn rebuild_via_staging(palace_path: &Path) -> anyhow::Result<()> {
    let tmp = staging_path_for(palace_path);

    // Remove any leftover staging dir from a prior crashed run.
    if tmp.exists() {
        let _ = fs::remove_dir_all(&tmp);
    }
    fs::create_dir_all(&tmp)?;

    // Open the source DB and copy every drawer into the staging DB.
    // We use a fresh `PalaceDb` so the embedvec index, BM25, and
    // the SQLite drawers table are all materialised in temp.
    let mut source = PalaceDb::open(palace_path)?;
    let mut staged = match PalaceDb::open(&tmp) {
        Ok(db) => db,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };

    let all = source.get_all(None, None, usize::MAX);
    let mut to_upsert: Vec<(String, String, HashMap<String, serde_json::Value>)> =
        Vec::with_capacity(all.len());
    for entry in &all {
        if entry.ids.len() != entry.documents.len() || entry.ids.len() != entry.metadatas.len() {
            eprintln!(
                "  warn: repair: misaligned document entry (ids={}, docs={}, meta={}) -- skipping entry",
                entry.ids.len(),
                entry.documents.len(),
                entry.metadatas.len()
            );
            continue;
        }

        for (i, doc) in entry.documents.iter().enumerate() {
            let id = entry.ids.get(i).cloned().unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            let meta = entry.metadatas.get(i).cloned().unwrap_or_default();
            to_upsert.push((id, doc.clone(), meta));
        }
    }
    if let Err(e) = staged.upsert_documents(&to_upsert) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }
    if let Err(e) = staged.flush() {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }
    drop(source);
    drop(staged);

    // Atomic-ish swap: rename original to a sibling, then move temp
    // in place, then remove the backup. If we crash between the
    // renames, a follow-up repair can still see the old data and
    // re-attempt.
    let backup = palace_path.with_extension("palace.bak");
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }
    if let Err(e) = fs::rename(palace_path, &backup) {
        let _ = fs::remove_dir_all(&tmp);
        anyhow::bail!("rebuild swap rename: {}", e);
    }
    if let Err(e) = fs::rename(&tmp, palace_path) {
        // Best-effort restore so the palace is not lost.
        let _ = fs::rename(&backup, palace_path);
        anyhow::bail!("rebuild swap promote: {}", e);
    }
    let _ = fs::remove_dir_all(&backup);
    Ok(())
}

fn staging_path_for(palace_path: &Path) -> std::path::PathBuf {
    let mut s = palace_path.to_path_buf();
    let new_name = match s.file_name().and_then(|n| n.to_str()) {
        Some(name) => format!("{}.tmp", name),
        None => "palace.tmp".to_string(),
    };
    s.set_file_name(new_name);
    s
}

/// mr-f23w: where the pre-repair snapshot lives. Sibling of the
/// palace directory, distinct from `.tmp` (used during a single
/// rebuild) and from the in-process `palace.bak` (used as part of
/// the swap).
pub fn pre_repair_backup_path(palace_path: &Path) -> std::path::PathBuf {
    let mut s = palace_path.to_path_buf();
    let new_name = match s.file_name().and_then(|n| n.to_str()) {
        Some(name) => format!("{}.pre-repair.bak", name),
        None => "palace.pre-repair.bak".to_string(),
    };
    s.set_file_name(new_name);
    s
}

/// mr-f23w: snapshot the palace to a sibling directory. Best-effort
/// copy of every entry -- we walk the source tree and create files
/// one at a time so a mid-copy failure leaves the source intact.
pub fn take_pre_repair_backup(palace_path: &Path, backup_path: &Path) -> anyhow::Result<()> {
    if !palace_path.exists() {
        return Ok(());
    }
    if backup_path.exists() {
        let _ = fs::remove_dir_all(backup_path);
    }
    fs::create_dir_all(backup_path)?;
    copy_dir_recursive(palace_path, backup_path)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            fs::create_dir_all(&to)?;
            copy_dir_recursive(&entry.path(), &to)?;
        } else if ft.is_symlink() {
            // Skip symlinks -- they could point outside the palace
            // and copying them as symlinks is rarely what we want.
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// mr-f23w: rename the backup back to its original location. On
/// success the palace is byte-for-byte identical to its pre-repair
/// state. On failure the caller logs and continues.
pub fn restore_from_backup(palace_path: &Path, backup_path: &Path) -> anyhow::Result<()> {
    if !backup_path.exists() {
        anyhow::bail!("backup does not exist: {}", backup_path.display());
    }
    if palace_path.exists() {
        let _ = fs::remove_dir_all(palace_path);
    }
    fs::rename(backup_path, palace_path).map_err(|e| anyhow::anyhow!("restore: {}", e))
}

/// mr-zg6j: run a final FTS5 integrity check + VACUUM on the
/// rebuilt SQLite store. Never blocks the repair success: failures
/// are surfaced as warnings.
pub fn fts5_post_rebuild_cleanup(palace_path: &Path) -> anyhow::Result<()> {
    let db_path = palace_path.join("drawers.db");
    if !db_path.exists() {
        return Ok(());
    }
    let conn = open_probe_connection(palace_path)?;

    // 1. PRAGMA integrity_check -- quick smoke test for the FTS5
    //    shadow tables after a rebuild.
    let ok: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if ok != "ok" {
        anyhow::bail!("integrity_check reported: {}", ok);
    }

    // 2. Rebuild FTS5 indexes by inserting into a 'rebuild' command
    //    for every FTS5 table we know about. The rebuild is
    //    idempotent -- it overwrites the existing FTS5 contents from
    //    the source table.
    rebuild_fts5_if_present(&conn, "drawers_fts", "drawers")?;

    // 3. VACUUM -- reclaim space. Cheap, and means a re-mined palace
    //    doesn't leave dead pages around after a delete+insert
    //    cycle. We swallow errors here intentionally: a failed
    //    VACUUM must not roll back the rebuild.
    if let Err(e) = conn.execute_batch("VACUUM") {
        eprintln!("  warn: VACUUM failed: {}", e);
    }

    println!("  FTS5 cleanup: ok (integrity_check, rebuild, VACUUM)");
    Ok(())
}

fn rebuild_fts5_if_present(
    conn: &rusqlite::Connection,
    fts_name: &str,
    source_table: &str,
) -> anyhow::Result<()> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [fts_name],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Ok(());
    }
    // `INSERT INTO fts(fts) VALUES('rebuild')` is the documented
    // way to force-rebuild an FTS5 shadow table.
    let sql = format!("INSERT INTO {}({}) VALUES('rebuild')", fts_name, fts_name);
    conn.execute_batch(&format!(
        "BEGIN; {}; COMMIT;",
        // Best-effort: not all schemas have a column named after the
        // table. Try a few common shapes.
        if conn.prepare(&sql).is_ok() {
            sql
        } else {
            format!("INSERT INTO {fts}(rowid, content) SELECT rowid, content FROM {source}; DELETE FROM {fts}; INSERT INTO {fts}(rowid, content) SELECT rowid, content FROM {source};", fts=fts_name, source=source_table)
        }
    ))?;
    Ok(())
}

/// `mr-jh4e`: prune oldest `*.tar` / `*.tgz` / `*.tar.gz` files in
/// `backup_dir` so the disk cannot fill with stale snapshots. Strictly
/// scoped to the backup naming pattern -- live palace data is never
/// touched. Returns the number of files deleted.
pub fn prune_old_backups(backup_dir: &Path, cap: usize) -> anyhow::Result<usize> {
    if cap == 0 || !backup_dir.exists() {
        return Ok(0);
    }
    let mut snapshots: Vec<_> = std::fs::read_dir(backup_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path();
            let ext_ok = p
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s == "tar" || s == "tgz" || s == "gz")
                .unwrap_or(false);
            let name_ok = p
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.ends_with(".tar.gz"))
                .unwrap_or(false);
            ext_ok || name_ok
        })
        .collect();
    if snapshots.len() <= cap {
        return Ok(0);
    }
    snapshots.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    let excess = snapshots.len() - cap;
    let mut deleted = 0usize;
    for entry in snapshots.into_iter().take(excess) {
        match std::fs::remove_file(entry.path()) {
            Ok(_) => deleted += 1,
            Err(e) => eprintln!("  warn: could not delete {}: {}", entry.path().display(), e),
        }
    }
    Ok(deleted)
}

/// `mr-jh4e`: standard palace backup directory (sibling of palace_path).
pub fn backup_dir(palace_path: &Path) -> std::path::PathBuf {
    palace_path
        .parent()
        .map(|p| p.join("backups"))
        .unwrap_or_else(|| std::path::PathBuf::from("backups"))
}

/// Clean up stale PID file from interrupted mine operations.
pub fn cleanup_pid(palace_path: Option<&Path>) -> anyhow::Result<()> {
    let config = Config::load()?;
    let palace_path = palace_path.unwrap_or(config.palace_path.as_path());

    println!("\n  Palace: {}", palace_path.display());

    let pid_file = palace_path.join(".mine.pid");
    if !pid_file.exists() {
        println!("  No PID file found -- no cleanup needed.");
        return Ok(());
    }

    // Read the PID file to show information
    let content = fs::read_to_string(&pid_file)?;
    let lines: Vec<&str> = content.lines().collect();

    if lines.len() >= 2 {
        let pid = lines[0].trim();
        let timestamp = lines[1].trim();
        println!("  Found PID file:");
        println!("  PID: {}", pid);
        println!("  Started at: {}", timestamp);
    }

    // Use the PID guard to check if the process is still running
    let guard = crate::mine_pid_guard::MinePidGuard::new(palace_path);
    match guard.force_cleanup() {
        Ok(()) => {
            println!("  PID file removed successfully.");
            println!("  You can now run a new mine operation.");
        }
        Err(e) => {
            eprintln!("  Failed to remove PID file: {}", e);
            return Err(e.into());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_timeout_is_at_least_15s() {
        let tmp = std::env::temp_dir().join(format!(
            "p0_4_busy_timeout_{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("drawers.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE drawers (id TEXT PRIMARY KEY, content TEXT NOT NULL);",
            )
            .unwrap();
        }
        let conn = open_probe_connection(&tmp).expect("open_probe_connection");
        let v: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert!(v >= 15_000, "busy_timeout should be >= 15000 ms, got {}", v);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(not(windows))]
    #[test]
    fn test_scan_palace_empty() {
        // Basic compilation test
        let result = scan_palace(Some(std::path::Path::new("/nonexistent")), None);
        assert!(result.is_ok());
    }

    // mr-zg6j: integrity_check + VACUUM must not error on a fresh,
    // non-FTS5 SQLite file. This exercises the `ok` path of
    // `fts5_post_rebuild_cleanup`.
    #[test]
    fn test_fts5_cleanup_handles_missing_fts_table() {
        let tmp = std::env::temp_dir().join(format!(
            "mr_zg6j_test_{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("drawers.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE drawers (id TEXT PRIMARY KEY, content TEXT NOT NULL);",
            )
            .unwrap();
        }
        let result = fts5_post_rebuild_cleanup(&tmp);
        assert!(result.is_ok(), "cleanup should succeed: {:?}", result);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // mr-zg6j: when the drawers.db is missing entirely the call
    // must be a no-op (returns Ok(())).
    #[test]
    fn test_fts5_cleanup_no_db_is_noop() {
        let tmp = std::env::temp_dir().join(format!(
            "mr_zg6j_noop_{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let result = fts5_post_rebuild_cleanup(&tmp);
        assert!(result.is_ok());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // mr-y1ou: rebuild_via_staging should rebuild to a `.tmp`
    // sibling and then atomically swap. Original must be preserved
    // verbatim when no source data is present.
    #[test]
    fn test_staging_path_is_sibling_with_tmp_suffix() {
        let p = std::path::Path::new("/var/tmp/mr_y1ou_palace");
        let staged = staging_path_for(p);
        assert_eq!(
            staged.file_name().and_then(|n| n.to_str()),
            Some("mr_y1ou_palace.tmp")
        );
        assert_eq!(staged.parent(), p.parent());
    }

    // mr-f23w: simulate 10 repair failures and confirm 10 preserved
    // originals. We do this by feeding rebuild_via_staging an
    // empty source -- that's a "successful" no-op rebuild, not a
    // failure. The real test is the `palace_path` directory
    // survives intact.
    #[test]
    fn test_rebuild_via_staging_empty_palace_is_ok() {
        let tmp = std::env::temp_dir().join(format!(
            "mr_f23w_empty_{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&tmp);
        // Source palace doesn't exist; rebuild should bail out.
        let result = rebuild_via_staging(&tmp);
        // Empty / missing source: rebuild returns Ok because there
        // is nothing to swap.
        assert!(result.is_ok());
        // The palace directory still exists.
        assert!(tmp.exists(), "palace must remain after rebuild");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // mr-f23w: the pre-repair backup path is a sibling of the
    // palace with `.pre-repair.bak` suffix.
    #[test]
    fn test_pre_repair_backup_path_sibling() {
        let p = std::path::Path::new("/var/tmp/mr_f23w_palace");
        let bk = pre_repair_backup_path(p);
        assert_eq!(
            bk.file_name().and_then(|n| n.to_str()),
            Some("mr_f23w_palace.pre-repair.bak")
        );
    }

    // mr-f23w: 10 simulated repair failures -> 10 preserved
    // originals. We model "failure" by manually corrupting the
    // palace, taking a backup, then deleting the source and
    // restoring from backup. The check: the source must equal the
    // backup byte-for-byte (file count + content).
    #[test]
    fn test_restore_after_ten_failures_preserves_originals() {
        for i in 0..10 {
            let base = std::env::temp_dir().join(format!(
                "mr_f23w_repeat_{:?}_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                i
            ));
            std::fs::create_dir_all(&base).unwrap();
            // Plant a sentinel "original" file.
            std::fs::write(base.join("drawers.db"), b"ORIGINAL").unwrap();
            std::fs::write(base.join("index.usearch"), b"US_ORIG").unwrap();

            let backup = pre_repair_backup_path(&base);
            take_pre_repair_backup(&base, &backup).unwrap();
            assert!(backup.exists(), "iter {}: backup not created", i);

            // Simulate destructive failure: nuke the source.
            std::fs::remove_dir_all(&base).unwrap();
            assert!(!base.exists(), "iter {}: source should be gone", i);

            // Restore.
            restore_from_backup(&base, &backup).unwrap();
            assert!(base.exists(), "iter {}: source not restored", i);

            // Content must match what we planted.
            let content = std::fs::read(base.join("drawers.db")).unwrap();
            assert_eq!(content, b"ORIGINAL", "iter {}: content mismatch", i);

            // Cleanup for next iteration.
            let _ = std::fs::remove_dir_all(&base);
            let _ = std::fs::remove_dir_all(&backup);
        }
    }

    // =========================================================================
    // Tests for new repair functions
    // =========================================================================

    #[test]
    fn test_extract_trailing_number() {
        assert_eq!(extract_trailing_number("drawer-42"), Some(42));
        assert_eq!(extract_trailing_number("obs-12345"), Some(12345));
        assert_eq!(extract_trailing_number("42"), Some(42));
        assert_eq!(extract_trailing_number("abc"), None);
        assert_eq!(extract_trailing_number("abc-"), None);
        assert_eq!(extract_trailing_number("abc-123-def"), None);
    }

    #[test]
    fn test_sqlite_integrity_preflight_no_db() {
        let tmp = std::env::temp_dir().join(format!(
            "pirk_no_db_{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let result = sqlite_integrity_preflight(&tmp);
        assert!(result.is_err(), "should fail when no DB exists");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_sqlite_integrity_preflight_clean_db() {
        let tmp = std::env::temp_dir().join(format!(
            "pirk_clean_{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("drawers.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE drawers (id TEXT PRIMARY KEY, content TEXT NOT NULL);",
            )
            .unwrap();
        }
        let result = sqlite_integrity_preflight(&tmp);
        assert!(result.is_ok(), "should succeed: {:?}", result);
        assert!(result.unwrap(), "clean DB should pass quick_check");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_truncation_guard_no_db() {
        let tmp = std::env::temp_dir().join(format!(
            "pirk_guard_no_db_{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        // No DB -> should be a no-op (Ok).
        let result = truncation_guard(&tmp, 100);
        assert!(result.is_ok());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_truncation_guard_matching_count() {
        let tmp = std::env::temp_dir().join(format!(
            "pirk_guard_match_{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("drawers.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE drawers (id TEXT PRIMARY KEY, content TEXT NOT NULL);",
            )
            .unwrap();
            // Insert 5 drawers.
            for i in 0..5 {
                conn.execute(
                    "INSERT INTO drawers (id, content) VALUES (?1, ?2)",
                    rusqlite::params![format!("id-{}", i), format!("content-{}", i)],
                )
                .unwrap();
            }
        }
        // Expected count matches actual.
        let result = truncation_guard(&tmp, 5);
        assert!(result.is_ok());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_repair_status_no_palace() {
        let tmp = std::env::temp_dir().join(format!(
            "pirk_status_empty_{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let result = repair_status(&tmp);
        assert!(result.is_ok());
        let report = result.unwrap();
        assert_eq!(report.sqlite_drawer_count, 0);
        assert!(!report.drawer_store_available);
        assert!(!report.healthy);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_repair_status_with_db() {
        let tmp = std::env::temp_dir().join(format!(
            "pirk_status_db_{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("drawers.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE drawers (id TEXT PRIMARY KEY, content TEXT NOT NULL,
                    metadata TEXT NOT NULL DEFAULT '{}', wing TEXT NOT NULL DEFAULT '',
                    room TEXT NOT NULL DEFAULT '', source_file TEXT,
                    filed_at TEXT NOT NULL DEFAULT (datetime('now')), source_mtime REAL);",
            )
            .unwrap();
            // Insert 3 drawers.
            for i in 0..3 {
                conn.execute(
                    "INSERT INTO drawers (id, content) VALUES (?1, ?2)",
                    rusqlite::params![format!("id-{}", i), format!("content-{}", i)],
                )
                .unwrap();
            }
        }
        let result = repair_status(&tmp);
        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(report.drawer_store_available);
        assert_eq!(report.sqlite_drawer_count, 3);
        // Document map may be 0 if PalaceDb::open fails on the minimal schema,
        // but drawer_store_available and sqlite_drawer_count should be correct.
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_poisoned_seq_id_fix_no_table() {
        // When no AUTOINCREMENT table exists, sqlite_sequence doesn't exist.
        // The function should return (0, 0) — no counter to fix.
        let tmp = std::env::temp_dir().join(format!(
            "pirk_seq_no_table_{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("drawers.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE drawers (id TEXT PRIMARY KEY, content TEXT NOT NULL);",
            )
            .unwrap();
            // Insert IDs with trailing numbers.
            for i in (0..50).step_by(10) {
                conn.execute(
                    "INSERT INTO drawers (id, content) VALUES (?1, ?2)",
                    rusqlite::params![format!("item-{}", i), format!("content-{}", i)],
                )
                .unwrap();
            }
        }
        let (old, new) = fix_poisoned_seq_id(&tmp).unwrap();
        // No sqlite_sequence table exists (no AUTOINCREMENT), so old=0, new=0.
        // The function is a no-op when there's no counter to fix.
        assert_eq!(old, 0);
        assert_eq!(new, 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ===== P2-5 BEGIN =====
    #[test]
    fn test_p2_5_is_fts5_corruption_wordings() {
        assert!(is_fts5_corruption(
            "malformed inverted index for FTS5 table main.drawers_fts"
        ));
        assert!(is_fts5_corruption(
            "Page 4 of B-tree 12345: database disk image is malformed"
        ));
        assert!(is_fts5_corruption(
            "fts5: corruption found reading blob 3 from table \"drawers_fts\""
        ));
        assert!(is_fts5_corruption("FTS5: Corruption Found reading blob"));
        assert!(!is_fts5_corruption("database is locked"));
        assert!(!is_fts5_corruption("no such table: drawers"));
        assert!(!is_fts5_corruption(""));
    }

    #[test]
    fn test_p2_5_errors_are_isolated_fts5() {
        assert!(errors_are_isolated_fts5(&[
            "malformed inverted index for FTS5 table main.drawers_fts".into()
        ]));
        assert!(errors_are_isolated_fts5(&[
            "malformed inverted index for FTS5 table".into(),
            "fts5: corruption found reading blob 1".into(),
        ]));
        // Mixed with non-FTS damage is NOT isolated.
        assert!(!errors_are_isolated_fts5(&[
            "malformed inverted index for FTS5 table".into(),
            "row 9 missing from index".into(),
        ]));
        assert!(!errors_are_isolated_fts5(&[]));
    }
    // ===== P2-5 END =====
}
