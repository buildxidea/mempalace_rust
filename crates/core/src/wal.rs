//! Write-ahead log (WAL) for MemPalace mutation audit.
//!
//! SQLite-backed audit trail of palace mutations. Every write operation
//! (add/delete/update/sync) is recorded here with metadata for replay and
//! forensics. Thread-safe via `tokio::sync::Mutex`.
//!
//! This lives in its own module so callers that only need WAL audit logging
//! -- the CLI ``sync`` path and the daemon's ``service`` layer -- can obtain
//! a ``WalStore`` without importing ``mcp_server`` (which runs module-level
//! stdio guards at startup).

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::sync::Mutex;

/// Default retention period for WAL entries (90 days).
const DEFAULT_RETENTION_DAYS: u64 = 90;

/// A single write-ahead log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalEntry {
    /// Unique identifier (UUID v4).
    pub id: String,
    /// ISO-8601 timestamp of the operation.
    pub timestamp: DateTime<Utc>,
    /// Operation type: add, delete, update, sync.
    pub operation: String,
    /// Target identifier (drawer ID, memory ID, entity name, etc.).
    pub target: String,
    /// Arbitrary JSON metadata about the operation.
    pub metadata: serde_json::Value,
    /// Source context (e.g., MCP tool name, CLI command).
    pub source_file: String,
}

/// SQLite-backed write-ahead log store.
///
/// Thread-safe via `tokio::sync::Mutex`. All access is synchronous via
/// `blocking_lock()` to avoid `Send` issues with `rusqlite::Connection`.
pub struct WalStore {
    conn: Mutex<rusqlite::Connection>,
}

impl WalStore {
    /// Open or create the WAL database at `db_path`.
    ///
    /// Creates parent directories if they do not exist.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = db_path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = rusqlite::Connection::open(db_path)?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory WAL store (for testing).
    pub fn in_memory() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Run schema migrations.
    fn migrate(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS wal_entries (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                operation TEXT NOT NULL,
                target TEXT NOT NULL DEFAULT '',
                metadata TEXT NOT NULL DEFAULT '{}',
                source_file TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_wal_timestamp ON wal_entries(timestamp);
            CREATE INDEX IF NOT EXISTS idx_wal_operation ON wal_entries(operation);",
        )?;
        Ok(())
    }

