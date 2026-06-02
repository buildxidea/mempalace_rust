use crate::types::MemorySlot;
use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::Path;

pub struct SlotStore {
    conn: Connection,
}

impl SlotStore {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        let store = Self { conn };
        store.init_db()?;
        Ok(store)
    }

    fn init_db(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS slots (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                content TEXT NOT NULL DEFAULT '',
                token_count INTEGER NOT NULL DEFAULT 0,
                priority INTEGER NOT NULL DEFAULT 2,
                last_updated TEXT NOT NULL,
                size_limit INTEGER,
                description TEXT,
                pinned INTEGER NOT NULL DEFAULT 0,
                read_only INTEGER NOT NULL DEFAULT 0,
                scope TEXT NOT NULL DEFAULT 'project'
            );
            CREATE INDEX IF NOT EXISTS idx_slots_priority ON slots(priority);
            CREATE INDEX IF NOT EXISTS idx_slots_pinned ON slots(pinned);
            ",
        )?;
        Ok(())
    }

    pub fn create_slot(&self, slot: &MemorySlot) -> Result<()> {
        self.conn.execute(
            "INSERT INTO slots (id, name, content, token_count, priority, last_updated, size_limit, description, pinned, read_only, scope) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                slot.id, slot.name, slot.content, slot.token_count, slot.priority,
                slot.last_updated.to_rfc3339(),
                Option::<i64>::None, Option::<String>::None,
                0, 0, "project"
            ],
        )?;
        Ok(())
    }

    pub fn get_slot(&self, id: &str) -> Result<Option<MemorySlot>> {
        let mut stmt = self.conn.prepare("SELECT * FROM slots WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(self.row_to_slot(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_slot_by_name(&self, name: &str) -> Result<Option<MemorySlot>> {
        let mut stmt = self.conn.prepare("SELECT * FROM slots WHERE name = ?1")?;
        let mut rows = stmt.query(params![name])?;
        if let Some(row) = rows.next()? {
            Ok(Some(self.row_to_slot(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn update_slot(&self, id: &str, content: &str) -> Result<()> {
        let now = Utc::now();
        let token_count = content.len() / 3;
        self.conn.execute(
            "UPDATE slots SET content = ?1, token_count = ?2, last_updated = ?3 WHERE id = ?4",
            params![content, token_count, now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn delete_slot(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM slots WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_slots(&self) -> Result<Vec<MemorySlot>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM slots ORDER BY priority ASC, last_updated DESC")?;
        let rows = stmt.query_map([], |row| self.row_to_slot(row))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_pinned_slots(&self) -> Result<Vec<MemorySlot>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM slots WHERE pinned = 1 ORDER BY priority ASC")?;
        let rows = stmt.query_map([], |row| self.row_to_slot(row))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

impl SlotStore {
    fn row_to_slot(&self, row: &rusqlite::Row) -> rusqlite::Result<MemorySlot> {
        Ok(MemorySlot {
            id: row.get("id")?,
            name: row.get("name")?,
            content: row.get("content")?,
            token_count: row.get("token_count")?,
            priority: row.get("priority")?,
            last_updated: chrono::DateTime::parse_from_rfc3339(
                &row.get::<_, String>("last_updated")?,
            )
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> SlotStore {
        SlotStore::open(Path::new(":memory:")).unwrap()
    }

    fn test_slot(id: &str, name: &str) -> MemorySlot {
        MemorySlot {
            id: id.to_string(),
            name: name.to_string(),
            content: "Test content".to_string(),
            token_count: 4,
            priority: 2,
            last_updated: Utc::now(),
        }
    }

    #[test]
    fn test_create_and_get_slot() {
        let store = test_store();
        let slot = test_slot("s-1", "test-slot");
        store.create_slot(&slot).unwrap();
        let retrieved = store.get_slot("s-1").unwrap().unwrap();
        assert_eq!(retrieved.name, "test-slot");
    }

    #[test]
    fn test_get_slot_by_name() {
        let store = test_store();
        store.create_slot(&test_slot("s-1", "my-slot")).unwrap();
        let retrieved = store.get_slot_by_name("my-slot").unwrap().unwrap();
        assert_eq!(retrieved.id, "s-1");
    }

    #[test]
    fn test_update_slot() {
        let store = test_store();
        store.create_slot(&test_slot("s-1", "test-slot")).unwrap();
        store.update_slot("s-1", "Updated content").unwrap();
        let slot = store.get_slot("s-1").unwrap().unwrap();
        assert_eq!(slot.content, "Updated content");
    }

    #[test]
    fn test_delete_slot() {
        let store = test_store();
        store.create_slot(&test_slot("s-1", "test-slot")).unwrap();
        store.delete_slot("s-1").unwrap();
        assert!(store.get_slot("s-1").unwrap().is_none());
    }

    #[test]
    fn test_list_slots() {
        let store = test_store();
        store.create_slot(&test_slot("s-1", "slot-1")).unwrap();
        store.create_slot(&test_slot("s-2", "slot-2")).unwrap();
        let slots = store.list_slots().unwrap();
        assert_eq!(slots.len(), 2);
    }

    #[test]
    fn test_get_pinned_slots() {
        let store = test_store();
        store.create_slot(&test_slot("s-1", "pinned-slot")).unwrap();
        store
            .conn
            .execute("UPDATE slots SET pinned = 1 WHERE id = 's-1'", [])
            .unwrap();
        let pinned = store.get_pinned_slots().unwrap();
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].name, "pinned-slot");
    }
}
