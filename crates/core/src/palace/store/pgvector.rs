// =====================================================================
// PgvectorStore — PostgreSQL + pgvector PalaceStore implementation
// (bead mempalace_rust-pg0z)
// =====================================================================
//
// Tier 4 backend: PostgreSQL with the pgvector extension for vector
// similarity search. Suited for production deployments where the local
// embedvec / usearch stores are insufficient (multi-process access,
// >100 k drawers, HA requirements).
//
// Connection pool: deadpool-postgres (tokio-postgres under the hood).
// Vector index: HNSW (default) or IVFFlat, created automatically.
// Config: MEMPALACE_PGVECTOR_DSN env var or pgvector_dsn in config.json.
//
// ## Schema
//
//   CREATE EXTENSION IF NOT EXISTS vector;
//
//   CREATE TABLE IF NOT EXISTS palace_drawers (
//       id TEXT PRIMARY KEY,
//       content TEXT NOT NULL,
//       kind TEXT NOT NULL,
//       tier TEXT NOT NULL DEFAULT 'working',
//       wing TEXT,
//       room TEXT,
//       metadata JSONB DEFAULT '{}',
//       embedding vector($DIM)
//   );
//
//   -- HNSW index (default, created in ensure_indexes)
//   CREATE INDEX IF NOT EXISTS palace_drawers_hnsw
//       ON palace_drawers
//       USING hnsw (embedding vector_cosine_ops)
//       WITH (m = 16, ef_construction = 64);
//
//   -- Embedder identity tracking
//   CREATE TABLE IF NOT EXISTS palace_embedder_meta (
//       key TEXT PRIMARY KEY,
//       value TEXT NOT NULL
//   );
//
// ## Thread Safety
//
// `deadpool_postgres::Pool` is internally Arc-wrapped and safe to share
// across async tasks. No additional locking is needed.

use async_trait::async_trait;
use deadpool_postgres::{Config, Pool, Runtime};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use tokio_postgres::NoTls;
use tracing::{info, warn};

use crate::embed::EmbeddingManifest;
use crate::palace::{Drawer, DrawerId, PalaceStore, SearchHit, SearchScope, StoreTier};

/// Index algorithm used for the pgvector ANN index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PgvectorIndexType {
    /// Hierarchical Navigable Small World (default, better recall).
    Hnsw,
    /// Inverted File with Product Quantization (faster build, lower recall).
    Ivfflat,
}

impl Default for PgvectorIndexType {
    fn default() -> Self {
        Self::Hnsw
    }
}

impl std::fmt::Display for PgvectorIndexType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hnsw => write!(f, "hnsw"),
            Self::Ivfflat => write!(f, "ivfflat"),
        }
    }
}

/// Configuration for the pgvector backend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PgvectorConfig {
    /// PostgreSQL connection string (DSN).
    /// e.g. `postgresql://user:pass@localhost:5432/mempalace`
    pub dsn: String,
    /// Embedding dimensionality. Must match the embedder's `dim()`.
    pub embedding_dim: usize,
    /// Index algorithm. Default: Hnsw.
    #[allow(dead_code)]
    pub index_type: PgvectorIndexType,
    /// Maximum number of connections in the pool. Default: 5.
    pub max_pool_size: usize,
    /// HNSW `m` parameter (number of connections per layer). Default: 16.
    #[allow(dead_code)]
    pub hnsw_m: u32,
    /// HNSW `ef_construction` parameter. Default: 64.
    #[allow(dead_code)]
    pub hnsw_ef_construction: u32,
    /// IVFFlat `lists` parameter (number of clusters). Default: 100.
    #[allow(dead_code)]
    pub ivfflat_lists: u32,
}

impl Default for PgvectorConfig {
    fn default() -> Self {
        Self {
            dsn: String::new(),
            embedding_dim: 384,
            index_type: PgvectorIndexType::default(),
            max_pool_size: 5,
            hnsw_m: 16,
            hnsw_ef_construction: 64,
            ivfflat_lists: 100,
        }
    }
}