    /// Record a WAL entry.
    ///
    /// `metadata` defaults to an empty JSON object `{}` when `None`.
    pub fn record(
        &self,
        operation: &str,
        target: &str,
        metadata: Option<serde_json::Value>,
        source_file: &str,
    ) -> Result<WalEntry> {
        let entry = WalEntry {
            id: format!("wal-{}", uuid::Uuid::new_v4()),
            timestamp: Utc::now(),
            operation: operation.to_string(),
            target: target.to_string(),
            metadata: metadata.unwrap_or(serde_json::Value::Object(Default::default())),
            source_file: source_file.to_string(),
        };

        let conn = self.conn.blocking_lock();
        conn.execute(
            "INSERT INTO wal_entries (id, timestamp, operation, target, metadata, source_file)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                entry.id,
                entry.timestamp.to_rfc3339(),
                entry.operation,
                entry.target,
                serde_json::to_string(&entry.metadata)?,
                entry.source_file,
            ],
        )?;
        Ok(entry)
    }

    /// List the most recent WAL entries, newest first.
    pub fn list_recent(&self, limit: usize) -> Result<Vec<WalEntry>> {
        let conn = self.conn.blocking_lock();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, operation, target, metadata, source_file
             FROM wal_entries ORDER BY timestamp DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], parse_wal_row)?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    /// Filter WAL entries by operation type and/or date range.
    ///
    /// All filters are optional; results are sorted newest-first and capped
    /// at `limit`.
    #[allow(clippy::needless_pass_by_value)]
    pub fn filter(
        &self,
        operation: Option<&str>,
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<WalEntry>> {
        let conn = self.conn.blocking_lock();

        let mut sql = String::from(
            "SELECT id, timestamp, operation, target, metadata, source_file FROM wal_entries WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(op) = operation {
            sql.push_str(" AND operation = ?");
            param_values.push(Box::new(op.to_string()));
        }
        if let Some(from) = date_from {
            sql.push_str(" AND timestamp >= ?");
            param_values.push(Box::new(from.to_rfc3339()));
        }
        if let Some(to) = date_to {
            sql.push_str(" AND timestamp <= ?");
            param_values.push(Box::new(to.to_rfc3339()));
        }

        sql.push_str(" ORDER BY timestamp DESC LIMIT ?");
        param_values.push(Box::new(limit as i64));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), parse_wal_row)?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    /// Prune entries older than `retention_days` (default: 90).
    ///
    /// Returns the number of deleted entries.
    pub fn prune(&self, retention_days: Option<u64>) -> Result<usize> {
        let days = retention_days.unwrap_or(DEFAULT_RETENTION_DAYS);
        let cutoff = Utc::now() - chrono::Duration::days(days as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let conn = self.conn.blocking_lock();
        let deleted = conn.execute(
            "DELETE FROM wal_entries WHERE timestamp < ?1",
            params![cutoff_str],
        )?;
        Ok(deleted)
    }

    /// Count total WAL entries.
    pub fn count(&self) -> Result<i64> {
        let conn = self.conn.blocking_lock();
        conn.query_row("SELECT COUNT(*) FROM wal_entries", [], |row| row.get(0))
            .map_err(|e| anyhow::anyhow!("failed to count WAL entries: {e}"))
    }

    /// Prune entries older than `retention_days` using an explicit cutoff.
    ///
    /// This is an alternative to `prune()` that accepts a `DateTime` directly
    /// for callers that already have a computed cutoff.
    pub fn prune_before(&self, cutoff: DateTime<Utc>) -> Result<usize> {
        let conn = self.conn.blocking_lock();
        let deleted = conn.execute(
            "DELETE FROM wal_entries WHERE timestamp < ?1",
            params![cutoff.to_rfc3339()],
        )?;
        Ok(deleted)
    }

    /// Delete all WAL entries (for testing / reset).
    pub fn clear(&self) -> Result<usize> {
        let conn = self.conn.blocking_lock();
        let deleted = conn.execute("DELETE FROM wal_entries", [])?;
        Ok(deleted)
    }
}

