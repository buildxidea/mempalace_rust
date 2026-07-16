//! SQLite-based drawer storage with FTS5 full-text search.
//!
//! Replaces the legacy JSON file (`mempalace_drawers.json`) with an
//! incremental, indexed SQLite database. The schema stores each drawer
//! as a row and maintains an FTS5 virtual table for fast full-text
//! search across `content`, `wing`, and `room` columns.
//!
//! # Migration
//!
//! [`DrawerStore::migrate_from_json`] reads an existing JSON map and
//! bulk-inserts all entries into SQLite. After migration the JSON file
//! can be deleted (the store never reads it except during migration).
//!
//! # Backward compatibility
//!
//! [`PalaceDb`] still holds a `documents: HashMap<String, DocumentEntry>`
//! for the many existing code paths that iterate in-memory. When a
//! [`DrawerStore`] is present, writes go to SQLite *and* the HashMap;
//! reads work from the HashMap (fast). The `save()` method becomes a
//! no-op because SQLite writes are incremental.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde_json::Value;
use tracing::info;

use crate::normalize::sanitize_for_fts5;
use crate::palace_db::DocumentEntry;

/// SQLite-backed drawer store with FTS5 search.
pub struct DrawerStore {
    conn: Mutex<Connection>,
}

/// Metadata-only row returned by [`DrawerStore::list_filtered`].
///
/// Intentionally omits the drawer body so list endpoints stay cheap.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DrawerRow {
    pub id: String,
    pub title: String,
    pub source_file: Option<String>,
    pub filed_at: String,
    pub wing: String,
}