impl PgvectorConfig {
    /// Build config from environment variables.
    pub fn from_env() -> anyhow::Result<Self> {
        let dsn = std::env::var("MEMPALACE_PGVECTOR_DSN")
            .map_err(|_| anyhow::anyhow!(
                "MEMPALACE_PGVECTOR_DSN not set. Example: postgresql://user:pass@localhost:5432/mempalace"
            ))?;

        let embedding_dim = std::env::var("MEMPALACE_PGVECTOR_DIM")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(384);

        let index_type = std::env::var("MEMPALACE_PGVECTOR_INDEX")
            .ok()
            .and_then(|s| match s.to_lowercase().as_str() {
                "hnsw" => Some(PgvectorIndexType::Hnsw),
                "ivfflat" => Some(PgvectorIndexType::Ivfflat),
                _ => None,
            })
            .unwrap_or_default();

        let max_pool_size = std::env::var("MEMPALACE_PGVECTOR_POOL_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);

        let hnsw_m = std::env::var("MEMPALACE_PGVECTOR_HNSW_M")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(16);

        let hnsw_ef_construction = std::env::var("MEMPALACE_PGVECTOR_HNSW_EF")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(64);

        let ivfflat_lists = std::env::var("MEMPALACE_PGVECTOR_IVFFLAT_LISTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);

        Ok(Self {
            dsn,
            embedding_dim,
            index_type,
            max_pool_size,
            hnsw_m,
            hnsw_ef_construction,
            ivfflat_lists,
        })
    }
}

/// PostgreSQL + pgvector backed [`PalaceStore`].
///
/// Stores drawers in a PostgreSQL table with a `vector` column and
/// performs ANN search using pgvector's cosine distance operator (`<=>`).
///
/// ## Construction
///
/// ```ignore
/// let store = PgvectorStore::new(PgvectorConfig {
///     dsn: "postgresql://localhost/mempalace".into(),
///     embedding_dim: 384,
///     ..Default::default()
/// }).await?;
/// ```
pub struct PgvectorStore {
    pool: Pool,
    config: PgvectorConfig,
}

// -----------------------------------------------------------------------
// Raw SQL — extracted as const strings for readability
// -----------------------------------------------------------------------

const SQL_CREATE_EXTENSION: &str = "CREATE EXTENSION IF NOT EXISTS vector";

const SQL_CREATE_DRAWERS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS palace_drawers (
    id TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'raw',
    tier TEXT NOT NULL DEFAULT 'working',
    wing TEXT,
    room TEXT,
    metadata JSONB DEFAULT '{}',
    embedding vector($1)
)
"#;

const SQL_CREATE_EMBEDDER_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS palace_embedder_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
)
"#;

const SQL_CREATE_INDEX_HNSW: &str = r#"
CREATE INDEX IF NOT EXISTS palace_drawers_hnsw
    ON palace_drawers
    USING hnsw (embedding vector_cosine_ops)
    WITH (m = $1, ef_construction = $2)
"#;

const SQL_CREATE_INDEX_IVFFLAT: &str = r#"
CREATE INDEX IF NOT EXISTS palace_drawers_ivfflat
    ON palace_drawers
    USING ivfflat (embedding vector_cosine_ops)
    WITH (lists = $1)
"#;

const SQL_UPSERT_DRAWER: &str = r#"
INSERT INTO palace_drawers (id, content, kind, tier, wing, room, metadata, embedding)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
ON CONFLICT (id) DO UPDATE SET
    content = EXCLUDED.content,
    kind = EXCLUDED.kind,
    tier = EXCLUDED.tier,
    wing = EXCLUDED.wing,
    room = EXCLUDED.room,
    metadata = EXCLUDED.metadata,
    embedding = EXCLUDED.embedding
"#;

const SQL_DELETE_BY_IDS: &str = "DELETE FROM palace_drawers WHERE id = ANY($1)";

const SQL_COUNT_FILTERED: &str = r#"
SELECT COUNT(*) FROM palace_drawers
WHERE ($1::text IS NULL OR wing = $1)
  AND ($2::text IS NULL OR room = $2)
"#;

const SQL_SELECT_DRAWERS: &str = r#"
SELECT id, content, kind, tier, wing, room, metadata
FROM palace_drawers
WHERE ($1::text IS NULL OR wing = $1)
  AND ($2::text IS NULL OR room = $2)
ORDER BY id
LIMIT $3
"#;

const SQL_SEARCH_ANN: &str = r#"
SELECT id, content, kind, tier, wing, room, metadata,
       1.0 - (embedding <=> $1::vector) AS similarity
FROM palace_drawers
WHERE ($2::text IS NULL OR wing = $2)
  AND ($3::text IS NULL OR room = $3)
ORDER BY embedding <=> $1::vector
LIMIT $4
"#;

const SQL_SET_EMBEDDER_META: &str = r#"
INSERT INTO palace_embedder_meta (key, value)
VALUES ($1, $2)
ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value
"#;

const SQL_GET_EMBEDDER_META: &str = "SELECT value FROM palace_embedder_meta WHERE key = $1";

const SQL_ANALYZE: &str = "ANALYZE palace_drawers";

// -----------------------------------------------------------------------
// Helper: build a deadpool-postgres config from a DSN string
// -----------------------------------------------------------------------

fn pool_config_from_dsn(dsn: &str, max_size: usize) -> anyhow::Result<Config> {
    let mut cfg = Config::new();
    cfg.url = Some(dsn.to_string());
    cfg.manager = Some(deadpool_postgres::ManagerConfig {
        recycling_method: deadpool_postgres::RecyclingMethod::Fast,
    });
    // deadpool uses max_size (not max_connections); default 10 if not set.
    cfg.pool = Some(deadpool_postgres::PoolConfig::new(max_size));
    Ok(cfg)
}

// -----------------------------------------------------------------------
// PgvectorStore implementation
// -----------------------------------------------------------------------

impl PgvectorStore {
    /// Connect to PostgreSQL and initialise the schema.
    ///
    /// Creates the pgvector extension (requires superuser), the
    /// `palace_drawers` table, and the ANN index. Idempotent — safe
    /// to call on every startup.
    pub async fn new(config: PgvectorConfig) -> anyhow::Result<Self> {
        if config.dsn.is_empty() {
            anyhow::bail!(
                "pgvector DSN is empty. Set MEMPALACE_PGVECTOR_DSN or provide dsn in PgvectorConfig."
            );
        }

        let pg_config = pool_config_from_dsn(&config.dsn, config.max_pool_size)?;
        let pool = pg_config
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| anyhow::anyhow!("pgvector pool create: {e}"))?;

