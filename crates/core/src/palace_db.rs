use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use crate::dedup_window::{DedupVerdict, WindowedDedup};
use crate::onnx_embed::OnnxModel;

pub const DEFAULT_COLLECTION_NAME: &str = "mempalace_drawers";
pub const DEFAULT_COMPRESSED_COLLECTION_NAME: &str = "mempalace_compressed";

/// Process-global online dedup window (mp-032 / report 06 §3.5).
///
/// Initialised lazily on first use with [`WindowedDedup::default`]
/// (5-minute window, 4096-entry LRU). The same instance is shared across
/// every `PalaceDb` opened in this process so multiple short-lived
/// `PalaceDb::open` calls (e.g. the MCP `tool_add_drawer` flow that
/// re-opens per request) still benefit from the rolling window.
///
/// Returns an `Arc` so callers can either
/// (a) use the returned handle directly, or
/// (b) hand it to [`PalaceDb::add_drawer_with_dedup`] for tests that
/// want isolated state.
pub fn dedup_window() -> Arc<WindowedDedup> {
    static GLOBAL: OnceLock<Arc<WindowedDedup>> = OnceLock::new();
    GLOBAL
        .get_or_init(|| Arc::new(WindowedDedup::default()))
        .clone()
}

/// Distinct lifecycle states of a palace on disk (#1498).
///
/// The upstream Python port (`fix(palace): stratify state messages for
/// empty/missing palace`, milla-jovovich/mempalace#1498) split the single
/// "No palace found / run init" message into three actionable states so the
/// CLI no longer tells a user to re-run `init` after `init` has already
/// succeeded but `mine` has not. The Rust port carries the same distinction
/// here, applied by `cmd_status`, `cmd_compress`, and the search error path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PalaceState {
    /// Palace directory does not exist (or is not a directory).
    Missing,
    /// Palace directory exists but the collection JSON has not been written
    /// yet — `init` ran, `mine` did not.
    NotInitialized,
    /// Collection JSON exists but contains no documents.
    Empty,
    /// Palace is initialized and contains at least one drawer.
    Ready,
}

/// Classify the current state of `palace_path` (#1498).
///
/// Cheap, side-effect-free: only touches the filesystem (existence checks +
/// one JSON read when present). Safe to call from any CLI command before
/// opening a `PalaceDb` to choose an actionable user-facing message.
pub fn classify_palace(palace_path: &std::path::Path) -> PalaceState {
    classify_palace_for_collection(palace_path, DEFAULT_COLLECTION_NAME)
}

/// Collection-aware variant of [`classify_palace`] (#1498).
pub fn classify_palace_for_collection(
    palace_path: &std::path::Path,
    collection_name: &str,
) -> PalaceState {
    if !palace_path.is_dir() {
        return PalaceState::Missing;
    }
    let docs_path = palace_path.join(format!("{}.json", collection_name));
    if !docs_path.is_file() {
        return PalaceState::NotInitialized;
    }
    // Mirror PalaceDb::open: missing/unparseable JSON degrades to empty.
    let content = match std::fs::read_to_string(&docs_path) {
        Ok(c) => c,
        Err(_) => return PalaceState::NotInitialized,
    };
    let docs: HashMap<String, DocumentEntry> = serde_json::from_str(&content).unwrap_or_default();
    if docs.is_empty() {
        PalaceState::Empty
    } else {
        PalaceState::Ready
    }
}

/// Print the actionable next-step hint for a non-`Ready` palace state (#1498).
///
/// Returns `true` when a message was printed (state was not `Ready`) so the
/// caller can decide whether to short-circuit. The leading newline matches the
/// pre-existing print style used elsewhere in the CLI.
pub fn print_palace_state_hint(state: PalaceState, palace_path: &std::path::Path) -> bool {
    match state {
        PalaceState::Missing => {
            println!("\n  No palace found at {}", palace_path.display());
            println!("  Run: mpr init <dir>");
            true
        }
        PalaceState::NotInitialized => {
            println!(
                "\n  Palace directory exists at {} but no data has been mined yet.",
                palace_path.display()
            );
            println!("  Run: mpr mine <dir>");
            true
        }
        PalaceState::Empty => {
            println!(
                "\n  Palace at {} has no drawers yet.",
                palace_path.display()
            );
            println!("  Run: mpr mine <dir> to ingest content.");
            true
        }
        PalaceState::Ready => false,
    }
}

