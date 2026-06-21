//! BM25 strategy — delegates to PalaceDb's lazy BM25 index.
//!
//! No rebuild from scratch on every call. PalaceDb maintains a
//! lazily-initialised `bm25::SearchEngine` (built once from all documents)
//! which `Bm25Strategy::search` queries via `PalaceDb::bm25_search`.

use super::traits::{SearchHit, SearchStrategy};
use crate::palace_db::PalaceDb;
use anyhow::Result;

#[allow(dead_code)]
pub struct Bm25Strategy {
    k1: f64,
    b: f64,
}

impl Default for Bm25Strategy {
    fn default() -> Self {
        Self::new()
    }
}

impl Bm25Strategy {
    pub fn new() -> Self {
        Self { k1: 1.5, b: 0.75 }
    }
}

impl SearchStrategy for Bm25Strategy {
    fn name(&self) -> &str {
        "bm25"
    }

    fn search(&self, query: &str, db: &PalaceDb, n: usize) -> Result<Vec<SearchHit>> {
        if query.trim().is_empty() {
            return Ok(vec![]);
        }
        // Delegate to PalaceDb's lazy BM25 index — built once, reused across calls.
        Ok(db.bm25_search(query, n))
    }
}