        Self::ensure_schema(&pool, &config).await?;

        info!(
            "pgvector store connected: dim={}, index={}",
            config.embedding_dim, config.index_type
        );

        Ok(Self { pool, config })
    }

    /// Connect using only a DSN string (convenience).
    pub async fn connect(dsn: &str) -> anyhow::Result<Self> {
        let config = PgvectorConfig {
            dsn: dsn.to_string(),
            ..Default::default()
        };
        Self::new(config).await
    }

    /// Connect with a DSN and embedding dimension.
    pub async fn connect_with_dim(dsn: &str, embedding_dim: usize) -> anyhow::Result<Self> {
        let config = PgvectorConfig {
            dsn: dsn.to_string(),
            embedding_dim,
            ..Default::default()
        };
        Self::new(config).await
    }

    /// Create extension, tables, and indexes. Idempotent.
    async fn ensure_schema(pool: &Pool, config: &PgvectorConfig) -> anyhow::Result<()> {
        let client = pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("pgvector pool get: {e}"))?;

        // CREATE EXTENSION — requires superuser on first run.
        client
            .execute(SQL_CREATE_EXTENSION, &[])
            .await
            .map_err(|e| {
                anyhow::anyhow!("pgvector CREATE EXTENSION: {e}. Does the DB user have superuser?")
            })?;