pub struct PalaceDb {
    documents: HashMap<String, DocumentEntry>,
    palace_path: PathBuf,
    collection_name: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct DocumentEntry {
    content: String,
    metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct QueryResult {
    pub ids: Vec<String>,
    pub documents: Vec<String>,
    pub distances: Vec<f64>,
    pub metadatas: Vec<HashMap<String, serde_json::Value>>,
}

pub struct EmbeddingDb {
    embedder: Arc<OnnxModel>,
    hnsw: embedvec::HnswIndex,
    #[allow(dead_code)]
    documents: Vec<(String, String)>,
    storage: embedvec::VectorStorage,
}

impl PalaceDb {
    pub fn open(palace_path: &std::path::Path) -> anyhow::Result<Self> {
        Self::open_collection(palace_path, DEFAULT_COLLECTION_NAME)
    }

    pub fn open_collection(
        palace_path: &std::path::Path,
        collection_name: &str,
    ) -> anyhow::Result<Self> {
        let collection_name = collection_name.to_string();
        let docs_path = palace_path.join(format!("{}.json", collection_name));

        let documents = if docs_path.exists() {
            let content = std::fs::read_to_string(&docs_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            HashMap::new()
        };

        Ok(Self {
            documents,
            palace_path: palace_path.to_path_buf(),
            collection_name,
        })
    }

    pub async fn query(
        &self,
        query_text: &str,
        wing: Option<&str>,
        room: Option<&str>,
        n_results: usize,
    ) -> anyhow::Result<Vec<QueryResult>> {
        self.query_sync(query_text, wing, room, n_results)
    }

    pub fn query_sync(
        &self,
        query_text: &str,
        wing: Option<&str>,
        room: Option<&str>,
        n_results: usize,
    ) -> anyhow::Result<Vec<QueryResult>> {
        self.query_sync_with_filter(query_text, wing, room, n_results, None)
    }

    pub fn query_sync_with_filter(
        &self,
        query_text: &str,
        wing: Option<&str>,
        room: Option<&str>,
        n_results: usize,
        metadata_filter: Option<&std::collections::HashMap<String, String>>,
    ) -> anyhow::Result<Vec<QueryResult>> {
        let query_lower = query_text.to_lowercase();

        let mut results: Vec<(String, f64, &DocumentEntry)> = self
            .documents
            .iter()
            .filter_map(|(id, entry)| {
                if let Some(w) = wing {
                    let entry_wing = entry
                        .metadata
                        .get("wing")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if entry_wing != w {
                        return None;
                    }
                }
                if let Some(r) = room {
                    let entry_room = entry
                        .metadata
                        .get("room")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if entry_room != r {
                        return None;
                    }
                }

                // Apply custom metadata filter if provided
                if let Some(filter) = metadata_filter {
                    for (key, expected_value) in filter {
                        let entry_value = entry
                            .metadata
                            .get(key)
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if entry_value != *expected_value {
                            return None;
                        }
                    }
                }

                let similarity = naive_similarity(&query_lower, &entry.content.to_lowercase());
                if similarity > 0.05 {
                    Some((id.clone(), similarity, entry))
                } else {
                    None
                }
            })
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(n_results);

        let query_results: Vec<QueryResult> = results
            .into_iter()
            .map(|(id, similarity, entry)| {
                let mut metadata = entry.metadata.clone();
                metadata.insert("distance".to_string(), serde_json::json!(1.0 - similarity));

                QueryResult {
                    ids: vec![id],
                    documents: vec![entry.content.clone()],
                    distances: vec![1.0 - similarity],
                    metadatas: vec![metadata],
                }
            })
            .collect();

        Ok(query_results)
    }

    pub fn add(
        &mut self,
        documents: &[(&str, &str)],
        metadata: &[&[(&str, &str)]],
    ) -> anyhow::Result<()> {
        for ((id, content), meta) in documents.iter().zip(metadata.iter()) {
            let meta_map: HashMap<String, serde_json::Value> = meta
                .iter()
                .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
                .collect();

            // Privacy filter (mp-031, ADR-12) — strip API keys, JWTs,
            // private keys, and other well-known secret patterns from the
            // verbatim drawer body before it lands on disk. Wing/room slugs
            // are structural and never run through the redactor; only the
            // user-text `content` field is processed.
            let redacted = crate::privacy::redact(content).redacted_text;

            self.documents.insert(
                id.to_string(),
                DocumentEntry {
                    content: redacted,
                    metadata: meta_map,
                },
            );
        }

        // Don't auto-save on every add - caller should call flush() when done batching
        Ok(())
    }

    pub fn upsert_documents(
        &mut self,
        documents: &[(String, String, HashMap<String, serde_json::Value>)],
    ) -> anyhow::Result<()> {
        for (id, content, metadata) in documents {
            // Privacy filter (mp-031, ADR-12) — same as `add()`.
            let redacted = crate::privacy::redact(content).redacted_text;

            self.documents.insert(
                id.clone(),
                DocumentEntry {
                    content: redacted,
                    metadata: metadata.clone(),
                },
            );
        }

        Ok(())
    }

    pub fn delete_id(&mut self, id: &str) -> anyhow::Result<bool> {
        let removed = self.documents.remove(id).is_some();
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn file_already_mined(&self, source_file: &str, check_mtime: bool) -> bool {
        self.file_already_mined_with_mode(source_file, check_mtime, None)
    }

    /// Extract-mode-aware variant of [`Self::file_already_mined`] (#1505).
    ///
    /// When `extract_mode` is `Some`, only drawers whose stored
    /// `extract_mode` metadata matches the argument are considered. Legacy
    /// drawers (no `extract_mode` field) are treated as `exchange`-mode so
    /// pre-#1505 palaces still classify correctly.
    pub fn file_already_mined_with_mode(
        &self,
        source_file: &str,
        check_mtime: bool,
        extract_mode: Option<&str>,
    ) -> bool {
        let Some(entry) = self.documents.values().find(|entry| {
            let same_source =
                entry.metadata.get("source_file").and_then(|v| v.as_str()) == Some(source_file);
            if !same_source {
                return false;
            }
            match extract_mode {
                None => true,
                Some(want) => {
                    let stored = entry.metadata.get("extract_mode").and_then(|v| v.as_str());
                    match stored {
                        Some(value) => value == want,
                        // Legacy: unfielded rows are treated as exchange.
                        None => want == "exchange",
                    }
                }
            }
        }) else {
            return false;
        };

        // Pre-v2 drawers have no version field — treat them as stale.
        // Returns false so the file gets re-mined with the new schema.
        let stored_version = entry
            .metadata
            .get("normalize_version")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(1);
        if stored_version < crate::constants::NORMALIZE_VERSION as i64 {
            return false;
        }

        if !check_mtime {
            return true;
        }

        let Some(stored_mtime) = entry
            .metadata
            .get("source_mtime")
            .and_then(|v| v.as_str())
            .and_then(|v| v.parse::<f64>().ok())
        else {
            return false;
        };

        let Ok(metadata) = std::fs::metadata(source_file) else {
            return false;
        };
        let Ok(modified) = metadata.modified() else {
            return false;
        };
        let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) else {
            return false;
        };

        (duration.as_secs_f64() - stored_mtime).abs() < f64::EPSILON
    }

    pub fn flush(&mut self) -> anyhow::Result<()> {
        self.save()
    }

    pub fn complete_test_setup(&mut self) -> anyhow::Result<()> {
        self.flush()
    }

    fn save(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.palace_path)?;

        let docs_path = self
            .palace_path
            .join(format!("{}.json", self.collection_name));
        let content = serde_json::to_string_pretty(&self.documents)?;
        std::fs::write(docs_path, content)?;

        Ok(())
    }

    pub(crate) fn _get_document(&self, id: &str) -> Option<&DocumentEntry> {
        self.documents.get(id)
    }

    /// Get metadata for a document by ID.
    pub fn get_document_metadata(&self, id: &str) -> Option<&HashMap<String, serde_json::Value>> {
        self.documents.get(id).map(|e| &e.metadata)
    }

    /// Get documents by their IDs. Returns only the IDs that exist.
    pub fn get_documents(&self, ids: &[String]) -> Vec<String> {
        ids.iter()
            .filter(|id| self.documents.contains_key(id.as_str()))
            .cloned()
            .collect()
    }

    /// Get all documents that have a matching session_id in their metadata.
    /// Returns vector of (id, content, metadata) tuples.
    pub fn get_documents_by_session(
        &self,
        session_id: &str,
    ) -> Vec<(String, String, HashMap<String, serde_json::Value>)> {
        self.documents
            .iter()
            .filter(|(_, entry)| {
                entry.metadata.get("session_id").and_then(|v| v.as_str()) == Some(session_id)
            })
            .map(|(id, entry)| (id.clone(), entry.content.clone(), entry.metadata.clone()))
            .collect()
    }

    pub fn count(&self) -> usize {
        self.documents.len()
    }

    /// Get all documents, optionally filtered by wing and/or room.
    /// Returns results sorted by importance (from metadata or distance).
    pub fn get_all(
        &self,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Vec<QueryResult> {
        let mut entries: Vec<(&String, &DocumentEntry)> = self
            .documents
            .iter()
            .filter(|(_, entry)| {
                if let Some(w) = wing {
                    let entry_wing = entry
                        .metadata
                        .get("wing")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if entry_wing != w {
                        return false;
                    }
                }
                if let Some(r) = room {
                    let entry_room = entry
                        .metadata
                        .get("room")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if entry_room != r {
                        return false;
                    }
                }
                true
            })
            .collect();

        // Sort by importance metadata if available, otherwise by order added
        entries.sort_by(|(id_a, entry_a), (id_b, entry_b)| {
            let imp_a = entry_a
                .metadata
                .get("importance")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.5);
            let imp_b = entry_b
                .metadata
                .get("importance")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.5);
            imp_b
                .partial_cmp(&imp_a)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| id_a.cmp(id_b))
        });

        entries.truncate(limit);

        let query_results: Vec<QueryResult> = entries
            .into_iter()
            .map(|(id, entry)| QueryResult {
                ids: vec![id.clone()],
                documents: vec![entry.content.clone()],
                distances: vec![0.0],
                metadatas: vec![entry.metadata.clone()],
            })
            .collect();

        query_results
    }

