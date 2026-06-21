//! Search strategy abstraction — multiple backends for searching palace drawers.
//!
//! Pattern borrowed from hermes-agent's `MemoryProvider` ABC but simpler:
//! we only need search, not full memory middleware. User picks a strategy
//! at `mpr init` time and can override per-call via `--strategy`.
//!
//! Available strategies:
//! - [`ContainsStrategy`]: Substring match, 0MB, fast, default
//! - [`NaiveJaccardStrategy`]: Jaccard token overlap, 0MB, slow for large palaces
//! - [`Bm25Strategy`]: BM25 rerank on top of naive, 0MB, fast
//! - [`EmbeddingStrategy`]: ONNX MiniLM + HNSW, 90MB+, semantic, slow

pub mod bm25;
pub mod contains;
pub mod embedding;
pub mod naive;
pub mod traits;

pub use traits::{SearchHit, SearchStrategy};

use crate::palace_db::PalaceDb;
use anyhow::Result;
use std::path::Path;

/// Strategy identifier. Stored in `~/.mempalace/config.json` under
/// `search.strategy`. Default = "contains".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyName {
    Contains,
    Naive,
    Bm25,
    Embedding,
}

impl StrategyName {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::Naive => "naive",
            Self::Bm25 => "bm25",
            Self::Embedding => "embedding",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "contains" => Some(Self::Contains),
            "naive" => Some(Self::Naive),
            "bm25" => Some(Self::Bm25),
            "embedding" => Some(Self::Embedding),
            _ => None,
        }
    }
}

impl Default for StrategyName {
    fn default() -> Self {
        Self::Contains
    }
}

/// Build a strategy instance by name. Falls back to Contains if name unknown.
pub fn build_strategy(name: StrategyName) -> Box<dyn SearchStrategy> {
    match name {
        StrategyName::Contains => Box::new(contains::ContainsStrategy::new()),
        StrategyName::Naive => Box::new(naive::NaiveJaccardStrategy::new()),
        StrategyName::Bm25 => Box::new(bm25::Bm25Strategy::new()),
        StrategyName::Embedding => Box::new(embedding::EmbeddingStrategy::new()),
    }
}

/// Convenience: run a search using the named strategy against the given
/// PalaceDb. Opens the DB if path is provided.
pub fn run_search(
    name: StrategyName,
    query: &str,
    db: &PalaceDb,
    n: usize,
) -> Result<Vec<SearchHit>> {
    let strategy = build_strategy(name);
    strategy.search(query, db, n)
}

/// Detect FTS5 availability on the given SQLite path. Used at startup
/// to gracefully fall back if the bundled SQLite was built without FTS5.
pub fn fts5_available(db_path: &Path) -> bool {
    use rusqlite::Connection;
    match Connection::open(db_path) {
        Ok(conn) => conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='drawers_fts' LIMIT 1",
                [],
                |_| Ok(1),
            )
            .is_ok(),
        Err(_) => false,
    }
}