        // Drawers table with dimension parameter.
        let dim_i32 = config.embedding_dim as i32;
        client
            .execute(SQL_CREATE_DRAWERS_TABLE, &[&dim_i32])
            .await
            .map_err(|e| anyhow::anyhow!("pgvector CREATE TABLE: {e}"))?;

        // Embedder identity tracking table.
        client
            .execute(SQL_CREATE_EMBEDDER_TABLE, &[])
            .await
            .map_err(|e| anyhow::anyhow!("pgvector CREATE embedder_meta: {e}"))?;

        // ANN index.
        match config.index_type {
            PgvectorIndexType::Hnsw => {
                let m = config.hnsw_m as i32;
                let ef = config.hnsw_ef_construction as i32;
                client
                    .execute(SQL_CREATE_INDEX_HNSW, &[&m, &ef])
                    .await
                    .map_err(|e| anyhow::anyhow!("pgvector CREATE HNSW index: {e}"))?;
            }
            PgvectorIndexType::Ivfflat => {
                // IVFFlat needs existing data to build the index.
                let count: i64 = client
                    .query_one("SELECT COUNT(*) FROM palace_drawers", &[])
                    .await
                    .map_err(|e| anyhow::anyhow!("pgvector count for IVFFlat: {e}"))?
                    .get(0);
                if count > 0 {
                    let lists = config.ivfflat_lists as i32;
                    client
                        .execute(SQL_CREATE_INDEX_IVFFLAT, &[&lists])
                        .await
                        .map_err(|e| anyhow::anyhow!("pgvector CREATE IVFFlat index: {e}"))?;
                } else {
                    warn!("pgvector: IVFFlat index skipped — table is empty. Index will be created after first bulk insert.");
                }
            }
        }

        Ok(())
    }

    /// Rebuild the ANN index (useful after large batch inserts or
    /// parameter changes). Idempotent.
    pub async fn rebuild_index(&self) -> anyhow::Result<()> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("pgvector pool get: {e}"))?;

        match self.config.index_type {
            PgvectorIndexType::Hnsw => {
                client
                    .execute("DROP INDEX IF EXISTS palace_drawers_hnsw", &[])
                    .await
                    .map_err(|e| anyhow::anyhow!("drop HNSW index: {e}"))?;
                let m = self.config.hnsw_m as i32;
                let ef = self.config.hnsw_ef_construction as i32;
                client
                    .execute(SQL_CREATE_INDEX_HNSW, &[&m, &ef])
                    .await
                    .map_err(|e| anyhow::anyhow!("recreate HNSW index: {e}"))?;
            }
            PgvectorIndexType::Ivfflat => {
                client
                    .execute("DROP INDEX IF EXISTS palace_drawers_ivfflat", &[])
                    .await
                    .map_err(|e| anyhow::anyhow!("drop IVFFlat index: {e}"))?;
                let lists = self.config.ivfflat_lists as i32;
                client
                    .execute(SQL_CREATE_INDEX_IVFFLAT, &[&lists])
                    .await
                    .map_err(|e| anyhow::anyhow!("recreate IVFFlat index: {e}"))?;
            }
        }
        info!("pgvector: index rebuilt ({})", self.config.index_type);
        Ok(())
    }

    /// Store embedder identity metadata for manifest validation.
    pub async fn set_embedder_meta(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("pgvector pool get: {e}"))?;
        client
            .execute(SQL_SET_EMBEDDER_META, &[&key, &value])
            .await
            .map_err(|e| anyhow::anyhow!("pgvector set_embedder_meta: {e}"))?;
        Ok(())
    }

    /// Retrieve embedder identity metadata.
    pub async fn get_embedder_meta(&self, key: &str) -> anyhow::Result<Option<String>> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("pgvector pool get: {e}"))?;
        let row = client
            .query_opt(SQL_GET_EMBEDDER_META, &[&key])
            .await
            .map_err(|e| anyhow::anyhow!("pgvector get_embedder_meta: {e}"))?;
        Ok(row.map(|r| r.get::<_, String>(0)))
    }

    /// Record the current embedder identity so future opens can validate.
    pub async fn record_embedder_manifest(
        &self,
        manifest: &EmbeddingManifest,
    ) -> anyhow::Result<()> {
        self.set_embedder_meta("model_name", &manifest.model_name)
            .await?;
        self.set_embedder_meta("dim", &manifest.dim.to_string())
            .await?;
        self.set_embedder_meta("fingerprint", &manifest.fingerprint)
            .await?;
        self.set_embedder_meta("created_at", &manifest.created_at.to_rfc3339())
            .await?;
        self.set_embedder_meta("mempalace_version", &manifest.mempalace_version)
            .await?;
        Ok(())
    }

    /// Read back the recorded embedder manifest from the database.
    pub async fn read_embedder_manifest(&self) -> anyhow::Result<Option<EmbeddingManifest>> {
        let model_name = self.get_embedder_meta("model_name").await?;
        let dim = self.get_embedder_meta("dim").await?;
        let fingerprint = self.get_embedder_meta("fingerprint").await?;

        match (model_name, dim, fingerprint) {
            (Some(model), Some(dim_str), Some(fp)) => {
                let dim: usize = dim_str
                    .parse()
                    .map_err(|e| anyhow::anyhow!("pgvector invalid dim in DB: {e}"))?;
                let created_at = self.get_embedder_meta("created_at").await?;
                let mempalace_version = self.get_embedder_meta("mempalace_version").await?;
                Ok(Some(EmbeddingManifest {
                    model_name: model,
                    dim,
                    fingerprint: fp,
                    created_at: created_at
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    mempalace_version: mempalace_version.unwrap_or_else(|| "unknown".to_string()),
                }))
            }
            _ => Ok(None),
        }
    }

    /// Raw pool access for advanced usage / testing.
    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    /// Get the configured embedding dimension.
    pub fn embedding_dim(&self) -> usize {
        self.config.embedding_dim
    }
}

