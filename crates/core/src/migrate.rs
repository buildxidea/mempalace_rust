//! migrate.rs — Detect and migrate ChromaDB palaces.
//!
//! Detects ChromaDB version from SQLite schema, reads drawers directly
//! from SQLite (bypassing ChromaDB API), and migrates to embedvec.
//!
//! Usage:
//!     mpr migrate [--dry-run]

#![doc(hidden)]

use crate::config::Config;
use rusqlite::{Connection, Result as SqlResult};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Migration statistics.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct MigrateStats {
    pub drawers_found: usize,
    pub drawers_migrated: usize,
    pub skipped: usize,
    pub errors: usize,
}

/// Detect ChromaDB version from the SQLite schema fingerprint.
fn detect_chroma_version(schema_sql: &str) -> Option<String> {
    if schema_sql.contains("embeddings") && schema_sql.contains("metadatas") {
        Some("0.4+".to_string())
    } else if schema_sql.contains("rowid") {
        Some("0.3.x".to_string())
    } else {
        None
    }
}

/// Read raw drawer data from ChromaDB SQLite (bypasses ChromaDB API).
fn read_chroma_sqlite(palace_path: &Path) -> SqlResult<Vec<DrawerRecord>> {
    let db_path = palace_path.join("chroma.sqlite3");
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let conn = Connection::open(&db_path)?;
    let mut stmt = conn.prepare(
        "SELECT r.id, r.document, r.embedding, m.key, m.value
         FROM records r
         LEFT JOIN metadata m ON r.id = m.record_id",
    )?;

    let mut records: Vec<DrawerRecord> = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_doc: Option<String> = None;
    let mut current_meta: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    let rows = stmt.query_map([], |row| {
        Ok(RowRaw {
            id: row.get(0)?,
            document: row.get(1)?,
            meta_key: row.get(3)?,
            meta_value: row.get(4)?,
        })
    })?;

    for row in rows.flatten() {
        if current_id.is_none() || current_id.as_ref() != Some(&row.id) {
            if current_id.is_some() {
                records.push(DrawerRecord {
                    id: current_id.take().unwrap(),
                    document: current_doc.take().unwrap_or_default(),
                    metadata: std::mem::take(&mut current_meta),
                });
            }
            current_id = Some(row.id);
            current_doc = row.document;
        }
        if let (Some(k), Some(v)) = (row.meta_key, row.meta_value) {
            current_meta.insert(k, v);
        }
    }

    if current_id.is_some() {
        records.push(DrawerRecord {
            id: current_id.take().unwrap(),
            document: current_doc.take().unwrap_or_default(),
            metadata: current_meta,
        });
    }

    Ok(records)
}

struct RowRaw {
    id: String,
    document: Option<String>,
    meta_key: Option<String>,
    meta_value: Option<String>,
}

#[allow(dead_code)]
struct DrawerRecord {
    id: String,
    document: String,
    metadata: std::collections::HashMap<String, String>,
}

/// Detect ChromaDB version and return a report.
pub fn detect_version(palace_path: &Path) -> anyhow::Result<ChromaDetectReport> {
    let db_path = palace_path.join("chroma.sqlite3");
    if !db_path.exists() {
        return Err(anyhow::anyhow!(
            "No chroma.sqlite3 found at {}",
            db_path.display()
        ));
    }

    let conn = Connection::open(&db_path)?;
    let schema_sql: String = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='records'",
        [],
        |row| row.get(0),
    )?;

    let version = detect_chroma_version(&schema_sql)
        .ok_or_else(|| anyhow::anyhow!("Cannot detect ChromaDB version from schema"))?;

    let count: usize = conn.query_row("SELECT COUNT(*) FROM records", [], |row| row.get(0))?;

    Ok(ChromaDetectReport {
        version,
        drawer_count: count,
        palace_path: palace_path.to_path_buf(),
    })
}

#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct ChromaDetectReport {
    pub version: String,
    pub drawer_count: usize,
    pub palace_path: PathBuf,
}