impl DrawerStore {
    /// Open (or create) the drawers SQLite database at `palace_path/drawers.db`.
    ///
    /// Creates the schema if it does not exist, including FTS5 virtual
    /// tables and triggers. WAL journal mode is enabled for better
    /// concurrent-read performance.
    pub fn open(palace_path: &Path) -> Result<Self> {
        let db_path = palace_path.join("drawers.db");
        // ===== P2-1 BEGIN =====
        // Reject empty / garbage files before rusqlite opens them. A bare
        // Connection::open on a non-SQLite file can leave a confusing error
        // (or a 0-byte stub from a previous failed create). Verify the
        // 16-byte "SQLite format 3\0" magic when the file already exists.
        verify_sqlite_magic_header(&db_path)?;
        // ===== P2-1 END =====
        let conn = Connection::open(&db_path)
            .with_context(|| format!("failed to open drawer store at {}", db_path.display()))?;

        // Enable WAL mode for better concurrent-read performance
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        // Create schema
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS drawers (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                metadata TEXT NOT NULL DEFAULT '{}',
                wing TEXT NOT NULL DEFAULT '',
                room TEXT NOT NULL DEFAULT '',
                source_file TEXT,
                filed_at TEXT NOT NULL DEFAULT (datetime('now')),
                source_mtime REAL,
                -- ===== P1-2 BEGIN =====
                authored_at TEXT
                -- ===== P1-2 END =====
            );

            -- ===== P1-5 BEGIN =====
            -- AAAK closet summaries keyed by source_file so delete_by_source
            -- can purge them alongside drawers (upstream 5ae2315).
            CREATE TABLE IF NOT EXISTS closets (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                source_file TEXT,
                wing TEXT NOT NULL DEFAULT '',
                room TEXT NOT NULL DEFAULT '',
                filed_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_closets_source_file ON closets(source_file);
            -- ===== P1-5 END =====

            CREATE VIRTUAL TABLE IF NOT EXISTS drawers_fts USING fts5(
                content, wing, room,
                content=drawers,
                content_rowid=rowid,
                tokenize='porter unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS drawers_ai AFTER INSERT ON drawers BEGIN
                INSERT INTO drawers_fts(rowid, content, wing, room)
                VALUES (new.rowid, new.content, new.wing, new.room);
            END;

            CREATE TRIGGER IF NOT EXISTS drawers_ad AFTER DELETE ON drawers BEGIN
                INSERT INTO drawers_fts(drawers_fts, rowid, content, wing, room)
                VALUES('delete', old.rowid, old.content, old.wing, old.room);
            END;

            CREATE TRIGGER IF NOT EXISTS drawers_au AFTER UPDATE ON drawers BEGIN
                INSERT INTO drawers_fts(drawers_fts, rowid, content, wing, room)
                VALUES('delete', old.rowid, old.content, old.wing, old.room);
                INSERT INTO drawers_fts(rowid, content, wing, room)
                VALUES (new.rowid, new.content, new.wing, new.room);
            END;",
        )?;

        // ===== P1-2 BEGIN =====
        // Existing palaces created before authored_at was introduced need a
        // nullable column added in place. CREATE TABLE IF NOT EXISTS is a
        // no-op on those DBs, so ALTER TABLE is the non-breaking migration.
        ensure_authored_at_column(&conn)?;
        // ===== P1-2 END =====

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Return the number of drawers in the store.
    pub fn len(&self) -> usize {
        self.conn
            .lock()
            .expect("conn")
            .query_row("SELECT COUNT(*) FROM drawers", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap_or(0) as usize
    }

    /// Returns true if the store has no drawers.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Load all drawers into a HashMap compatible with `PalaceDb::documents`.
    ///
    /// Used during [`PalaceDb::open`] to populate the in-memory cache.
    /// Returns `id → DocumentEntry` mappings suitable for direct use.
    pub fn load_all_to_hashmap(&self) -> Result<HashMap<String, DocumentEntry>> {
        let guard = self.conn.lock().expect("conn");
        // ===== P1-2 BEGIN =====
        let mut stmt =
            guard.prepare("SELECT id, content, metadata, wing, room, authored_at FROM drawers")?;
        // ===== P1-2 END =====
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let content: String = row.get(1)?;
            let metadata_str: String = row.get(2)?;
            let wing: String = row.get(3)?;
            let room: String = row.get(4)?;
            // ===== P1-2 BEGIN =====
            let authored_at: Option<String> = row.get(5)?;
            // ===== P1-2 END =====

            // Parse metadata JSON, add wing/room
            let mut metadata: HashMap<String, Value> =
                serde_json::from_str(&metadata_str).unwrap_or_default();
            if !wing.is_empty() {
                metadata.insert("wing".to_string(), Value::String(wing));
            }
            if !room.is_empty() {
                metadata.insert("room".to_string(), Value::String(room));
            }
            // ===== P1-2 BEGIN =====
            // Prefer the dedicated column when present so searcher recency
            // tie-break sees authored_at even if older metadata JSON lacked it.
            if let Some(ts) = authored_at {
                if !ts.is_empty() {
                    metadata
                        .entry("authored_at".to_string())
                        .or_insert(Value::String(ts));
                }
            }
            // ===== P1-2 END =====

            Ok((id, DocumentEntry { content, metadata }))
        })?;

        let mut documents = HashMap::new();
        for row in rows {
            let (id, entry) = row?;
            documents.insert(id, entry);
        }
        Ok(documents)
    }

    /// Get all drawers, optionally filtered by wing and/or room, with a limit.
    ///
    /// Returns `Vec<(id, content, metadata)>` matching the filter criteria.
    pub fn get_all(
        &self,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(String, String, HashMap<String, Value>)>> {
        let mut sql =
            String::from("SELECT id, content, metadata, wing, room FROM drawers WHERE 1=1");
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(w) = wing {
            sql.push_str(" AND wing = ?");
            param_values.push(Box::new(w.to_string()));
        }
        if let Some(r) = room {
            sql.push_str(" AND room = ?");
            param_values.push(Box::new(r.to_string()));
        }
        // ===== P1-2 BEGIN =====
        // Prefer original transcript time when present; fall back to mine time.
        sql.push_str(" ORDER BY COALESCE(authored_at, filed_at) DESC, filed_at DESC");
        // ===== P1-2 END =====
        sql.push_str(&format!(" LIMIT {}", limit));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let guard = self.conn.lock().expect("conn");
        let mut stmt = guard.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            let id: String = row.get(0)?;
            let content: String = row.get(1)?;
            let metadata_str: String = row.get(2)?;
            let wing: String = row.get(3)?;
            let room: String = row.get(4)?;

            let mut metadata: HashMap<String, Value> =
                serde_json::from_str(&metadata_str).unwrap_or_default();
            if !wing.is_empty() {
                metadata.insert("wing".to_string(), Value::String(wing));
            }
            if !room.is_empty() {
                metadata.insert("room".to_string(), Value::String(room));
            }

            Ok((id, content, metadata))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get a single drawer by ID.
    ///
    /// Returns `Some((content, metadata))` if found, `None` otherwise.
    pub fn get_by_id(&self, id: &str) -> Result<Option<(String, HashMap<String, Value>)>> {
        let guard = self.conn.lock().expect("conn");
        let mut stmt =
            guard.prepare("SELECT content, metadata, wing, room FROM drawers WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            let content: String = row.get(0)?;
            let metadata_str: String = row.get(1)?;
            let wing: String = row.get(2)?;
            let room: String = row.get(3)?;

            let mut metadata: HashMap<String, Value> =
                serde_json::from_str(&metadata_str).unwrap_or_default();
            if !wing.is_empty() {
                metadata.insert("wing".to_string(), Value::String(wing));
            }
            if !room.is_empty() {
                metadata.insert("room".to_string(), Value::String(room));
            }

            Ok(Some((content, metadata)))
        } else {
            Ok(None)
        }
    }

    /// Search drawers using FTS5 MATCH.
    ///
    /// Returns `Vec<(id, content, score)>` ordered by descending BM25 score.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<(String, String, f64)>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Build an FTS5 query from the user's terms.
        // FTS5 supports simple term queries; we join terms with AND for
        // precision. Escape special characters and convert to term queries.
        let fts_query = build_fts_query(query);

        let sql = format!(
            "SELECT drawers.id, drawers.content, bm25(drawers_fts, 0.0, 0.0, 1.0, 1.0) AS score
             FROM drawers_fts
             JOIN drawers ON drawers.rowid = drawers_fts.rowid
             WHERE drawers_fts MATCH ?1
             ORDER BY score
             LIMIT ?2"
        );

        let guard = self.conn.lock().expect("conn");
        let mut stmt = guard.prepare(&sql)?;
        let rows = stmt.query_map(params![fts_query, limit as i64], |row| {
            let id: String = row.get(0)?;
            let content: String = row.get(1)?;
            let score: f64 = row.get(2)?;
            Ok((id, content, score))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Search drawers using FTS5 MATCH and return results with metadata.
    ///
    /// Returns `Vec<(id, content, metadata, score)>` ordered by BM25 score.
    pub fn search_with_metadata(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(String, String, HashMap<String, Value>, f64)>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let fts_query = build_fts_query(query);

        let sql = format!(
            "SELECT drawers.id, drawers.content, drawers.metadata, drawers.wing, drawers.room,
                    bm25(drawers_fts, 0.0, 0.0, 1.0, 1.0) AS score
             FROM drawers_fts
             JOIN drawers ON drawers.rowid = drawers_fts.rowid
             WHERE drawers_fts MATCH ?1
             ORDER BY score
             LIMIT ?2"
        );

        let guard = self.conn.lock().expect("conn");
        let mut stmt = guard.prepare(&sql)?;
        let rows = stmt.query_map(params![fts_query, limit as i64], |row| {
            let id: String = row.get(0)?;
            let content: String = row.get(1)?;
            let metadata_str: String = row.get(2)?;
            let wing: String = row.get(3)?;
            let room: String = row.get(4)?;
            let score: f64 = row.get(5)?;

            let mut metadata: HashMap<String, Value> =
                serde_json::from_str(&metadata_str).unwrap_or_default();
            if !wing.is_empty() {
                metadata.insert("wing".to_string(), Value::String(wing));
            }
            if !room.is_empty() {
                metadata.insert("room".to_string(), Value::String(room));
            }

            Ok((id, content, metadata, score))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Insert a single drawer.
    ///
    /// Extracts `wing`, `room`, and `source_file` from metadata if present.
    pub fn insert(
        &self,
        id: &str,
        content: &str,
        metadata: &HashMap<String, Value>,
        wing: &str,
        room: &str,
        source_file: Option<&str>,
        source_mtime: Option<f64>,
    ) -> Result<()> {
        // ===== P0-3 BEGIN: NUL sanitization (do not edit) =====
        // P0-3: strip NUL bytes / lone surrogates before FTS5 indexing
        // (would corrupt the inverted index via the AFTER INSERT trigger
        // that mirrors content into drawers_fts).
        let content = sanitize_for_fts5(content).into_owned();
        let wing = sanitize_for_fts5(wing).into_owned();
        let room = sanitize_for_fts5(room).into_owned();
        // ===== P0-3 END =====

        let _metadata_json = serde_json::to_string(metadata)?;

        // Strip wing/room from metadata JSON to avoid duplication
        // (they're stored as separate columns)
        let mut clean_meta = metadata.clone();
        clean_meta.remove("wing");
        clean_meta.remove("room");
        // ===== P1-2 BEGIN =====
        // authored_at is a first-class column; keep a copy in metadata JSON
        // for consumers that only read the JSON blob.
        let authored_at = clean_meta
            .get("authored_at")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        // ===== P1-2 END =====
        let clean_meta_json = serde_json::to_string(&clean_meta)?;

        let guard = self.conn.lock().expect("conn");
        // ===== P1-2 / P2-3 BEGIN =====
        // Mid-mine auto-heal: if the FTS5 inverted index is corrupted
        // (common after a killed-mid-write mine), rebuild it from the
        // intact content table and retry the insert once.
        let insert_sql = "INSERT OR REPLACE INTO drawers              (id, content, metadata, wing, room, source_file, source_mtime, authored_at)              VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";
        let first = guard.execute(
            insert_sql,
            params![
                id,
                content,
                clean_meta_json,
                wing,
                room,
                source_file,
                source_mtime,
                authored_at,
            ],
        );
        match first {
            Ok(_) => Ok(()),
            Err(e) if is_fts5_corruption_error(&e) => {
                tracing::warn!(
                    target: "mempalace::drawer_store",
                    error = %e,
                    "FTS5 corruption on insert; attempting in-place rebuild"
                );
                if let Err(heal_err) = rebuild_drawers_fts(&guard) {
                    tracing::warn!(
                        target: "mempalace::drawer_store",
                        error = %heal_err,
                        "FTS5 auto-heal failed"
                    );
                    return Err(e.into());
                }
                guard.execute(
                    insert_sql,
                    params![
                        id,
                        content,
                        clean_meta_json,
                        wing,
                        room,
                        source_file,
                        source_mtime,
                        authored_at,
                    ],
                )?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
        // ===== P1-2 / P2-3 END =====
    }

    /// Batch-insert multiple drawers in a single transaction.
    pub fn insert_batch(
        &self,
        items: &[(
            &str,                    // id
            &str,                    // content
            &HashMap<String, Value>, // metadata
            &str,                    // wing
            &str,                    // room
            Option<&str>,            // source_file
            Option<f64>,             // source_mtime
        )],
    ) -> Result<()> {
        let guard_tx = self.conn.lock().expect("conn");
        let tx = guard_tx.unchecked_transaction()?;
        {
            // ===== P1-2 BEGIN =====
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO drawers \
                 (id, content, metadata, wing, room, source_file, source_mtime, authored_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            // ===== P1-2 END =====

            for &(id, content, metadata, wing, room, source_file, source_mtime) in items {
                // ===== P0-3 BEGIN: NUL sanitization (do not edit) =====
                // P0-3: strip NUL bytes / lone surrogates before FTS5 indexing.
                let content = sanitize_for_fts5(content).into_owned();
                let wing = sanitize_for_fts5(wing).into_owned();
                let room = sanitize_for_fts5(room).into_owned();
                // ===== P0-3 END =====

                let mut clean_meta = metadata.clone();
                clean_meta.remove("wing");
                clean_meta.remove("room");
                // ===== P1-2 BEGIN =====
                let authored_at = clean_meta
                    .get("authored_at")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                // ===== P1-2 END =====
                let clean_meta_json = serde_json::to_string(&clean_meta)?;

                stmt.execute(params![
                    id,
                    content,
                    clean_meta_json,
                    wing,
                    room,
                    source_file,
                    source_mtime,
                    // ===== P1-2 BEGIN =====
                    authored_at,
                    // ===== P1-2 END =====
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    // ===== P1-2 BEGIN =====
    /// Update the `authored_at` column and metadata JSON for a drawer.
    ///
    /// Used by the authored-at backfill migration. Embeddings are untouched.
    pub fn update_authored_at(&self, id: &str, authored_at: &str) -> Result<bool> {
        let guard = self.conn.lock().expect("conn");
        let meta_str: Option<String> = guard
            .query_row(
                "SELECT metadata FROM drawers WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .ok();
        let Some(meta_str) = meta_str else {
            return Ok(false);
        };
        let mut meta: HashMap<String, Value> = serde_json::from_str(&meta_str).unwrap_or_default();
        meta.insert(
            "authored_at".to_string(),
            Value::String(authored_at.to_string()),
        );
        let meta_json = serde_json::to_string(&meta)?;
        let rows = guard.execute(
            "UPDATE drawers SET authored_at = ?1, metadata = ?2 WHERE id = ?3",
            params![authored_at, meta_json, id],
        )?;
        Ok(rows > 0)
    }

    /// Return drawers whose metadata has `ingest_mode = "convos"`.
    ///
    /// Yields `(id, source_file, authored_at_column, metadata_json)` for the
    /// authored-at backfill. `authored_at_column` is the dedicated column
    /// value (may be NULL on pre-migration rows).
    pub fn list_convo_drawers_for_authored_at_backfill(
        &self,
    ) -> Result<Vec<(String, Option<String>, Option<String>, String)>> {
        let guard = self.conn.lock().expect("conn");
        let mut stmt = guard.prepare(
            "SELECT id, source_file, authored_at, metadata FROM drawers \
             WHERE json_extract(metadata, '$.ingest_mode') = 'convos'",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Read the `authored_at` column for a drawer id (test / diagnostics).
    pub fn get_authored_at(&self, id: &str) -> Result<Option<String>> {
        let guard = self.conn.lock().expect("conn");
        let mut stmt = guard.prepare("SELECT authored_at FROM drawers WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(row.get(0)?)
        } else {
            Ok(None)
        }
    }
    // ===== P1-2 END =====

    /// Delete a drawer by ID.
    pub fn delete(&self, id: &str) -> Result<bool> {
        let rows = self
            .conn
            .lock()
            .expect("conn")
            .execute("DELETE FROM drawers WHERE id = ?1", params![id])?;
        Ok(rows > 0)
    }

    /// Delete all drawers that have a given source_file.
    ///
    /// Also purges matching AAAK closet rows (P1-5 / upstream 5ae2315) so
    /// stale closet index pointers cannot surface after a source purge.
    pub fn delete_by_source(&self, source_file: &str) -> Result<usize> {
        let guard = self.conn.lock().expect("conn");
        // ===== P1-5 BEGIN =====
        let drawer_rows = guard.execute(
            "DELETE FROM drawers WHERE source_file = ?1",
            params![source_file],
        )?;
        let _closet_rows = guard.execute(
            "DELETE FROM closets WHERE source_file = ?1",
            params![source_file],
        )?;
        Ok(drawer_rows)
        // ===== P1-5 END =====
    }

    // ===== P1-5 BEGIN =====
    /// Insert a closet (AAAK summary) row. Used by tests and the compress path.
    pub fn insert_closet(
        &self,
        id: &str,
        content: &str,
        source_file: Option<&str>,
        wing: &str,
        room: &str,
    ) -> Result<()> {
        self.conn.lock().expect("conn").execute(
            "INSERT OR REPLACE INTO closets (id, content, source_file, wing, room)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, content, source_file, wing, room],
        )?;
        Ok(())
    }

    /// Count closet rows, optionally filtered by source_file.
    pub fn count_closets(&self, source_file: Option<&str>) -> Result<usize> {
        let guard = self.conn.lock().expect("conn");
        let n: i64 = match source_file {
            Some(src) => guard.query_row(
                "SELECT COUNT(*) FROM closets WHERE source_file = ?1",
                params![src],
                |row| row.get(0),
            )?,
            None => guard.query_row("SELECT COUNT(*) FROM closets", [], |row| row.get(0))?,
        };
        Ok(n as usize)
    }
    // ===== P1-5 END =====

    /// Get all drawers with full column data for export.
    ///
    /// Returns `(id, content, wing, room, source_file, filed_at)` tuples,
    /// optionally filtered by wing. Ordered by `wing, room, filed_at`.
    /// Used by the streaming Markdown exporter.
    pub fn get_all_for_export(
        &self,
        wing: Option<&str>,
    ) -> Result<Vec<(String, String, String, String, Option<String>, String)>> {
        let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
            if let Some(w) = wing {
                (
                    "SELECT id, content, wing, room, source_file, filed_at
                 FROM drawers
                 WHERE wing = ?1
                 ORDER BY wing, room, filed_at"
                        .to_string(),
                    vec![Box::new(w.to_string())],
                )
            } else {
                (
                    "SELECT id, content, wing, room, source_file, filed_at
                 FROM drawers
                 ORDER BY wing, room, filed_at"
                        .to_string(),
                    vec![],
                )
            };

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let guard = self.conn.lock().expect("conn");
        let mut stmt = guard.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Streaming export: iterate all drawers grouped by source_file,
    /// writing one output file per source_file.
    ///
    /// `format` determines the output format. Currently supports
    /// `"basic-memory"` (Obsidian-compatible Markdown) and `"markdown"`.
    pub fn export_stream(&self, output_dir: &Path, format: &str) -> Result<()> {
        let guard = self.conn.lock().expect("conn");
        let mut stmt = guard.prepare(
            "SELECT id, content, metadata, wing, room, source_file, filed_at
             FROM drawers ORDER BY source_file, filed_at",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let content: String = row.get(1)?;
            let metadata_str: String = row.get(2)?;
            let wing: String = row.get(3)?;
            let room: String = row.get(4)?;
            let source_file: Option<String> = row.get(5)?;
            let filed_at: String = row.get(6)?;
            Ok((id, content, metadata_str, wing, room, source_file, filed_at))
        })?;

        let mut current_source: Option<String> = None;
        let mut current_file: Option<std::fs::File> = None;

        for row in rows {
            let (id, content, metadata_str, wing, room, source_file, filed_at) = row?;

            let source = source_file.as_deref().unwrap_or("unknown");

            if current_source.as_deref() != Some(source) {
                // Close previous file
                if let Some(mut f) = current_file.take() {
                    use std::io::Write;
                    let _ = writeln!(f);
                }

                // Open new file for this source
                let safe_name = source.replace('/', "_").replace('\\', "_");
                let out_path = output_dir.join(format!("{}.md", safe_name));
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let file = std::fs::File::create(&out_path)
                    .with_context(|| format!("creating export file {}", out_path.display()))?;
                current_source = Some(source.to_string());
                current_file = Some(file);
            }

            if let Some(ref mut f) = current_file {
                use std::io::Write;
                match format {
                    "basic-memory" | "markdown" => {
                        writeln!(f, "## {}\n", id)?;
                        writeln!(f, "{}\n", content)?;
                        if !wing.is_empty() || !room.is_empty() {
                            writeln!(f, "**Wing:** {} | **Room:** {}", wing, room)?;
                        }
                        writeln!(f, "**Filed:** {} | **Source:** {}", filed_at, source)?;
                        writeln!(f, "---\n")?;
                    }
                    _ => {
                        anyhow::bail!("unknown export format '{}'", format);
                    }
                }
            }
        }

        Ok(())
    }

    /// Migrate from a legacy JSON file containing `HashMap<String, DocumentEntry>`.
    ///
    /// Reads the JSON, batch-inserts all entries into SQLite, and
    /// returns the number of migrated drawers. If the store is
    /// non-empty, migration is skipped (assumed already migrated).
    pub fn migrate_from_json(&self, json_path: &Path) -> Result<usize> {
        if !self.is_empty() {
            info!(
                "drawer store already has {} drawers; skipping JSON migration",
                self.len()
            );
            return Ok(0);
        }

        if !json_path.exists() {
            anyhow::bail!("JSON file not found: {}", json_path.display());
        }

        let content = std::fs::read_to_string(json_path)
            .with_context(|| format!("reading {}", json_path.display()))?;
        let docs: HashMap<String, DocumentEntry> = serde_json::from_str(&content)
            .with_context(|| format!("parsing {}", json_path.display()))?;

        if docs.is_empty() {
            info!("JSON file is empty; nothing to migrate");
            return Ok(0);
        }

        let total = docs.len();
        info!(
            "migrating {} drawers from {} to SQLite",
            total,
            json_path.display()
        );

        // Prepare batch items
        let batch_size = 500;
        let items: Vec<_> = docs
            .iter()
            .map(|(id, entry)| {
                let wing = entry
                    .metadata
                    .get("wing")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let room = entry
                    .metadata
                    .get("room")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let source_file = entry.metadata.get("source_file").and_then(|v| v.as_str());
                let source_mtime = entry.metadata.get("source_mtime").and_then(|v| v.as_f64());

                (
                    id.as_str(),
                    entry.content.as_str(),
                    &entry.metadata,
                    wing,
                    room,
                    source_file,
                    source_mtime,
                )
            })
            .collect();

        // Insert in batches
        for chunk in items.chunks(batch_size) {
            self.insert_batch(chunk)?;
        }

        info!("migrated {} drawers to SQLite", total);
        Ok(total)
    }

    /// Check if FTS5 is available and the drawers_fts table exists.
    pub fn fts5_available(&self) -> bool {
        let guard = self.conn.lock().expect("conn");
        guard
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='drawers_fts' LIMIT 1",
                [],
                |_| Ok(1),
            )
            .is_ok()
    }

    /// Count drawers matching a wing and/or room filter.
    pub fn count_filtered(&self, wing: Option<&str>, room: Option<&str>) -> Result<usize> {
        let mut sql = String::from("SELECT COUNT(*) FROM drawers WHERE 1=1");
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(w) = wing {
            sql.push_str(" AND wing = ?");
            param_values.push(Box::new(w.to_string()));
        }
        if let Some(r) = room {
            sql.push_str(" AND room = ?");
            param_values.push(Box::new(r.to_string()));
        }

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let count: i64 =
            self.conn
                .lock()
                .expect("conn")
                .query_row(&sql, params_refs.as_slice(), |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Get the source_file for a given drawer ID.
    pub fn get_source_file(&self, id: &str) -> Result<Option<String>> {
        let guard = self.conn.lock().expect("conn");
        let mut stmt = guard.prepare("SELECT source_file FROM drawers WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(row.get(0)?)
        } else {
            Ok(None)
        }
    }

    // ===== P0-5 BEGIN: list_filtered with date filter (do not edit) =====
    /// List drawers with optional `filed_at` date range and wing filter.
    ///
    /// Date bounds are half-open on the calendar day:
    /// - `since` is inclusive (`filed_at >= since`)
    /// - `before` is exclusive (`filed_at < before`)
    ///
    /// Results are ordered by `filed_at DESC`, then paginated with
    /// `limit` / `offset`. Returns metadata only (no drawer body).
    pub fn list_filtered(
        &self,
        since: Option<chrono::NaiveDate>,
        before: Option<chrono::NaiveDate>,
        wing: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<DrawerRow>> {
        let mut sql = String::from(
            "SELECT id, content, metadata, source_file, filed_at, wing \
             FROM drawers WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        // Stable bind order: since, before, wing, limit, offset.
        if let Some(d) = since {
            sql.push_str(" AND filed_at >= ?");
            param_values.push(Box::new(d.format("%Y-%m-%d").to_string()));
        }
        if let Some(d) = before {
            sql.push_str(" AND filed_at < ?");
            param_values.push(Box::new(d.format("%Y-%m-%d").to_string()));
        }
        if let Some(w) = wing {
            sql.push_str(" AND wing = ?");
            param_values.push(Box::new(w.to_string()));
        }
        // ===== P1-2 BEGIN =====
        sql.push_str(" ORDER BY COALESCE(authored_at, filed_at) DESC, filed_at DESC");
        // ===== P1-2 END =====
        sql.push_str(" LIMIT ? OFFSET ?");
        param_values.push(Box::new(limit as i64));
        param_values.push(Box::new(offset as i64));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let guard = self.conn.lock().expect("conn");
        let mut stmt = guard.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            let id: String = row.get(0)?;
            let content: String = row.get(1)?;
            let metadata_str: String = row.get(2)?;
            let source_file: Option<String> = row.get(3)?;
            let filed_at: String = row.get(4)?;
            let wing: String = row.get(5)?;

            let title = title_from_metadata_or_content(&metadata_str, &content);

            Ok(DrawerRow {
                id,
                title,
                source_file,
                filed_at,
                wing,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }
    // ===== P0-5 END =====
}

/// Derive a short list title from metadata `title` or the first line of content.
fn title_from_metadata_or_content(metadata_str: &str, content: &str) -> String {
    if let Ok(meta) = serde_json::from_str::<HashMap<String, Value>>(metadata_str) {
        if let Some(t) = meta.get("title").and_then(|v| v.as_str()) {
            let t = t.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    let first_line = content.lines().next().unwrap_or("").trim();
    if first_line.chars().count() > 80 {
        first_line.chars().take(80).collect()
    } else {
        first_line.to_string()
    }
}

// ===== P1-2 BEGIN =====
/// Ensure the nullable `authored_at` column exists on `drawers`.
///
/// Safe to call on every open: if the column is already present the ALTER
/// is skipped. Older palaces created before P1-2 only have `filed_at`.
fn ensure_authored_at_column(conn: &Connection) -> Result<()> {
    let has_col: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('drawers') WHERE name='authored_at' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if !has_col {
        conn.execute("ALTER TABLE drawers ADD COLUMN authored_at TEXT", [])?;
    }
    Ok(())
}
// ===== P1-2 END =====

/// Build an FTS5 query string from user input.
///
/// Escapes special FTS5 characters and joins terms with AND for
/// precision matching. Empty/malformed terms are filtered out.
fn build_fts_query(user_query: &str) -> String {
    // FTS5 special characters: ^, *, ", :, ~, (, ), +
    // We escape by wrapping each term in double quotes
    let terms: Vec<String> = user_query
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| {
            // Escape any double quotes in the term
            let escaped = t.replace('"', "\"\"");
            format!("\"{}\"", escaped)
        })
        .collect();

    if terms.is_empty() {
        return String::new();
    }

    terms.join(" AND ")
}

// ===== P2-1 BEGIN =====
/// SQLite on-disk header magic: 16 bytes `"SQLite format 3\0"`.
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// If `db_path` already exists, require a valid SQLite magic header.
/// Missing files are fine (rusqlite will create them). Empty / garbage
/// files fail fast with an actionable error (P2-1 / upstream #1893).
fn verify_sqlite_magic_header(db_path: &Path) -> Result<()> {
    if !db_path.exists() {
        return Ok(());
    }
    let meta = std::fs::metadata(db_path)
        .with_context(|| format!("failed to stat {}", db_path.display()))?;
    if meta.len() == 0 {
        anyhow::bail!(
            "drawer store at {} is empty (0 bytes); expected a SQLite database.              Delete the file and re-run `mpr init` / `mpr mine`, or restore from backup.",
            db_path.display()
        );
    }
    let mut f = std::fs::File::open(db_path)
        .with_context(|| format!("failed to open {} for magic check", db_path.display()))?;
    use std::io::Read;
    let mut magic = [0u8; 16];
    f.read_exact(&mut magic).with_context(|| {
        format!(
            "drawer store at {} is too short to be a SQLite database",
            db_path.display()
        )
    })?;
    if &magic != SQLITE_MAGIC {
        anyhow::bail!(
            "drawer store at {} is not a SQLite database (bad magic header). Expected the 16-byte SQLite format 3 magic. Restore from backup or delete and re-mine.",
            db_path.display()
        );
    }
    Ok(())
}
// ===== P2-1 END =====

// ===== P2-3 BEGIN =====
/// True when a rusqlite error message indicates FTS5 inverted-index
/// corruption (recoverable by rebuild). Matches both legacy and modern
/// SQLite wordings (P2-5 strings, reused mid-mine).
fn is_fts5_corruption_error(err: &rusqlite::Error) -> bool {
    crate::repair::is_fts5_corruption(&err.to_string())
}

/// Rebuild the `drawers_fts` virtual table from the intact `drawers`
/// content. Idempotent; used by mid-mine auto-heal (P2-3).
fn rebuild_drawers_fts(conn: &Connection) -> Result<()> {
    // Documented FTS5 rebuild command — regenerates the inverted index
    // from the content= table without touching drawer rows.
    conn.execute_batch("INSERT INTO drawers_fts(drawers_fts) VALUES('rebuild')")
        .context("FTS5 rebuild command failed")?;
    Ok(())
}
// ===== P2-3 END =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_creates_schema() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();
        assert!(store.is_empty());

        // Verify schema exists
        let guard = store.conn.lock().expect("conn");
        let has_drawers: bool = guard
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='drawers'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .is_ok();
        assert!(has_drawers);

        let has_fts: bool = guard
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='drawers_fts'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .is_ok();
        assert!(has_fts);
    }

    #[test]
    fn test_insert_and_count() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();
        assert_eq!(store.len(), 0);

        let mut meta = HashMap::new();
        meta.insert("key1".to_string(), Value::String("val1".to_string()));

        store
            .insert("test-1", "hello world", &meta, "wing1", "room1", None, None)
            .unwrap();
        assert_eq!(store.len(), 1);

        store
            .insert("test-2", "foo bar", &HashMap::new(), "", "", None, None)
            .unwrap();
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_fts_search() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();

        store
            .insert(
                "d1",
                "the quick brown fox",
                &HashMap::new(),
                "animals",
                "mammals",
                None,
                None,
            )
            .unwrap();
        store
            .insert(
                "d2",
                "jumped over the lazy dog",
                &HashMap::new(),
                "animals",
                "mammals",
                None,
                None,
            )
            .unwrap();
        store
            .insert(
                "d3",
                "Rust programming language",
                &HashMap::new(),
                "tech",
                "languages",
                None,
                None,
            )
            .unwrap();

        // Search for "fox" should find d1
        let results = store.search("fox", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "d1");

        // Search for "dog" should find d2
        let results = store.search("dog", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "d2");

        // Search for "Rust" should find d3
        let results = store.search("Rust", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "d3");

        // Search for "quick fox" (AND) should find d1
        let results = store.search("quick fox", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "d1");
    }

    #[test]
    fn test_get_by_id() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();

        let mut meta = HashMap::new();
        meta.insert("source".to_string(), Value::String("test.txt".to_string()));

        store
            .insert(
                "my-id",
                "some content",
                &meta,
                "w1",
                "r1",
                Some("src.txt"),
                Some(12345.0),
            )
            .unwrap();

        let result = store.get_by_id("my-id").unwrap();
        assert!(result.is_some());
        let (content, metadata) = result.unwrap();
        assert_eq!(content, "some content");
        assert_eq!(
            metadata.get("source").and_then(|v| v.as_str()),
            Some("test.txt")
        );
        assert_eq!(metadata.get("wing").and_then(|v| v.as_str()), Some("w1"));
        assert_eq!(metadata.get("room").and_then(|v| v.as_str()), Some("r1"));

        // Non-existent ID
        let result = store.get_by_id("nope").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();

        store
            .insert(
                "del-me",
                "to be deleted",
                &HashMap::new(),
                "",
                "",
                None,
                None,
            )
            .unwrap();
        assert_eq!(store.len(), 1);

        assert!(store.delete("del-me").unwrap());
        assert_eq!(store.len(), 0);

        // Deleting non-existent returns false
        assert!(!store.delete("nope").unwrap());
    }

    #[test]
    fn test_delete_by_source() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();

        store
            .insert(
                "a",
                "content a",
                &HashMap::new(),
                "",
                "",
                Some("src1"),
                None,
            )
            .unwrap();
        store
            .insert(
                "b",
                "content b",
                &HashMap::new(),
                "",
                "",
                Some("src1"),
                None,
            )
            .unwrap();
        store
            .insert(
                "c",
                "content c",
                &HashMap::new(),
                "",
                "",
                Some("src2"),
                None,
            )
            .unwrap();
        assert_eq!(store.len(), 3);

        assert_eq!(store.delete_by_source("src1").unwrap(), 2);
        assert_eq!(store.len(), 1);
        assert!(store.get_by_id("c").unwrap().is_some());
    }

    // ===== P1-2 BEGIN =====
    #[test]
    fn test_p1_2_insert_stores_authored_at_column() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();

        let mut meta = HashMap::new();
        meta.insert(
            "authored_at".to_string(),
            Value::String("2024-03-01T12:00:00Z".to_string()),
        );
        meta.insert(
            "ingest_mode".to_string(),
            Value::String("convos".to_string()),
        );
        store
            .insert(
                "d-authored",
                "hello authored",
                &meta,
                "wing",
                "room",
                Some("session.jsonl"),
                None,
            )
            .unwrap();

        assert_eq!(
            store.get_authored_at("d-authored").unwrap().as_deref(),
            Some("2024-03-01T12:00:00Z")
        );

        // load_all_to_hashmap surfaces authored_at in metadata.
        let docs = store.load_all_to_hashmap().unwrap();
        let entry = docs.get("d-authored").expect("drawer present");
        assert_eq!(
            entry.metadata.get("authored_at").and_then(|v| v.as_str()),
            Some("2024-03-01T12:00:00Z")
        );
    }

    #[test]
    fn test_p1_2_get_all_orders_by_coalesce_authored_at() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();

        // Older authored_at, filed later.
        let mut meta_old = HashMap::new();
        meta_old.insert(
            "authored_at".to_string(),
            Value::String("2020-01-01T00:00:00Z".to_string()),
        );
        store
            .insert("old", "old content", &meta_old, "w", "r", None, None)
            .unwrap();

        // Newer authored_at.
        let mut meta_new = HashMap::new();
        meta_new.insert(
            "authored_at".to_string(),
            Value::String("2025-06-01T00:00:00Z".to_string()),
        );
        store
            .insert("new", "new content", &meta_new, "w", "r", None, None)
            .unwrap();

        // No authored_at — falls back to filed_at (datetime('now')), so it
        // should sit after the 2025 authored drawer when COALESCE is used
        // only if filed_at is earlier; pin filed_at to be mid-range.
        store
            .insert("mid", "mid content", &HashMap::new(), "w", "r", None, None)
            .unwrap();
        {
            let guard = store.conn.lock().expect("conn");
            guard
                .execute(
                    "UPDATE drawers SET filed_at = ?1 WHERE id = ?2",
                    params!["2022-01-01 00:00:00", "mid"],
                )
                .unwrap();
            // Also pin the others' filed_at so ORDER is driven by authored_at.
            guard
                .execute(
                    "UPDATE drawers SET filed_at = ?1 WHERE id = ?2",
                    params!["2024-01-01 00:00:00", "old"],
                )
                .unwrap();
            guard
                .execute(
                    "UPDATE drawers SET filed_at = ?1 WHERE id = ?2",
                    params!["2024-01-02 00:00:00", "new"],
                )
                .unwrap();
        }

        let rows = store.get_all(Some("w"), None, 10).unwrap();
        let ids: Vec<&str> = rows.iter().map(|(id, _, _)| id.as_str()).collect();
        // COALESCE(authored_at, filed_at) DESC:
        //   new  -> 2025-06-01
        //   mid  -> 2022-01-01 (filed_at only)
        //   old  -> 2020-01-01
        assert_eq!(ids, vec!["new", "mid", "old"]);
    }

    #[test]
    fn test_p1_2_alter_table_on_legacy_schema() {
        // Open a pre-P1-2 DB (no authored_at column) and ensure open() adds it.
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("drawers.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE drawers (
                    id TEXT PRIMARY KEY,
                    content TEXT NOT NULL,
                    metadata TEXT NOT NULL DEFAULT '{}',
                    wing TEXT NOT NULL DEFAULT '',
                    room TEXT NOT NULL DEFAULT '',
                    source_file TEXT,
                    filed_at TEXT NOT NULL DEFAULT (datetime('now')),
                    source_mtime REAL
                );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO drawers (id, content) VALUES ('legacy', 'hello')",
                [],
            )
            .unwrap();
        }
        let store = DrawerStore::open(temp.path()).unwrap();
        // Column must exist and be nullable (legacy row has NULL).
        assert_eq!(store.get_authored_at("legacy").unwrap(), None);
        // Insert with authored_at still works.
        let mut meta = HashMap::new();
        meta.insert(
            "authored_at".to_string(),
            Value::String("2021-01-01T00:00:00Z".to_string()),
        );
        store
            .insert("fresh", "body", &meta, "w", "r", None, None)
            .unwrap();
        assert_eq!(
            store.get_authored_at("fresh").unwrap().as_deref(),
            Some("2021-01-01T00:00:00Z")
        );
    }
    // ===== P1-2 END =====

    // ===== P1-5 BEGIN =====
    #[test]
    fn test_p1_5_delete_by_source_also_purges_closets() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();

        for i in 0..5 {
            store
                .insert(
                    &format!("d{i}"),
                    &format!("drawer content {i} needle from foo"),
                    &HashMap::new(),
                    "wing",
                    "room",
                    Some("foo.txt"),
                    None,
                )
                .unwrap();
        }
        store
            .insert(
                "other",
                "other drawer",
                &HashMap::new(),
                "wing",
                "room",
                Some("bar.txt"),
                None,
            )
            .unwrap();

        for i in 0..3 {
            store
                .insert_closet(
                    &format!("c{i}"),
                    &format!("closet summary {i} needle from foo"),
                    Some("foo.txt"),
                    "wing",
                    "room",
                )
                .unwrap();
        }
        store
            .insert_closet("c-other", "closet other", Some("bar.txt"), "wing", "room")
            .unwrap();

        assert_eq!(store.count_closets(Some("foo.txt")).unwrap(), 3);
        assert_eq!(store.count_closets(None).unwrap(), 4);

        let deleted = store.delete_by_source("foo.txt").unwrap();
        assert_eq!(deleted, 5);
        assert_eq!(store.len(), 1);
        assert_eq!(store.count_closets(Some("foo.txt")).unwrap(), 0);
        assert_eq!(store.count_closets(Some("bar.txt")).unwrap(), 1);
        assert!(store.get_by_id("other").unwrap().is_some());
    }
    // ===== P1-5 END =====

    #[test]
    fn test_get_all_filtered() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();

        store
            .insert(
                "a",
                "content a",
                &HashMap::new(),
                "wing1",
                "room1",
                None,
                None,
            )
            .unwrap();
        store
            .insert(
                "b",
                "content b",
                &HashMap::new(),
                "wing1",
                "room2",
                None,
                None,
            )
            .unwrap();
        store
            .insert(
                "c",
                "content c",
                &HashMap::new(),
                "wing2",
                "room1",
                None,
                None,
            )
            .unwrap();

        // All
        let all = store.get_all(None, None, 10).unwrap();
        assert_eq!(all.len(), 3);

        // Filter by wing
        let wing1 = store.get_all(Some("wing1"), None, 10).unwrap();
        assert_eq!(wing1.len(), 2);

        // Filter by wing + room
        let specific = store.get_all(Some("wing1"), Some("room1"), 10).unwrap();
        assert_eq!(specific.len(), 1);
    }

    /// P0-5: date-range filter on `filed_at` is half-open and ordered DESC.
    #[test]
    fn list_filtered_date_range() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();

        // Seed 5 drawers with filed_at spread across 3 different days.
        // day1: d1, d2  |  day2: d3, d4  |  day3: d5
        let seeds: &[(&str, &str)] = &[
            ("d1", "2026-04-01 10:00:00"),
            ("d2", "2026-04-01 18:00:00"),
            ("d3", "2026-04-02 09:00:00"),
            ("d4", "2026-04-02 15:30:00"),
            ("d5", "2026-04-03 12:00:00"),
        ];
        for (id, filed_at) in seeds {
            store
                .insert(
                    id,
                    &format!("content of {id}"),
                    &HashMap::new(),
                    "wing_a",
                    "room_a",
                    Some(&format!("src/{id}.txt")),
                    None,
                )
                .unwrap();
            // Override filed_at (insert uses datetime('now') default).
            store
                .conn
                .lock()
                .expect("conn")
                .execute(
                    "UPDATE drawers SET filed_at = ?1 WHERE id = ?2",
                    params![filed_at, id],
                )
                .unwrap();
        }

        let day2 = chrono::NaiveDate::from_ymd_opt(2026, 4, 2).unwrap();
        let day3 = chrono::NaiveDate::from_ymd_opt(2026, 4, 3).unwrap();

        // since=day2, before=day3 → drawers on day2 only (d3, d4).
        let rows = store
            .list_filtered(Some(day2), Some(day3), None, 100, 0)
            .unwrap();
        assert_eq!(
            rows.len(),
            2,
            "expected 2 drawers in [day2, day3); got {:?}",
            rows.iter().map(|r| &r.id).collect::<Vec<_>>()
        );
        // ORDER BY filed_at DESC → d4 (15:30) before d3 (09:00).
        assert_eq!(rows[0].id, "d4");
        assert_eq!(rows[1].id, "d3");
        assert_eq!(rows[0].wing, "wing_a");
        assert_eq!(rows[0].source_file.as_deref(), Some("src/d4.txt"));
        assert!(!rows[0].title.is_empty());
    }

    #[test]
    fn test_load_all_to_hashmap() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();

        let mut meta = HashMap::new();
        meta.insert("extra".to_string(), Value::String("value".to_string()));

        store
            .insert("id1", "hello", &meta, "w1", "r1", None, None)
            .unwrap();
        store
            .insert("id2", "world", &HashMap::new(), "", "", None, None)
            .unwrap();

        let map = store.load_all_to_hashmap().unwrap();
        assert_eq!(map.len(), 2);

        let entry1 = map.get("id1").unwrap();
        assert_eq!(entry1.content, "hello");
        assert_eq!(
            entry1.metadata.get("wing").and_then(|v| v.as_str()),
            Some("w1")
        );
        assert_eq!(
            entry1.metadata.get("extra").and_then(|v| v.as_str()),
            Some("value")
        );
    }

    #[test]
    fn test_batch_insert() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();

        let empty_meta = HashMap::new();
        let items = vec![
            (
                "a",
                "alpha",
                &empty_meta,
                "w1",
                "r1",
                None as Option<&str>,
                None as Option<f64>,
            ),
            ("b", "beta", &empty_meta, "w1", "r1", None, None),
            ("c", "gamma", &empty_meta, "w1", "r2", None, None),
        ];

        store.insert_batch(&items).unwrap();
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn test_count_filtered() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();

        store
            .insert("a", "a", &HashMap::new(), "w1", "r1", None, None)
            .unwrap();
        store
            .insert("b", "b", &HashMap::new(), "w1", "r1", None, None)
            .unwrap();
        store
            .insert("c", "c", &HashMap::new(), "w1", "r2", None, None)
            .unwrap();

        assert_eq!(store.count_filtered(None, None).unwrap(), 3);
        assert_eq!(store.count_filtered(Some("w1"), None).unwrap(), 3);
        assert_eq!(store.count_filtered(Some("w1"), Some("r1")).unwrap(), 2);
        assert_eq!(store.count_filtered(Some("w1"), Some("r2")).unwrap(), 1);
        assert_eq!(store.count_filtered(Some("w2"), None).unwrap(), 0);
    }

    #[test]
    fn test_fts5_available() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();
        assert!(store.fts5_available());
    }

    #[test]
    fn test_migrate_from_json() {
        let temp = tempfile::tempdir().unwrap();

        // Create a legacy JSON file
        let mut docs = HashMap::new();
        let mut meta = HashMap::new();
        meta.insert("wing".to_string(), Value::String("test".to_string()));
        meta.insert("room".to_string(), Value::String("migration".to_string()));
        docs.insert(
            "legacy-1".to_string(),
            DocumentEntry {
                content: "legacy content".to_string(),
                metadata: meta.clone(),
            },
        );
        docs.insert(
            "legacy-2".to_string(),
            DocumentEntry {
                content: "more legacy".to_string(),
                metadata: meta,
            },
        );

        let json_path = temp.path().join("legacy.json");
        let json_content = serde_json::to_string_pretty(&docs).unwrap();
        std::fs::write(&json_path, &json_content).unwrap();

        // Migrate
        let store = DrawerStore::open(temp.path()).unwrap();
        let count = store.migrate_from_json(&json_path).unwrap();
        assert_eq!(count, 2);
        assert_eq!(store.len(), 2);

        // Verify data
        let (content, _) = store.get_by_id("legacy-1").unwrap().unwrap();
        assert_eq!(content, "legacy content");

        // Second migration should be a no-op
        let count2 = store.migrate_from_json(&json_path).unwrap();
        assert_eq!(count2, 0);
    }

    /// P0-3: A drawer whose content contains a NUL byte must still be
    /// retrievable via FTS5 MATCH after sanitization strips the NUL.
    #[test]
    fn nul_byte_does_not_corrupt_fts5() {
        let temp = tempfile::tempdir().unwrap();
        let store = DrawerStore::open(temp.path()).unwrap();

        // Insert a drawer whose content has an embedded NUL byte.
        // Spaces around the NULs keep the tokens distinct after strip.
        let dirty = "hello\0 world\0 rust";
        store
            .insert(
                "nul-1",
                dirty,
                &HashMap::new(),
                "wing\0bad",
                "room\0bad",
                None,
                None,
            )
            .expect("insert should accept dirty input");

        // The NUL-stripped content must be present in `drawers.content`.
        let (stored_content, _) = store.get_by_id("nul-1").unwrap().unwrap();
        assert_eq!(stored_content, "hello world rust");
        assert!(!stored_content.contains('\0'));

        // The same wing/room columns should be sanitized.
        let guard = store.conn.lock().expect("conn");
        let (wing, room): (String, String) = guard
            .query_row(
                "SELECT wing, room FROM drawers WHERE id = ?1",
                params!["nul-1"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        drop(guard);
        assert_eq!(wing, "wingbad");
        assert_eq!(room, "roombad");

        // Search for "world" — must find the drawer via FTS5 MATCH.
        let hits = store.search("world", 10).unwrap();
        assert!(
            hits.iter().any(|(id, _, _)| id == "nul-1"),
            "expected FTS5 hit for sanitized drawer; got: {:?}",
            hits
        );
    }

    // ===== P2-1 BEGIN =====
    #[test]
    fn test_p2_1_rejects_non_sqlite_magic() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("drawers.db");
        std::fs::write(&db_path, b"this is not a sqlite file!!!!").unwrap();
        // DrawerStore is not Debug; match instead of unwrap_err().
        let msg = match DrawerStore::open(temp.path()) {
            Ok(_) => panic!("expected magic-header rejection"),
            Err(e) => format!("{e:#}"),
        };
        assert!(
            msg.contains("not a SQLite database") || msg.contains("bad magic"),
            "expected magic-header error, got: {msg}"
        );
    }

    #[test]
    fn test_p2_1_rejects_empty_db_file() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("drawers.db");
        std::fs::write(&db_path, b"").unwrap();
        let msg = match DrawerStore::open(temp.path()) {
            Ok(_) => panic!("expected empty-file rejection"),
            Err(e) => format!("{e:#}"),
        };
        assert!(
            msg.contains("empty") || msg.contains("0 bytes"),
            "expected empty-file error, got: {msg}"
        );
    }

    #[test]
    fn test_p2_1_accepts_valid_sqlite() {
        let temp = tempfile::tempdir().unwrap();
        // Create a real SQLite DB first so magic is valid.
        {
            let p = temp.path().join("drawers.db");
            let conn = Connection::open(&p).unwrap();
            conn.execute_batch("CREATE TABLE t (id INTEGER);").unwrap();
        }
        // Opening via DrawerStore should succeed (schema migrate path).
        match DrawerStore::open(temp.path()) {
            Ok(store) => assert_eq!(store.len(), 0),
            Err(e) => panic!("valid sqlite must open: {e:#}"),
        }
    }
    // ===== P2-1 END =====

    // ===== P2-3 BEGIN =====
    #[test]
    fn test_p2_3_is_fts5_corruption_error_matches() {
        // Wire through the shared repair helper.
        assert!(crate::repair::is_fts5_corruption(
            "malformed inverted index for FTS5 table main.drawers_fts"
        ));
        assert!(crate::repair::is_fts5_corruption(
            "database disk image is malformed"
        ));
        assert!(crate::repair::is_fts5_corruption(
            "fts5: corruption found reading blob 3"
        ));
        assert!(!crate::repair::is_fts5_corruption("database is locked"));
    }
    // ===== P2-3 END =====
}