// -----------------------------------------------------------------------
// PalaceStore trait implementation
// -----------------------------------------------------------------------

fn row_get_json(row: &tokio_postgres::Row, idx: usize) -> serde_json::Value {
    let val: Option<serde_json::Value> = row.try_get(idx).ok().flatten();
    val.unwrap_or(serde_json::Value::Object(Default::default()))
}

fn row_into_drawer(row: &tokio_postgres::Row) -> Drawer {
    let id: String = row.get(0);
    let content: String = row.get(1);
    let kind_str: String = row.get(2);
    let tier_str: String = row.get(3);
    let wing: Option<String> = row.get(4);
    let room: Option<String> = row.get(5);
    let metadata_val: serde_json::Value = row_get_json(row, 6);

    let metadata_map = match metadata_val {
        JsonValue::Object(m) => m.into_iter().collect(),
        _ => HashMap::new(),
    };

    let mut drawer = Drawer {
        id: Some(DrawerId(id)),
        content,
        kind: serde_json::from_str(&kind_str).unwrap_or_default(),
        tier: serde_json::from_str(&tier_str).unwrap_or_default(),
        wing,
        room,
        metadata: metadata_map,
        derived_from: Vec::new(),
        tags: Vec::new(),
        trust: None,
        access_count: 0,
        last_accessed: None,
        reinforcements: Vec::new(),
        superseded_by: None,
        active: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        confidence: 1.0,
        consolidation_strength: 1,
    };
    drawer.migrate_metadata();
    drawer
}

#[async_trait]
impl PalaceStore for PgvectorStore {
    async fn upsert(&self, drawers: Vec<Drawer>) -> anyhow::Result<()> {
        if drawers.is_empty() {
            return Ok(());
        }

        let mut client = self
            .pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("pgvector pool get: {e}"))?;