    /// Add a single drawer with the process-global online dedup window
    /// (mp-032). If the trimmed `content` was seen within the rolling
    /// window, the insert is skipped and `Ok(None)` is returned; otherwise
    /// the drawer is inserted via [`PalaceDb::add`] and `Ok(Some(id))` is
    /// returned. Caller is responsible for `flush()` when batching.
    ///
    /// Use [`PalaceDb::add_drawer_with_dedup`] in tests that need an
    /// isolated dedup state, since the global window persists across
    /// concurrent test cases otherwise.
    pub fn add_drawer(
        &mut self,
        id: &str,
        content: &str,
        metadata: &[(&str, &str)],
    ) -> anyhow::Result<Option<String>> {
        self.add_drawer_with_dedup(&dedup_window(), id, content, metadata)
    }

    /// Variant of [`PalaceDb::add_drawer`] that uses an explicit dedup
    /// instance. Lets call sites (and unit tests) inject scoped state
    /// instead of relying on the process-global window.
    pub fn add_drawer_with_dedup(
        &mut self,
        dedup: &WindowedDedup,
        id: &str,
        content: &str,
        metadata: &[(&str, &str)],
    ) -> anyhow::Result<Option<String>> {
        match dedup.check_and_record(content) {
            DedupVerdict::Duplicate => {
                let hash = crate::dedup_window::hash_normalized(content);
                tracing::debug!(
                    target: "mempalace::dedup",
                    drawer_id = %id,
                    sha256 = %hex::encode(hash),
                    "dedup skipped"
                );
                Ok(None)
            }
            DedupVerdict::Fresh => {
                self.add(&[(id, content)], &[metadata])?;
                Ok(Some(id.to_string()))
            }
        }
    }
}