/// Parse a WAL entry row from SQLite query results.
fn parse_wal_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WalEntry> {
    let metadata_str: String = row.get(4)?;
    let metadata: serde_json::Value =
        serde_json::from_str(&metadata_str).unwrap_or(serde_json::Value::Object(Default::default()));

    Ok(WalEntry {
        id: row.get(0)?,
        timestamp: row.get::<_, String>(1).and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })
        })?,
        operation: row.get(2)?,
        target: row.get(3)?,
        metadata,
        source_file: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> WalStore {
        WalStore::in_memory().unwrap()
    }

    #[test]
    fn test_record_and_list_recent() {
        let store = make_store();
        let entry = store
            .record("add", "drawer-123", None, "mcp::add_drawer")
            .unwrap();
        assert_eq!(entry.operation, "add");
        assert_eq!(entry.target, "drawer-123");
        assert!(entry.id.starts_with("wal-"));

        let recent = store.list_recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, entry.id);
    }

    #[test]
    fn test_record_with_metadata() {
        let store = make_store();
        let meta = serde_json::json!({"wing": "test", "room": "testing"});
        let entry = store
            .record("update", "drawer-456", Some(meta.clone()), "cli::mine")
            .unwrap();
        assert_eq!(entry.metadata, meta);
        assert_eq!(entry.source_file, "cli::mine");
    }

    #[test]
    fn test_list_recent_newest_first() {
        let store = make_store();
        store
            .record("add", "first", None, "test")
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        store
            .record("add", "second", None, "test")
            .unwrap();

        let recent = store.list_recent(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].target, "second");
        assert_eq!(recent[1].target, "first");
    }

    #[test]
    fn test_list_recent_respects_limit() {
        let store = make_store();
        for i in 0..10 {
            store
                .record("add", &format!("drawer-{i}"), None, "test")
                .unwrap();
        }
        let limited = store.list_recent(3).unwrap();
        assert_eq!(limited.len(), 3);
    }

    #[test]
    fn test_filter_by_operation() {
        let store = make_store();
        store.record("add", "a", None, "test").unwrap();
        store.record("delete", "b", None, "test").unwrap();
        store.record("add", "c", None, "test").unwrap();
        store.record("sync", "d", None, "test").unwrap();

        let adds = store.filter(Some("add"), None, None, 100).unwrap();
        assert_eq!(adds.len(), 2);
        assert!(adds.iter().all(|e| e.operation == "add"));

        let deletes = store.filter(Some("delete"), None, None, 100).unwrap();
        assert_eq!(deletes.len(), 1);
    }

    #[test]
    fn test_filter_by_date_range() {
        let store = make_store();
        store.record("add", "a", None, "test").unwrap();
        store.record("add", "b", None, "test").unwrap();

        let now = Utc::now();
        let all = store.filter(None, None, None, 100).unwrap();
        assert_eq!(all.len(), 2);

        let after = store
            .filter(None, Some(now - chrono::Duration::hours(1)), None, 100)
            .unwrap();
        assert_eq!(after.len(), 2);

        let before = store
            .filter(None, None, Some(now - chrono::Duration::hours(1)), 100)
            .unwrap();
        assert_eq!(before.len(), 0);
    }

    #[test]
    fn test_prune_removes_old_entries() {
        let store = make_store();

        // Insert an entry with an explicit old timestamp by writing raw SQL.
        {
            let conn = store.conn.blocking_lock();
            conn.execute(
                "INSERT INTO wal_entries (id, timestamp, operation, target, metadata, source_file)
                 VALUES ('wal-old', '2020-01-01T00:00:00Z', 'add', 'old-drawer', '{}', 'test')",
                [],
            )
            .unwrap();
        }

        store.record("add", "new-drawer", None, "test").unwrap();

        let deleted = store.prune(Some(1)).unwrap();
        assert_eq!(deleted, 1, "should prune exactly the old entry");

        let remaining = store.list_recent(100).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].target, "new-drawer");
    }

    #[test]
    fn test_prune_before_removes_entries_before_cutoff() {
        let store = make_store();
        {
            let conn = store.conn.blocking_lock();
            conn.execute(
                "INSERT INTO wal_entries (id, timestamp, operation, target, metadata, source_file)
                 VALUES ('wal-old', '2020-06-15T00:00:00Z', 'add', 'old', '{}', 'test')",
                [],
            )
            .unwrap();
        }
        store.record("add", "current", None, "test").unwrap();

        let cutoff = chrono::DateTime::parse_from_rfc3339("2023-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let deleted = store.prune_before(cutoff).unwrap();
        assert_eq!(deleted, 1);
    }

    #[test]
    fn test_count() {
        let store = make_store();
        assert_eq!(store.count().unwrap(), 0);
        store.record("add", "a", None, "test").unwrap();
        assert_eq!(store.count().unwrap(), 1);
        store.record("delete", "b", None, "test").unwrap();
        assert_eq!(store.count().unwrap(), 2);
    }

    #[test]
    fn test_clear() {
        let store = make_store();
        store.record("add", "a", None, "test").unwrap();
        store.record("add", "b", None, "test").unwrap();
        assert_eq!(store.count().unwrap(), 2);
        let deleted = store.clear().unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn test_record_default_metadata() {
        let store = make_store();
        let entry = store.record("sync", "target-1", None, "cli::sync").unwrap();
        assert_eq!(
            entry.metadata,
            serde_json::Value::Object(Default::default())
        );
        assert_eq!(entry.source_file, "cli::sync");
    }

    #[test]
    fn test_empty_store_returns_empty() {
        let store = make_store();
        assert_eq!(store.list_recent(10).unwrap().len(), 0);
        assert_eq!(
            store.filter(None, None, None, 10).unwrap().len(),
            0
        );
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn test_filter_operation_case_sensitive() {
        let store = make_store();
        store.record("Add", "a", None, "test").unwrap();
        store.record("add", "b", None, "test").unwrap();
        let results = store.filter(Some("add"), None, None, 100).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].target, "b");
    }
}