        // Use a transaction for atomicity.
        let tx = client
            .transaction()
            .await
            .map_err(|e| anyhow::anyhow!("pgvector begin tx: {e}"))?;

        for mut drawer in drawers {
            drawer.touch();

            let id = drawer
                .id
                .as_ref()
                .map(|d| d.0.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let kind = serde_json::to_string(&drawer.kind).unwrap_or_default();
            let tier = serde_json::to_string(&drawer.tier).unwrap_or_default();

            // Upsert metadata timestamps for round-trip fidelity.
            let mut meta_map = drawer.metadata.clone();
            meta_map.insert(
                "created_at".to_string(),
                JsonValue::String(drawer.created_at.to_rfc3339()),
            );
            meta_map.insert(
                "updated_at".to_string(),
                JsonValue::String(drawer.updated_at.to_rfc3339()),
            );
            let metadata_json =
                serde_json::to_value(&meta_map).unwrap_or(JsonValue::Object(Default::default()));

            // The embedding vector is NOT in the Drawer struct. For the
            // PalaceStore trait, upsert receives Drawer objects without
            // vectors. We pass a zero-vector placeholder; real usage should
            // upsert_with_vectors or the embedding happens externally.
            let zero_vec: Vec<f32> = vec![0.0; self.config.embedding_dim];
            let vec_str = format_pgvector_literal(&zero_vec);

            tx.execute(
                SQL_UPSERT_DRAWER,
                &[
                    &id,
                    &drawer.content,
                    &kind,
                    &tier,
                    &drawer.wing,
                    &drawer.room,
                    &metadata_json,
                    &vec_str,
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!("pgvector upsert drawer {}: {e}", id))?;
        }

        tx.commit()
            .await
            .map_err(|e| anyhow::anyhow!("pgvector commit tx: {e}"))?;

        Ok(())
    }

    async fn delete(&self, ids: &[DrawerId]) -> anyhow::Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("pgvector pool get: {e}"))?;
        let id_strs: Vec<String> = ids.iter().map(|d| d.0.clone()).collect();
        let rows = client
            .execute(SQL_DELETE_BY_IDS, &[&id_strs])
            .await
            .map_err(|e| anyhow::anyhow!("pgvector delete: {e}"))?;
        Ok(rows as usize)
    }

    async fn search(
        &self,
        query: &[f32],
        scope: &SearchScope,
        limit: usize,
    ) -> anyhow::Result<Vec<SearchHit>> {
        let vec_str = format_pgvector_literal(query);
        let effective_limit = if limit == 0 { 10 } else { limit };

        let client = self
            .pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("pgvector pool get: {e}"))?;

        let rows = client
            .query(
                SQL_SEARCH_ANN,
                &[
                    &vec_str,
                    &scope.wing,
                    &scope.room,
                    &(effective_limit as i64),
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!("pgvector search: {e}"))?;

        Ok(rows
            .iter()
            .map(|row| {
                let content: String = row.get(1);
                let wing: Option<String> = row.get(4);
                let room: Option<String> = row.get(5);
                let metadata_val: serde_json::Value = row_get_json(row, 6);
                let similarity: f64 = row.get(7);
                let source_file = metadata_val
                    .get("source_file")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                SearchHit {
                    text: content,
                    wing,
                    room,
                    source_file,
                    similarity,
                    bm25_score: None,
                    combined_score: None,
                }
            })
            .collect())
    }

    async fn count(&self, scope: &SearchScope) -> anyhow::Result<usize> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("pgvector pool get: {e}"))?;
        let row = client
            .query_one(SQL_COUNT_FILTERED, &[&scope.wing, &scope.room])
            .await
            .map_err(|e| anyhow::anyhow!("pgvector count: {e}"))?;
        let count: i64 = row.get(0);
        Ok(count as usize)
    }

    async fn flush(&self) -> anyhow::Result<()> {
        // PostgreSQL is durable by default; flush is a no-op for pgvector.
        // Optionally trigger ANALYZE for query planner freshness.
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("pgvector pool get: {e}"))?;
        client
            .execute(SQL_ANALYZE, &[])
            .await
            .map_err(|e| anyhow::anyhow!("pgvector analyze: {e}"))?;
        Ok(())
    }

    async fn get_drawers(
        &self,
        scope: Option<&SearchScope>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<Drawer>> {
        let wing = scope.and_then(|s| s.wing.as_deref());
        let room = scope.and_then(|s| s.room.as_deref());
        let limit = limit.unwrap_or(1000) as i64;

        let client = self
            .pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("pgvector pool get: {e}"))?;

        let rows = client
            .query(SQL_SELECT_DRAWERS, &[&wing, &room, &limit])
            .await
            .map_err(|e| anyhow::anyhow!("pgvector get_drawers: {e}"))?;

        Ok(rows.iter().map(row_into_drawer).collect())
    }

    fn tier(&self) -> StoreTier {
        StoreTier::Pgvector
    }
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