impl EmbeddingDb {
    pub fn new(dimension: usize) -> anyhow::Result<Self> {
        let embedder = OnnxModel::load()?;
        Self::with_embedder(Arc::new(embedder), dimension)
    }

    pub fn with_embedder(embedder: Arc<OnnxModel>, dimension: usize) -> anyhow::Result<Self> {
        let hnsw = embedvec::HnswIndex::new(16, 200, embedvec::Distance::Cosine);
        let storage = embedvec::VectorStorage::new(dimension, embedvec::Quantization::None);
        Ok(Self {
            embedder,
            hnsw,
            documents: Vec::new(),
            storage,
        })
    }

    pub fn add(&mut self, id: &str, text: &str) -> anyhow::Result<usize> {
        let embedding = self.embed(text)?;
        let idx = self.documents.len();
        self.documents.push((id.to_string(), text.to_string()));
        self.storage.add(&embedding, None)?;
        self.hnsw.insert(idx, &embedding, &self.storage, None)?;
        Ok(idx)
    }

    pub fn add_batch(&mut self, items: &[(String, String)]) -> anyhow::Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let texts: Vec<&str> = items.iter().map(|(_, t)| t.as_str()).collect();
        let embeddings = self.embedder.encode_batch(&texts, true)?;
        let start_idx = self.documents.len();
        for (i, (id, text)) in items.iter().enumerate() {
            self.documents.push((id.clone(), text.clone()));
            // Normalize ONNX embeddings before storing (ONNX model returns unnormalized)
            let normalized = normalize_embedding(&embeddings[i]);
            self.storage.add(&normalized, None)?;
            self.hnsw
                .insert(start_idx + i, &normalized, &self.storage, None)?;
        }
        Ok(())
    }

    pub fn query(&self, query_text: &str, n_results: usize) -> anyhow::Result<Vec<(f32, usize)>> {
        let query_embedding = self.embed(query_text)?;
        let normalized_query = normalize_embedding(&query_embedding);
        let results = self
            .hnsw
            .search(&normalized_query, n_results, 1024, &self.storage, None)?;
        Ok(results.into_iter().map(|(id, dist)| (dist, id)).collect())
    }

    pub fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let embedding = self.embedder.encode(text)?;
        Ok(embedding)
    }
}