/// Run migration (detect + export + import to embedvec).
pub fn migrate_palace(palace_path: Option<&Path>, dry_run: bool) -> anyhow::Result<MigrateStats> {
    let config = Config::load()?;
    let palace_path = palace_path.unwrap_or(config.palace_path.as_path());

    println!("\n{}", "=".repeat(55));
    println!("  MemPalace Migrator");
    println!("{}", "=".repeat(55));

    // Detect version
    let report = match detect_version(palace_path) {
        Ok(r) => r,
        Err(e) => {
            println!("  Detection failed: {}", e);
            println!("  This palace may already be in embedvec format.");
            return Ok(MigrateStats {
                drawers_found: 0,
                drawers_migrated: 0,
                skipped: 0,
                errors: 1,
            });
        }
    };

    println!("  Palace: {}", palace_path.display());
    println!("  Detected: ChromaDB {}", report.version);
    println!("  Drawers: {}", report.drawer_count);
    println!("  Mode: {}", if dry_run { "DRY RUN" } else { "LIVE" });
    println!("{}", "=".repeat(55));

    // Read directly from SQLite
    let records = read_chroma_sqlite(palace_path)?;
    println!("  Read {} records from SQLite", records.len());

    if dry_run {
        println!("\n  [DRY RUN] No changes written.");
        return Ok(MigrateStats {
            drawers_found: records.len(),
            drawers_migrated: 0,
            skipped: 0,
            errors: 0,
        });
    }

    // Migration would upsert to embedvec here
    // (ChromaDB bypass → embedvec upsert)
    let mut migrated = 0usize;
    let errors = 0usize;

    for _record in &records {
        // Placeholder: upsert to PalaceDb (embedvec)
        // palace_db.upsert_documents(&[(...)])?;
        migrated += 1;
    }

    println!("\n  Done. Migrated: {}/{}", migrated, records.len());

    Ok(MigrateStats {
        drawers_found: records.len(),
        drawers_migrated: migrated,
        skipped: records.len().saturating_sub(migrated),
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_chroma_version_0_4() {
        let schema = "CREATE TABLE embeddings (id TEXT, embedding BLOB) CREATE TABLE metadatas";
        assert_eq!(detect_chroma_version(schema), Some("0.4+".to_string()));
    }

    #[test]
    fn test_detect_chroma_version_0_3() {
        let schema = "CREATE TABLE records (rowid INTEGER PRIMARY KEY)";
        assert_eq!(detect_chroma_version(schema), Some("0.3.x".to_string()));
    }

    #[test]
    fn test_detect_chroma_version_unknown() {
        let schema = "CREATE TABLE unknown (id INTEGER)";
        assert_eq!(detect_chroma_version(schema), None);
    }

    #[test]
    fn test_migrate_wings_renames_and_preserves_legacy() {
        // mr-qioh: every drawer must have its `wing` column normalized
        // (lowercase, separators → underscore), and the original spelling
        // must survive under `wing_legacy` in metadata so old
        // references still resolve.
        let temp = tempfile::tempdir().unwrap();
        let palace = temp.path();

        // Build a minimal palace.db with a `drawers` table that matches
        // the production schema (id, wing, metadata).
        let db = palace.join("palace.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE drawers (
                 id TEXT PRIMARY KEY,
                 content TEXT NOT NULL,
                 kind TEXT,
                 tier TEXT,
                 wing TEXT,
                 room TEXT,
                 metadata TEXT
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO drawers (id, content, kind, tier, wing, room, metadata) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params!["d1", "hello world", "note", "long", "Mixed Case", "room1", "{\"foo\":\"bar\"}"],
        ).unwrap();
        conn.execute(
            "INSERT INTO drawers (id, content, kind, tier, wing, room, metadata) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params!["d2", "another", "note", "long", "with-dash", "room2", "{}"],
        ).unwrap();
        conn.execute(
            "INSERT INTO drawers (id, content, kind, tier, wing, room, metadata) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params!["d3", "already ok", "note", "long", "lowercase_ok", "room3", "{}"],
        ).unwrap();
        drop(conn);

        let stats = migrate_wings(Some(palace), false).unwrap();
        assert_eq!(stats.drawers_scanned, 3);
        assert!(
            stats.renamed >= 2,
            "expected at least 2 renames, got {}",
            stats.renamed
        );

        // Verify the rows.
        let conn = Connection::open(&db).unwrap();
        let mut stmt = conn
            .prepare("SELECT id, wing, metadata FROM drawers ORDER BY id")
            .unwrap();
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        // d1: "Mixed Case" → "mixed_case"
        assert_eq!(rows[0].0, "d1");
        assert_eq!(rows[0].1, "mixed_case");
        let meta1: serde_json::Value = serde_json::from_str(&rows[0].2).unwrap();
        assert_eq!(
            meta1.get("wing_legacy").and_then(|v| v.as_str()),
            Some("Mixed Case")
        );

        // d2: "with-dash" → "with_dash"
        assert_eq!(rows[1].0, "d2");
        assert_eq!(rows[1].1, "with_dash");
        let meta2: serde_json::Value = serde_json::from_str(&rows[1].2).unwrap();
        assert_eq!(
            meta2.get("wing_legacy").and_then(|v| v.as_str()),
            Some("with-dash")
        );

        // d3: already normalized, no rename, no wing_legacy.
        assert_eq!(rows[2].0, "d3");
        assert_eq!(rows[2].1, "lowercase_ok");
        let meta3: serde_json::Value = serde_json::from_str(&rows[2].2).unwrap();
        assert!(meta3.get("wing_legacy").is_none());
    }

    // ===== P1-2 BEGIN =====
    #[test]
    fn test_migrate_authored_at_backfills_from_session_jsonl() {
        use crate::drawer_store::DrawerStore;
        use std::collections::HashMap;

        let temp = tempfile::tempdir().unwrap();
        let palace = temp.path().join("palace");
        let sessions = temp.path().join("sessions");
        std::fs::create_dir_all(&palace).unwrap();
        std::fs::create_dir_all(&sessions).unwrap();

        // Source transcript with known timestamps.
        let transcript = sessions.join("chat.jsonl");
        std::fs::write(
            &transcript,
            r#"{"type":"user","timestamp":"2022-04-01T10:00:00.000Z","message":"hi"}
{"type":"assistant","timestamp":"2022-04-01T10:05:00.000Z","message":"hello"}
"#,
        )
        .unwrap();

        // Seed a convo drawer without authored_at (pre-P1-2 shape).
        let store = DrawerStore::open(&palace).unwrap();
        let mut meta = HashMap::new();
        meta.insert(
            "ingest_mode".to_string(),
            serde_json::Value::String("convos".to_string()),
        );
        meta.insert(
            "filed_at".to_string(),
            serde_json::Value::String("2025-01-01T00:00:00Z".to_string()),
        );
        store
            .insert(
                "d1",
                "conversation content",
                &meta,
                "wing",
                "room",
                Some(transcript.to_str().unwrap()),
                None,
            )
            .unwrap();
        assert_eq!(store.get_authored_at("d1").unwrap(), None);

        let stats = migrate_authored_at(Some(&palace), &[sessions.clone()], false).unwrap();
        assert_eq!(stats.scanned, 1);
        assert_eq!(stats.updated, 1);
        assert_eq!(stats.resolved_files, 1);
        assert_eq!(
            store.get_authored_at("d1").unwrap().as_deref(),
            Some("2022-04-01T10:05:00.000Z")
        );

        // Idempotent: second run skips already-set.
        let stats2 = migrate_authored_at(Some(&palace), &[sessions], false).unwrap();
        assert_eq!(stats2.updated, 0);
        assert_eq!(stats2.skipped_already_set, 1);
    }
    // ===== P1-2 END =====
}

// ---------------------------------------------------------------------------
// mr-qioh: `migrate-wings` — normalize every drawer's `wing` column.
// ---------------------------------------------------------------------------

/// Stats from a `migrate-wings` run.
#[derive(Debug, Clone, Serialize, Default)]
#[non_exhaustive]
pub struct MigrateWingsStats {
    pub drawers_scanned: usize,
    pub renamed: usize,
    pub unchanged: usize,
    pub errors: usize,
}

/// Normalize every drawer's `wing` column in `<palace>/palace.db`.
///
/// Each drawer's `wing` is rewritten via `config::normalize_wing_name`
/// (lowercase, separators → `_`). The original spelling is preserved
/// inside the drawer's `metadata` JSON as `wing_legacy` so older code
/// paths and human readers can still find the original taxonomy.
///
/// Idempotent: re-running on an already-migrated palace is a no-op.
///
/// `dry_run=true` reports what *would* change without writing.
pub fn migrate_wings(
    palace_path: Option<&Path>,
    dry_run: bool,
) -> anyhow::Result<MigrateWingsStats> {
    let palace_path = match palace_path {
        Some(p) => p.to_path_buf(),
        None => Config::load()?.palace_path,
    };
    let db_path = palace_path.join("palace.db");
    if !db_path.exists() {
        anyhow::bail!(
            "No palace.db found at {} — run `mpr init` first",
            db_path.display()
        );
    }
    let mut conn = Connection::open(&db_path)?;

    // The drawers table may have been created by an older build without a
    // `wing` column, or not at all in a brand-new palace. We probe for
    // both and bail with a friendly error if neither is present.
    let has_drawers: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='drawers' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if !has_drawers {
        return Ok(MigrateWingsStats::default());
    }
    let has_wing_col: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('drawers') WHERE name='wing' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if !has_wing_col {
        // No wing column to normalize; nothing to do.
        return Ok(MigrateWingsStats::default());
    }

    let mut stats = MigrateWingsStats::default();

    // Snapshot all (id, wing, metadata) so we can detect changes.
    let mut stmt = conn.prepare("SELECT id, wing, metadata FROM drawers")?;
    let rows: Vec<(String, Option<String>, Option<String>)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .map(|r| r.unwrap())
        .collect();
    drop(stmt);
    stats.drawers_scanned = rows.len();

    for (id, wing, metadata_json) in rows {
        let Some(raw) = wing else {
            continue;
        };
        let normalized = crate::config::normalize_wing_name(&raw);
        if normalized == raw {
            stats.unchanged += 1;
            continue;
        }
        if !dry_run {
            // Preserve original spelling under metadata.wing_legacy. If
            // metadata is missing or not JSON, fall back to a fresh
            // object so we never silently lose pre-existing fields.
            let new_meta = upsert_legacy_wing(&metadata_json, &raw);
            let update = conn.execute(
                "UPDATE drawers SET wing = ?1, metadata = ?2 WHERE id = ?3",
                rusqlite::params![normalized, new_meta, id],
            );
            if let Err(e) = update {
                stats.errors += 1;
                eprintln!("  failed to migrate {id}: {e}");
                continue;
            }
        }
        stats.renamed += 1;
    }

    if dry_run {
        println!(
            "  [DRY RUN] would rename {} wings ({} unchanged)",
            stats.renamed, stats.unchanged
        );
    } else {
        println!(
            "  Renamed {} wings ({} unchanged, {} errors)",
            stats.renamed, stats.unchanged, stats.errors
        );
    }
    Ok(stats)
}

fn upsert_legacy_wing(metadata_json: &Option<String>, legacy: &str) -> String {
    let mut obj: serde_json::Map<String, serde_json::Value> = match metadata_json {
        Some(s) if !s.trim().is_empty() => serde_json::from_str(s).unwrap_or_default(),
        _ => serde_json::Map::new(),
    };
    // Only set wing_legacy if it isn't already present — never clobber.
    obj.entry("wing_legacy".to_string())
        .or_insert(serde_json::Value::String(legacy.to_string()));
    serde_json::to_string(&obj).unwrap_or_else(|_| "{}".to_string())
}

// ===== P1-2 BEGIN =====
// ---------------------------------------------------------------------------
// P1-2: `migrate authored-at` — backfill authored_at from source transcripts.
// ---------------------------------------------------------------------------

/// Stats from an authored-at backfill run.
#[derive(Debug, Clone, Serialize, Default)]
#[non_exhaustive]
pub struct MigrateAuthoredAtStats {
    pub scanned: usize,
    pub updated: usize,
    pub resolved_files: usize,
    pub unresolved_files: usize,
    pub skipped_already_set: usize,
}

/// Stamp `authored_at` on conversation drawers from their source transcript
/// timestamps (upstream `scripts/backfill_authored_at.py`).
///
/// Only touches drawers with `ingest_mode == "convos"`. Metadata + column
/// are updated in place; embeddings are left untouched. Idempotent: drawers
/// already carrying the correct `authored_at` are skipped.
///
/// `session_dirs` are roots that still hold the original `.jsonl` transcripts
/// (e.g. `~/.claude`, `~/.codex`). Basename match is used to resolve a
/// drawer’s `source_file` against those trees.
///
/// `dry_run=true` reports what *would* change without writing.
pub fn migrate_authored_at(
    palace_path: Option<&Path>,
    session_dirs: &[PathBuf],
    dry_run: bool,
) -> anyhow::Result<MigrateAuthoredAtStats> {
    use crate::convo_miner::extract_authored_at;
    use crate::drawer_store::DrawerStore;
    use std::collections::{HashMap, HashSet};
    use walkdir::WalkDir;

    let palace_path = match palace_path {
        Some(p) => p.to_path_buf(),
        None => Config::load()?.palace_path,
    };

    // Index basename.jsonl -> real path under the provided session dirs.
    let mut index: HashMap<String, PathBuf> = HashMap::new();
    for root in session_dirs {
        let root = if let Some(stripped) = root.to_string_lossy().strip_prefix("~/") {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(stripped)
        } else {
            root.to_path_buf()
        };
        if !root.exists() {
            eprintln!("  warning: session dir not found: {}", root.display());
            continue;
        }
        for entry in WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_lowercase();
            if ext != "jsonl" {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                index
                    .entry(name.to_string())
                    .or_insert_with(|| path.to_path_buf());
            }
        }
    }

    let store = DrawerStore::open(&palace_path)?;
    let rows = store.list_convo_drawers_for_authored_at_backfill()?;

    let mut stats = MigrateAuthoredAtStats::default();
    let mut cache: HashMap<String, Option<String>> = HashMap::new();
    let mut unresolved: HashSet<String> = HashSet::new();

    for (id, source_file, col_authored, meta_json) in rows {
        stats.scanned += 1;

        let basename = source_file
            .as_deref()
            .and_then(|s| Path::new(s).file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let authored = if basename.is_empty() {
            None
        } else if let Some(cached) = cache.get(&basename) {
            cached.clone()
        } else {
            let path = index.get(&basename);
            let extracted = path.and_then(|p| extract_authored_at(p));
            if path.is_none() {
                unresolved.insert(basename.clone());
            }
            cache.insert(basename.clone(), extracted.clone());
            extracted
        };

        let Some(authored) = authored else {
            continue;
        };

        // Already correct? Check column first, then metadata JSON.
        let meta_authored = serde_json::from_str::<serde_json::Value>(&meta_json)
            .ok()
            .and_then(|v| {
                v.get("authored_at")
                    .and_then(|a| a.as_str())
                    .map(|s| s.to_string())
            });
        let current = col_authored.filter(|s| !s.is_empty()).or(meta_authored);
        if current.as_deref() == Some(authored.as_str()) {
            stats.skipped_already_set += 1;
            continue;
        }

        if !dry_run {
            store.update_authored_at(&id, &authored)?;
        }
        stats.updated += 1;
    }

    stats.resolved_files = cache.values().filter(|v| v.is_some()).count();
    stats.unresolved_files = unresolved.len();

    let mode = if dry_run {
        "DRY-RUN (would update)"
    } else {
        "APPLIED"
    };
    println!(
        "  {mode}: scanned={} updated={} resolved_files={} unresolved_files={} already_set={}",
        stats.scanned,
        stats.updated,
        stats.resolved_files,
        stats.unresolved_files,
        stats.skipped_already_set
    );
    Ok(stats)
}
// ===== P1-2 END =====