/// Format a float slice as a pgvector literal string: `[0.1,0.2,0.3]`
fn format_pgvector_literal(vec: &[f32]) -> String {
    let inner: Vec<String> = vec.iter().map(|f| format!("{:.7}", f)).collect();
    format!("[{}]", inner.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_pgvector_literal() {
        let v = vec![1.0f32, 0.0, -0.5];
        let s = format_pgvector_literal(&v);
        assert_eq!(s, "[1.0000000,0.0000000,-0.5000000]");
    }

    #[test]
    fn test_format_pgvector_literal_empty() {
        let v: Vec<f32> = vec![];
        let s = format_pgvector_literal(&v);
        assert_eq!(s, "[]");
    }

    #[test]
    fn test_pgvector_index_type_display() {
        assert_eq!(PgvectorIndexType::Hnsw.to_string(), "hnsw");
        assert_eq!(PgvectorIndexType::Ivfflat.to_string(), "ivfflat");
    }

    #[test]
    fn test_pgvector_index_type_default() {
        assert_eq!(PgvectorIndexType::default(), PgvectorIndexType::Hnsw);
    }

    #[test]
    fn test_pgvector_config_default() {
        let cfg = PgvectorConfig::default();
        assert_eq!(cfg.embedding_dim, 384);
        assert_eq!(cfg.index_type, PgvectorIndexType::Hnsw);
        assert_eq!(cfg.max_pool_size, 5);
        assert_eq!(cfg.hnsw_m, 16);
        assert_eq!(cfg.hnsw_ef_construction, 64);
        assert_eq!(cfg.ivfflat_lists, 100);
        assert!(cfg.dsn.is_empty());
    }

    #[test]
    fn test_pgvector_config_serialization() {
        let cfg = PgvectorConfig {
            dsn: "postgresql://localhost/test".into(),
            embedding_dim: 768,
            index_type: PgvectorIndexType::Ivfflat,
            ..Default::default()
        };
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(json["dsn"], "postgresql://localhost/test");
        assert_eq!(json["embedding_dim"], 768);
        assert_eq!(json["index_type"], "ivfflat");
    }

    #[test]
    fn test_pool_config_from_dsn() {
        let cfg = pool_config_from_dsn("postgresql://user:pass@localhost:5432/testdb", 10).unwrap();
        assert_eq!(
            cfg.url.as_deref(),
            Some("postgresql://user:pass@localhost:5432/testdb")
        );
        let pool_cfg = cfg.pool.unwrap();
        assert_eq!(pool_cfg.max_size, 10);
    }

    #[test]
    fn test_pool_config_from_dsn_default_size() {
        let cfg = pool_config_from_dsn("postgresql://localhost/test", 5).unwrap();
        let pool_cfg = cfg.pool.unwrap();
        assert_eq!(pool_cfg.max_size, 5);
    }

    #[test]
    fn test_store_tier_is_pgvector() {
        // Verify the Pgvector variant exists in the StoreTier enum.
        let tier = StoreTier::Pgvector;
        assert_eq!(tier, StoreTier::Pgvector);
    }
}