fn normalize_embedding(embedding: &[f32]) -> Vec<f32> {
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm == 0.0 {
        return embedding.to_vec();
    }
    embedding.iter().map(|x| x / norm).collect()
}

fn naive_similarity(query: &str, content: &str) -> f64 {
    let query_words: std::collections::HashSet<_> = query.split_whitespace().collect();
    let content_words: std::collections::HashSet<_> = content.split_whitespace().collect();

    if query_words.is_empty() || content_words.is_empty() {
        return 0.0;
    }

    let intersection = query_words.intersection(&content_words).count();
    let union = query_words.union(&content_words).count();

    intersection as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_naive_similarity() {
        let sim = naive_similarity("hello world", "hello world");
        assert!((sim - 1.0).abs() < 1e-6);

        let sim = naive_similarity("hello world", "hello");
        assert!(sim > 0.3 && sim < 0.7);

        let sim = naive_similarity("hello world", "completely different");
        assert!(sim < 0.1);
    }

    /// #1498 regression: a non-existent path classifies as `Missing`.
    #[test]
    fn test_classify_palace_missing_dir() {
        let temp = tempfile::tempdir().unwrap();
        let palace = temp.path().join("nope");
        assert_eq!(classify_palace(&palace), PalaceState::Missing);
    }

    /// #1498 regression: directory exists but no collection JSON — the user
    /// has run `init` but not `mine`. Hint must be `mpr mine`, not `mpr init`.
    #[test]
    fn test_classify_palace_not_initialized_when_dir_only() {
        let temp = tempfile::tempdir().unwrap();
        let palace = temp.path().join("palace");
        std::fs::create_dir_all(&palace).unwrap();
        assert_eq!(classify_palace(&palace), PalaceState::NotInitialized);
    }

    /// #1498 regression: collection JSON exists but parses to zero documents.
    #[test]
    fn test_classify_palace_empty_when_no_documents() {
        let temp = tempfile::tempdir().unwrap();
        let palace = temp.path().join("palace");
        std::fs::create_dir_all(&palace).unwrap();
        let mut db = PalaceDb::open(&palace).unwrap();
        db.flush().unwrap();
        assert_eq!(classify_palace(&palace), PalaceState::Empty);
    }

    /// #1498 regression: a palace with at least one drawer classifies as
    /// `Ready` so the caller skips the actionable hint and prints stats.
    #[test]
    fn test_classify_palace_ready_when_documents_exist() {
        let temp = tempfile::tempdir().unwrap();
        let palace = temp.path().join("palace");
        std::fs::create_dir_all(&palace).unwrap();
        let mut db = PalaceDb::open(&palace).unwrap();
        db.add(
            &[("d1", "verbatim chunk")],
            &[&[("wing", "people"), ("room", "today")]],
        )
        .unwrap();
        db.flush().unwrap();
        assert_eq!(classify_palace(&palace), PalaceState::Ready);
    }

    /// mp-031 regression: secrets in drawer bodies must be redacted **before**
    /// they're persisted. We round-trip a chunk that contains a fake OpenAI
    /// key through `add()` + `flush()` + a fresh `open()` and assert the raw
    /// key is not present on disk.
    #[test]
    fn test_add_redacts_openai_key_before_storage() {
        let temp = tempfile::tempdir().unwrap();
        let palace = temp.path().join("palace");
        std::fs::create_dir_all(&palace).unwrap();

        let raw_key = "sk-abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ1234";
        let body = format!("the leaked key was {} please rotate it", raw_key);

        {
            let mut db = PalaceDb::open(&palace).unwrap();
            db.add(
                &[("d-secret", body.as_str())],
                &[&[("wing", "ops"), ("room", "incident")]],
            )
            .unwrap();
            db.flush().unwrap();
        }

        // Re-open from disk and inspect the persisted document.
        let db = PalaceDb::open(&palace).unwrap();
        let stored = db._get_document("d-secret").expect("drawer present");
        assert!(
            stored.content.contains("<REDACTED:OPENAI_KEY>"),
            "redaction placeholder missing: {}",
            stored.content
        );
        assert!(
            !stored.content.contains(raw_key),
            "raw key leaked into storage: {}",
            stored.content
        );
        // Surrounding prose is preserved.
        assert!(stored.content.contains("the leaked key was"));
        assert!(stored.content.contains("please rotate it"));
    }

    /// mp-031: `upsert_documents` is the other canonical add path
    /// (diary_ingest, sweeper, compress). It must redact too.
    #[test]
    fn test_upsert_documents_redacts_before_storage() {
        let temp = tempfile::tempdir().unwrap();
        let palace = temp.path().join("palace");
        std::fs::create_dir_all(&palace).unwrap();

        let raw_token = "ghp_abcdefghijklmnopqrstuvwxyz0123456789AB";
        let body = format!("token={}", raw_token);

        let mut db = PalaceDb::open(&palace).unwrap();
        let mut meta: HashMap<String, serde_json::Value> = HashMap::new();
        meta.insert("wing".into(), serde_json::json!("ops"));
        meta.insert("room".into(), serde_json::json!("creds"));
        db.upsert_documents(&[("d-gh".to_string(), body, meta)])
            .unwrap();
        db.flush().unwrap();

        let db = PalaceDb::open(&palace).unwrap();
        let stored = db._get_document("d-gh").expect("drawer present");
        assert!(stored.content.contains("<REDACTED:GITHUB_TOKEN>"));
        assert!(!stored.content.contains(raw_token));
    }

    /// mp-032 wiring: `PalaceDb::add_drawer_with_dedup` honours the
    /// rolling-window dedup. First call inserts; second call with the
    /// same content (within the window) skips and returns `None`. We
    /// inject a fresh `WindowedDedup` so this test does not entangle
    /// with the process-global window other tests share.
    #[test]
    fn test_add_drawer_with_dedup_skips_duplicate_within_window() {
        let temp = tempfile::tempdir().unwrap();
        let palace = temp.path().join("palace");
        std::fs::create_dir_all(&palace).unwrap();

        let mut db = PalaceDb::open(&palace).unwrap();
        let dedup =
            crate::dedup_window::WindowedDedup::new(std::time::Duration::from_secs(300), 64);
        let meta = [("wing", "ops"), ("room", "shipping")];

        let first = db
            .add_drawer_with_dedup(&dedup, "d-1", "release notes v0.1", &meta)
            .expect("first add_drawer call should succeed");
        assert_eq!(first, Some("d-1".to_string()));
        assert_eq!(db.count(), 1);

        let second = db
            .add_drawer_with_dedup(&dedup, "d-1-clone", "release notes v0.1", &meta)
            .expect("second add_drawer call should succeed");
        assert_eq!(second, None, "duplicate content should be skipped");
        assert_eq!(db.count(), 1, "no new drawer must be inserted on duplicate");
    }

    /// mp-032 wiring: whitespace differences are folded by the dedup
    /// hash so `"foo"` and `"  foo  "` collapse into one drawer.
    #[test]
    fn test_add_drawer_with_dedup_normalises_whitespace() {
        let temp = tempfile::tempdir().unwrap();
        let palace = temp.path().join("palace");
        std::fs::create_dir_all(&palace).unwrap();

        let mut db = PalaceDb::open(&palace).unwrap();
        let dedup =
            crate::dedup_window::WindowedDedup::new(std::time::Duration::from_secs(300), 64);
        let meta = [("wing", "ops"), ("room", "shipping")];

        assert_eq!(
            db.add_drawer_with_dedup(&dedup, "d-a", "foo", &meta)
                .unwrap(),
            Some("d-a".to_string())
        );
        assert_eq!(
            db.add_drawer_with_dedup(&dedup, "d-b", "  foo  ", &meta)
                .unwrap(),
            None,
            "trimmed whitespace should still hit the dedup window"
        );
        assert_eq!(db.count(), 1);
    }
}
