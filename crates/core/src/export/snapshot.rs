use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub id: String,
    pub commit_hash: String,
    pub created_at: String,
    pub message: String,
    pub stats: SnapshotStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotStats {
    pub sessions: usize,
    pub observations: usize,
    pub memories: usize,
    pub graph_nodes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub commit_hash: String,
    pub created_at: String,
    pub message: String,
}

pub struct SnapshotStore {
    snapshot_dir: PathBuf,
}

impl SnapshotStore {
    pub fn new<P: AsRef<Path>>(snapshot_dir: P) -> Result<Self> {
        let dir = snapshot_dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        Ok(Self { snapshot_dir: dir })
    }

    pub fn snapshot_dir(&self) -> &Path {
        &self.snapshot_dir
    }

    pub fn save_state(&self, state_json: &str, message: &str) -> Result<SnapshotMeta> {
        let state_path = self.snapshot_dir.join("state.json");
        fs::write(&state_path, state_json)?;

        let meta = SnapshotMeta {
            id: format!("snap-{}", uuid::Uuid::new_v4()),
            commit_hash: "local".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            message: message.to_string(),
            stats: SnapshotStats {
                sessions: 0,
                observations: 0,
                memories: 0,
                graph_nodes: 0,
            },
        };

        let meta_path = self.snapshot_dir.join("meta.json");
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?)?;

        Ok(meta)
    }

    pub fn load_state(&self) -> Result<String> {
        let state_path = self.snapshot_dir.join("state.json");
        let content = fs::read_to_string(&state_path)?;
        Ok(content)
    }

    pub fn list_snapshots(&self) -> Result<Vec<SnapshotEntry>> {
        let snapshots_dir = self.snapshot_dir.join("snapshots");
        if !snapshots_dir.exists() {
            return Ok(vec![]);
        }

        let mut entries = Vec::new();
        for entry in fs::read_dir(&snapshots_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let content = fs::read_to_string(&path)?;
                if let Ok(meta) = serde_json::from_str::<SnapshotMeta>(&content) {
                    entries.push(SnapshotEntry {
                        commit_hash: meta.commit_hash,
                        created_at: meta.created_at,
                        message: meta.message,
                    });
                }
            }
        }

        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        entries.truncate(20);
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> SnapshotStore {
        let dir = std::env::temp_dir().join(format!("snapshot_test_{}", uuid::Uuid::new_v4()));
        SnapshotStore::new(&dir).unwrap()
    }

    #[test]
    fn test_save_and_load_state() {
        let store = test_store();
        let state = r#"{"sessions":[],"memories":[]}"#;
        let meta = store.save_state(state, "test snapshot").unwrap();
        assert_eq!(meta.message, "test snapshot");

        let loaded = store.load_state().unwrap();
        assert_eq!(loaded, state);
    }

    #[test]
    fn test_list_snapshots_empty() {
        let store = test_store();
        let snapshots = store.list_snapshots().unwrap();
        assert!(snapshots.is_empty());
    }

    #[test]
    fn test_snapshot_dir_exists() {
        let store = test_store();
        assert!(store.snapshot_dir().exists());
    }

    #[test]
    fn test_snapshot_meta_serialization() {
        let meta = SnapshotMeta {
            id: "snap-1".to_string(),
            commit_hash: "abc123".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            message: "test".to_string(),
            stats: SnapshotStats {
                sessions: 5,
                observations: 100,
                memories: 20,
                graph_nodes: 50,
            },
        };
        let json = serde_json::to_string(&meta).unwrap();
        let loaded: SnapshotMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.id, "snap-1");
        assert_eq!(loaded.stats.sessions, 5);
    }

    #[test]
    fn test_snapshot_entry_from_meta() {
        let meta = SnapshotMeta {
            id: "snap-1".to_string(),
            commit_hash: "abc123".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            message: "test".to_string(),
            stats: SnapshotStats {
                sessions: 0,
                observations: 0,
                memories: 0,
                graph_nodes: 0,
            },
        };
        let entry = SnapshotEntry {
            commit_hash: meta.commit_hash.clone(),
            created_at: meta.created_at.clone(),
            message: meta.message.clone(),
        };
        assert_eq!(entry.commit_hash, "abc123");
    }

    #[test]
    fn test_save_state_creates_files() {
        let store = test_store();
        store.save_state("{}", "test").unwrap();
        assert!(store.snapshot_dir().join("state.json").exists());
        assert!(store.snapshot_dir().join("meta.json").exists());
    }

    #[test]
    fn test_load_state_missing_file() {
        let store = test_store();
        assert!(store.load_state().is_err());
    }
}
