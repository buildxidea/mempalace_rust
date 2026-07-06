// =====================================================================
// QdrantStore — Qdrant REST API backend (backend-qdrant)
// =====================================================================
//
// Implements `PalaceStore` against the Qdrant vector database via its
// HTTP REST API. No external SDK dependency — uses the already-present
// `reqwest` crate for all HTTP calls.
//
// Configuration:
//   - `MEMPALACE_QDRANT_URL` env var  (default: http://localhost:6333)
//   - `MEMPALACE_QDRANT_COLLECTION` env var  (default: mempalace_drawers)
//   - `MEMPALACE_QDRANT_API_KEY` env var  (optional, for Qdrant Cloud)
//
// Collection management:
//   - `QdrantStore::ensure_collection()` creates the collection with
//     cosine distance and the configured vector dimension if it does
//     not exist. HNSW parameters are set to sensible defaults.
//   - Called automatically on first upsert if the collection is missing.
//
// CRUD with payload filter:
//   - All search/count/scroll operations accept a `SearchScope` that is
//     translated into Qdrant payload filters (wing, room, hall).
//   - Upsert uses Qdrant's point upsert (idempotent by point ID).
//   - Delete uses Qdrant's point delete by filter.
//
// Bulk metadata scroll:
//   - `get_drawers` uses Qdrant's scroll endpoint with payload retrieval
//     to return full Drawer objects including all metadata.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::palace::{Drawer, DrawerId, PalaceStore, SearchHit, SearchScope, StoreTier};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for the Qdrant backend.
///
/// Populated from environment variables with the prefix `MEMPALACE_QDRANT_`.
#[derive(Debug, Clone)]
pub struct QdrantConfig {
    /// Qdrant HTTP API base URL (default: http://localhost:6333).
    pub url: String,
    /// Collection name (default: mempalace_drawers).
    pub collection: String,
    /// Optional API key for Qdrant Cloud.
    pub api_key: Option<String>,
    /// Embedding dimensionality (must match the configured embedder).
    pub dimension: usize,
}

impl Default for QdrantConfig {
    fn default() -> Self {
        Self {
            url: std::env::var("MEMPALACE_QDRANT_URL")
                .unwrap_or_else(|_| "http://localhost:6333".to_string()),
            collection: std::env::var("MEMPALACE_QDRANT_COLLECTION")
                .unwrap_or_else(|_| "mempalace_drawers".to_string()),
            api_key: std::env::var("MEMPALACE_QDRANT_API_KEY").ok(),
            dimension: 384, // common default for MiniLM variants
        }
    }
}

// ---------------------------------------------------------------------------
// Qdrant REST API request/response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct CreateCollectionRequest {
    vectors: VectorParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    optimizers_config: Option<OptimizersConfig>,
}

#[derive(Debug, Serialize)]
struct VectorParams {
    size: usize,
    distance: String,
}

#[derive(Debug, Serialize)]
struct OptimizersConfig {
    #[serde(rename = "memmap_threshold")]
    memmap_threshold: usize,
}

#[derive(Debug, Serialize)]
struct UpsertRequest {
    points: Vec<Point>,
}

#[derive(Debug, Serialize)]
struct Point {
    id: PointId,
    vector: Vec<f32>,
    payload: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum PointId {
    Uuid(String),
    Num(u64),
}

#[derive(Debug, Serialize)]
struct DeleteRequest {
    filter: Filter,
}

#[derive(Debug, Clone, Serialize)]
struct Filter {
    must: Vec<Condition>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum Condition {
    Field(FieldCondition),
}

#[derive(Debug, Clone, Serialize)]
struct FieldCondition {
    key: String,
    #[serde(rename = "match")]
    match_condition: MatchValue,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum MatchValue {
    Keyword(String),
    Integer(i64),
    Boolean(bool),
}

#[derive(Debug, Serialize)]
struct SearchRequest {
    vector: Vec<f32>,
    limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<Filter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    with_payload: Option<bool>,
}

#[derive(Debug, Serialize)]
struct CountRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<Filter>,
}

#[derive(Debug, Serialize)]
struct ScrollRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<Filter>,
    limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    with_payload: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ScrollResponse {
    result: ScrollResult,
}

#[derive(Debug, Deserialize)]
struct ScrollResult {
    points: Vec<ScrollPoint>,
    #[serde(default)]
    next_page_offset: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ScrollPoint {
    id: PointIdValue,
    #[serde(default)]
    payload: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PointIdValue {
    Uuid(String),
    Num(u64),
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    result: Vec<SearchResult>,
}

#[derive(Debug, Deserialize)]
struct SearchResult {
    id: PointIdValue,
    score: f64,
}

#[derive(Debug, Deserialize)]
struct CountResponse {
    result: CountResult,
}

#[derive(Debug, Deserialize)]
struct CountResult {
    count: usize,
}

#[derive(Debug, Deserialize)]
struct CollectionInfoResponse {
    result: Option<CollectionInfo>,
}

#[derive(Debug, Deserialize)]
struct CollectionInfo {
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    detail: Option<String>,
}

// ---------------------------------------------------------------------------
// QdrantStore
// ---------------------------------------------------------------------------

/// PalaceStore implementation backed by Qdrant's REST API.
///
/// Uses `reqwest::Client` for HTTP calls — no Qdrant SDK dependency.
/// All methods are async and safe to share across tokio tasks via `Arc`.
pub struct QdrantStore {
    client: Client,
    config: QdrantConfig,
    /// Ensures collection creation happens exactly once.
    collection_init: Arc<Mutex<bool>>,
}

impl QdrantStore {
    /// Create a new QdrantStore with the given configuration.
    pub fn new(config: QdrantConfig) -> Self {
        Self {
            client: Client::new(),
            config,
            collection_init: Arc::new(Mutex::new(false)),
        }
    }

    /// Create a new QdrantStore from environment variables.
    pub fn from_env(dimension: usize) -> Self {
        let mut config = QdrantConfig::default();
        config.dimension = dimension;
        Self::new(config)
    }

    /// Full URL for a collection endpoint.
    fn collection_url(&self, path: &str) -> String {
        format!(
            "{}/collections/{}{}",
            self.config.url, self.config.collection, path
        )
    }

    /// Build an authorization header value, if an API key is configured.
    fn auth_header(&self) -> Option<String> {
        self.config
            .api_key
            .as_ref()
            .map(|key| format!("Bearer {key}"))
    }

    /// Add auth header to a request builder if configured.
    fn maybe_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(auth) = self.auth_header() {
            builder.header("Authorization", auth)
        } else {
            builder
        }
    }

    /// Ensure the collection exists. Idempotent — creates it only once.
    async fn ensure_collection(&self) -> anyhow::Result<()> {
        let mut init = self.collection_init.lock().await;
        if *init {
            return Ok(());
        }

        // Check if collection exists
        let url = self.collection_url("");
        let mut req = self.client.get(&url);
        req = self.maybe_auth(req);
        let resp = req.send().await?;

        if resp.status().as_u16() == 404 || resp.status().as_u16() == 400 {
            // Collection doesn't exist — create it
            let create_url = format!("{}/collections/{}", self.config.url, self.config.collection);
            let body = CreateCollectionRequest {
                vectors: VectorParams {
                    size: self.config.dimension,
                    distance: "Cosine".to_string(),
                },
                optimizers_config: Some(OptimizersConfig {
                    memmap_threshold: 20000,
                }),
            };
            let mut req = self.client.put(&create_url).json(&body);
            req = self.maybe_auth(req);
            let resp = req.send().await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                anyhow::bail!("Qdrant create collection failed ({status}): {text}");
            }
        } else if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant collection check failed ({status}): {text}");
        }

        *init = true;
        Ok(())
    }

    /// Convert a SearchScope into a Qdrant payload filter.
    fn scope_to_filter(scope: &SearchScope) -> Option<Filter> {
        let mut conditions = Vec::new();

        if let Some(ref wing) = scope.wing {
            conditions.push(Condition::Field(FieldCondition {
                key: "wing".to_string(),
                match_condition: MatchValue::Keyword(wing.clone()),
            }));
        }
        if let Some(ref room) = scope.room {
            conditions.push(Condition::Field(FieldCondition {
                key: "room".to_string(),
                match_condition: MatchValue::Keyword(room.clone()),
            }));
        }
        if let Some(ref hall) = scope.hall {
            conditions.push(Condition::Field(FieldCondition {
                key: "kind".to_string(),
                match_condition: MatchValue::Keyword(hall.clone()),
            }));
        }

        if conditions.is_empty() {
            None
        } else {
            Some(Filter { must: conditions })
        }
    }

    /// Convert a Qdrant point payload + id into a Drawer.
    fn point_to_drawer(
        id_val: &PointIdValue,
        payload: &HashMap<String, serde_json::Value>,
    ) -> Drawer {
        let id_str = match id_val {
            PointIdValue::Uuid(s) => s.clone(),
            PointIdValue::Num(n) => n.to_string(),
        };

        let content = payload
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let kind_str = payload
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("raw");

        let tier_str = payload
            .get("tier")
            .and_then(|v| v.as_str())
            .unwrap_or("working");

        let wing = payload
            .get("wing")
            .and_then(|v| v.as_str())
            .map(String::from);

        let room = payload
            .get("room")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Build the metadata map from the payload, excluding known fields
        let mut metadata = HashMap::new();
        for (k, v) in payload {
            if !matches!(
                k.as_str(),
                "content"
                    | "kind"
                    | "tier"
                    | "wing"
                    | "room"
                    | "tags"
                    | "trust"
                    | "access_count"
                    | "active"
                    | "confidence"
                    | "consolidation_strength"
                    | "created_at"
                    | "updated_at"
            ) {
                metadata.insert(k.clone(), v.clone());
            }
        }

        let tags = payload
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let trust = payload
            .get("trust")
            .and_then(|v| v.as_str())
            .map(String::from);

        let access_count = payload
            .get("access_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let active = payload
            .get("active")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let confidence = payload
            .get("confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);

        let consolidation_strength = payload
            .get("consolidation_strength")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;

        let created_at = payload
            .get("created_at")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);

        let updated_at = payload
            .get("updated_at")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);

        let mut drawer = Drawer {
            id: Some(DrawerId(id_str)),
            content,
            kind: serde_json::from_str(&format!("\"{kind_str}\"")).unwrap_or_default(),
            tier: serde_json::from_str(&format!("\"{tier_str}\"")).unwrap_or_default(),
            wing,
            room,
            metadata,
            derived_from: vec![],
            tags,
            trust,
            access_count,
            last_accessed: None,
            reinforcements: vec![],
            superseded_by: None,
            active,
            created_at,
            updated_at,
            confidence,
            consolidation_strength,
        };
        drawer.migrate_metadata();
        drawer
    }
}

#[async_trait]
impl PalaceStore for QdrantStore {
    async fn upsert(&self, drawers: Vec<Drawer>) -> anyhow::Result<()> {
        if drawers.is_empty() {
            return Ok(());
        }

        self.ensure_collection().await?;

        let mut points = Vec::with_capacity(drawers.len());
        for (i, mut drawer) in drawers.into_iter().enumerate() {
            if drawer.id.is_none() {
                drawer.id = Some(DrawerId(uuid::Uuid::new_v4().to_string()));
            }
            drawer.touch();

            let id = drawer.id.as_ref().map(|d| d.0.clone()).unwrap_or_default();

            // Insert typed fields into metadata for round-trip fidelity
            drawer.metadata.insert(
                "content".to_string(),
                serde_json::Value::String(drawer.content.clone()),
            );
            drawer.metadata.insert(
                "kind".to_string(),
                serde_json::Value::String(
                    serde_json::to_string(&drawer.kind)
                        .unwrap_or_default()
                        .trim_matches('"')
                        .to_string(),
                ),
            );
            drawer.metadata.insert(
                "tier".to_string(),
                serde_json::Value::String(
                    serde_json::to_string(&drawer.tier)
                        .unwrap_or_default()
                        .trim_matches('"')
                        .to_string(),
                ),
            );
            if let Some(ref wing) = drawer.wing {
                drawer
                    .metadata
                    .insert("wing".to_string(), serde_json::Value::String(wing.clone()));
            }
            if let Some(ref room) = drawer.room {
                drawer
                    .metadata
                    .insert("room".to_string(), serde_json::Value::String(room.clone()));
            }
            if !drawer.tags.is_empty() {
                let tags: Vec<serde_json::Value> = drawer
                    .tags
                    .iter()
                    .map(|t| serde_json::Value::String(t.clone()))
                    .collect();
                drawer
                    .metadata
                    .insert("tags".to_string(), serde_json::Value::Array(tags));
            }
            if let Some(ref trust) = drawer.trust {
                drawer.metadata.insert(
                    "trust".to_string(),
                    serde_json::Value::String(trust.clone()),
                );
            }
            drawer.metadata.insert(
                "access_count".to_string(),
                serde_json::Value::Number(drawer.access_count.into()),
            );
            drawer
                .metadata
                .insert("active".to_string(), serde_json::Value::Bool(drawer.active));
            drawer.metadata.insert(
                "confidence".to_string(),
                serde_json::json!(drawer.confidence),
            );
            drawer.metadata.insert(
                "consolidation_strength".to_string(),
                serde_json::json!(drawer.consolidation_strength),
            );
            drawer.metadata.insert(
                "created_at".to_string(),
                serde_json::Value::String(drawer.created_at.to_rfc3339()),
            );
            drawer.metadata.insert(
                "updated_at".to_string(),
                serde_json::Value::String(drawer.updated_at.to_rfc3339()),
            );

            // Use the vector from metadata if present; otherwise a zero vector
            // (caller is responsible for providing the correct embedding).
            let vector = drawer
                .metadata
                .get("_vector")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| vec![0.0; self.config.dimension]);

            // Remove _vector from stored metadata
            drawer.metadata.remove("_vector");

            points.push(Point {
                id: PointId::Uuid(id),
                vector,
                payload: drawer.metadata.clone(),
            });
        }

        let body = UpsertRequest { points };
        let url = format!(
            "{}/collections/{}/points",
            self.config.url, self.config.collection
        );
        let mut req = self.client.put(&url).json(&body);
        req = self.maybe_auth(req);
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant upsert failed ({status}): {text}");
        }

        Ok(())
    }

    async fn delete(&self, ids: &[DrawerId]) -> anyhow::Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }

        self.ensure_collection().await?;

        // Qdrant delete by list of IDs
        #[derive(Debug, Serialize)]
        struct DeleteByIdsRequest {
            points: Vec<String>,
        }

        let point_ids: Vec<String> = ids.iter().map(|id| id.0.clone()).collect();
        let body = DeleteByIdsRequest { points: point_ids };

        let url = format!(
            "{}/collections/{}/points/delete",
            self.config.url, self.config.collection
        );
        let mut req = self.client.post(&url).json(&body);
        req = self.maybe_auth(req);
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant delete failed ({status}): {text}");
        }

        Ok(ids.len())
    }

    async fn search(
        &self,
        query: &[f32],
        scope: &SearchScope,
        limit: usize,
    ) -> anyhow::Result<Vec<SearchHit>> {
        self.ensure_collection().await?;

        let filter = Self::scope_to_filter(scope);
        let body = SearchRequest {
            vector: query.to_vec(),
            limit,
            filter,
            with_payload: Some(true),
        };

        let url = format!(
            "{}/collections/{}/points/search",
            self.config.url, self.config.collection
        );
        let mut req = self.client.post(&url).json(&body);
        req = self.maybe_auth(req);
        let resp = req.send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant search failed ({status}): {text}");
        }

        let search_resp: SearchResponse = resp.json().await?;
        let mut hits = Vec::with_capacity(search_resp.result.len());

        for result in &search_resp.result {
            // Fetch the full point to get the payload
            let get_url = format!(
                "{}/collections/{}/points/{}",
                self.config.url,
                self.config.collection,
                match &result.id {
                    PointIdValue::Uuid(s) => s.clone(),
                    PointIdValue::Num(n) => n.to_string(),
                }
            );
            let mut get_req = self.client.get(&get_url);
            get_req = self.maybe_auth(get_req);
            let get_resp = get_req.send().await?;

            if get_resp.status().is_success() {
                #[derive(Debug, Deserialize)]
                struct PointResponse {
                    result: ScrollPoint,
                }
                if let Ok(point_resp) = get_resp.json::<PointResponse>().await {
                    let payload = point_resp.result.payload.unwrap_or_default();
                    let source_file = payload
                        .get("source_file")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    hits.push(SearchHit {
                        text: payload
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        wing: scope.wing.clone().or_else(|| {
                            payload
                                .get("wing")
                                .and_then(|v| v.as_str())
                                .map(String::from)
                        }),
                        room: scope.room.clone().or_else(|| {
                            payload
                                .get("room")
                                .and_then(|v| v.as_str())
                                .map(String::from)
                        }),
                        source_file,
                        similarity: result.score,
                        bm25_score: None,
                        combined_score: None,
                    });
                }
            }
        }

        Ok(hits)
    }

    async fn count(&self, scope: &SearchScope) -> anyhow::Result<usize> {
        self.ensure_collection().await?;

        let filter = Self::scope_to_filter(scope);
        let body = CountRequest { filter };

        let url = format!(
            "{}/collections/{}/points/count",
            self.config.url, self.config.collection
        );
        let mut req = self.client.post(&url).json(&body);
        req = self.maybe_auth(req);
        let resp = req.send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant count failed ({status}): {text}");
        }

        let count_resp: CountResponse = resp.json().await?;
        Ok(count_resp.result.count)
    }

    async fn flush(&self) -> anyhow::Result<()> {
        // Qdrant is durable by default — no explicit flush needed.
        Ok(())
    }

    async fn get_drawers(
        &self,
        scope: Option<&SearchScope>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<Drawer>> {
        self.ensure_collection().await?;

        let limit = limit.unwrap_or(1000);
        let filter = scope.and_then(Self::scope_to_filter);

        let mut all_drawers = Vec::new();
        let mut offset: Option<String> = None;

        loop {
            let batch_limit = (limit - all_drawers.len()).min(100);
            if batch_limit == 0 {
                break;
            }

            let body = ScrollRequest {
                filter: filter.clone(),
                limit: batch_limit,
                with_payload: Some(true),
                offset: offset.clone(),
            };

            let url = format!(
                "{}/collections/{}/points/scroll",
                self.config.url, self.config.collection
            );
            let mut req = self.client.post(&url).json(&body);
            req = self.maybe_auth(req);
            let resp = req.send().await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                anyhow::bail!("Qdrant scroll failed ({status}): {text}");
            }

            let scroll_resp: ScrollResponse = resp.json().await?;

            for point in &scroll_resp.result.points {
                let payload = point.payload.as_ref().cloned().unwrap_or_default();
                all_drawers.push(Self::point_to_drawer(&point.id, &payload));
            }

            if all_drawers.len() >= limit {
                break;
            }

            offset = scroll_resp.result.next_page_offset;
            if offset.is_none() {
                break;
            }
        }

        all_drawers.truncate(limit);
        Ok(all_drawers)
    }

    fn tier(&self) -> StoreTier {
        StoreTier::Qdrant
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palace::DrawerKind;

    fn test_config() -> QdrantConfig {
        QdrantConfig {
            url: "http://localhost:6333".to_string(),
            collection: "test_mempalace".to_string(),
            api_key: None,
            dimension: 4,
        }
    }

    #[test]
    fn test_qdrant_config_defaults() {
        // Ensure default config doesn't panic
        let _ = QdrantConfig::default();
    }

    #[test]
    fn test_scope_to_filter_empty() {
        let scope = SearchScope::default();
        assert!(QdrantStore::scope_to_filter(&scope).is_none());
    }

    #[test]
    fn test_scope_to_filter_wing_only() {
        let scope = SearchScope {
            wing: Some("myproject".to_string()),
            ..Default::default()
        };
        let filter = QdrantStore::scope_to_filter(&scope).unwrap();
        assert_eq!(filter.must.len(), 1);
        match &filter.must[0] {
            Condition::Field(fc) => {
                assert_eq!(fc.key, "wing");
            }
        }
    }

    #[test]
    fn test_scope_to_filter_wing_and_room() {
        let scope = SearchScope {
            wing: Some("myproject".to_string()),
            room: Some("backend".to_string()),
            ..Default::default()
        };
        let filter = QdrantStore::scope_to_filter(&scope).unwrap();
        assert_eq!(filter.must.len(), 2);
    }

    #[test]
    fn test_point_to_drawer_basic() {
        let mut payload = HashMap::new();
        payload.insert(
            "content".to_string(),
            serde_json::Value::String("hello world".to_string()),
        );
        payload.insert(
            "kind".to_string(),
            serde_json::Value::String("fact".to_string()),
        );
        payload.insert(
            "tier".to_string(),
            serde_json::Value::String("working".to_string()),
        );
        payload.insert(
            "wing".to_string(),
            serde_json::Value::String("test_wing".to_string()),
        );
        payload.insert(
            "room".to_string(),
            serde_json::Value::String("test_room".to_string()),
        );
        payload.insert(
            "source_file".to_string(),
            serde_json::Value::String("test.md".to_string()),
        );
        payload.insert(
            "created_at".to_string(),
            serde_json::Value::String("2026-01-01T00:00:00Z".to_string()),
        );
        payload.insert(
            "updated_at".to_string(),
            serde_json::Value::String("2026-01-02T00:00:00Z".to_string()),
        );
        payload.insert("confidence".to_string(), serde_json::json!(0.95));
        payload.insert("consolidation_strength".to_string(), serde_json::json!(3));
        payload.insert("active".to_string(), serde_json::json!(true));
        payload.insert("access_count".to_string(), serde_json::json!(5));

        let id = PointIdValue::Uuid("test-uuid-123".to_string());
        let drawer = QdrantStore::point_to_drawer(&id, &payload);

        assert_eq!(drawer.id.as_ref().unwrap().0, "test-uuid-123");
        assert_eq!(drawer.content, "hello world");
        assert_eq!(drawer.wing.as_deref(), Some("test_wing"));
        assert_eq!(drawer.room.as_deref(), Some("test_room"));
        assert_eq!(drawer.confidence, 0.95);
        assert_eq!(drawer.consolidation_strength, 3);
        assert!(drawer.active);
        assert_eq!(drawer.access_count, 5);
    }

    #[test]
    fn test_point_to_drawer_numeric_id() {
        let mut payload = HashMap::new();
        payload.insert(
            "content".to_string(),
            serde_json::Value::String("test".to_string()),
        );
        payload.insert(
            "kind".to_string(),
            serde_json::Value::String("raw".to_string()),
        );
        payload.insert(
            "tier".to_string(),
            serde_json::Value::String("working".to_string()),
        );

        let id = PointIdValue::Num(42);
        let drawer = QdrantStore::point_to_drawer(&id, &payload);
        assert_eq!(drawer.id.as_ref().unwrap().0, "42");
    }

    #[test]
    fn test_point_to_drawer_with_tags() {
        let mut payload = HashMap::new();
        payload.insert(
            "content".to_string(),
            serde_json::Value::String("tagged".to_string()),
        );
        payload.insert(
            "kind".to_string(),
            serde_json::Value::String("raw".to_string()),
        );
        payload.insert(
            "tier".to_string(),
            serde_json::Value::String("working".to_string()),
        );
        payload.insert("tags".to_string(), serde_json::json!(["rust", "backend"]));
        payload.insert(
            "trust".to_string(),
            serde_json::Value::String("high".to_string()),
        );

        let id = PointIdValue::Uuid("t1".to_string());
        let drawer = QdrantStore::point_to_drawer(&id, &payload);
        assert_eq!(drawer.tags, vec!["rust", "backend"]);
        assert_eq!(drawer.trust.as_deref(), Some("high"));
    }

    #[test]
    fn test_qdrant_store_tier() {
        let store = QdrantStore::new(test_config());
        assert_eq!(store.tier(), StoreTier::Qdrant);
    }

    #[test]
    fn test_collection_url() {
        let store = QdrantStore::new(test_config());
        assert_eq!(
            store.collection_url("/points"),
            "http://localhost:6333/collections/test_mempalace/points"
        );
    }

    #[test]
    fn test_auth_header_none() {
        let store = QdrantStore::new(test_config());
        assert!(store.auth_header().is_none());
    }

    #[test]
    fn test_auth_header_with_key() {
        let mut cfg = test_config();
        cfg.api_key = Some("test-key-123".to_string());
        let store = QdrantStore::new(cfg);
        assert_eq!(store.auth_header().unwrap(), "Bearer test-key-123");
    }

    #[test]
    fn test_serialization_roundtrip() {
        let scope = SearchScope {
            wing: Some("proj".to_string()),
            room: Some("api".to_string()),
            hall: Some("fact".to_string()),
            ..Default::default()
        };
        let filter = QdrantStore::scope_to_filter(&scope).unwrap();
        let json = serde_json::to_string(&filter).unwrap();
        assert!(json.contains("wing"));
        assert!(json.contains("proj"));
        assert!(json.contains("room"));
        assert!(json.contains("api"));
    }

    // Integration test that hits a real Qdrant instance (requires
    // QDRANT_INTEGRATION=1 env var and a running Qdrant on localhost:6333).
    #[tokio::test]
    #[ignore = "requires running Qdrant instance"]
    async fn test_integration_upsert_and_search() {
        if std::env::var("QDRANT_INTEGRATION").is_err() {
            return;
        }

        let config = QdrantConfig {
            url: "http://localhost:6333".to_string(),
            collection: "test_integration".to_string(),
            api_key: None,
            dimension: 4,
        };
        let store = QdrantStore::new(config);

        let drawer = Drawer {
            id: Some(DrawerId("test-id-1".to_string())),
            content: "integration test drawer".to_string(),
            kind: DrawerKind::Fact,
            wing: Some("test_wing".to_string()),
            room: Some("test_room".to_string()),
            metadata: HashMap::from([
                (
                    "_vector".to_string(),
                    serde_json::json!([0.1, 0.2, 0.3, 0.4]),
                ),
                (
                    "source_file".to_string(),
                    serde_json::Value::String("test.md".to_string()),
                ),
            ]),
            ..Drawer::new("integration test drawer")
        };

        store.upsert(vec![drawer]).await.unwrap();

        let count = store.count(&SearchScope::default()).await.unwrap();
        assert!(count >= 1);

        let query = vec![0.1, 0.2, 0.3, 0.4];
        let hits = store
            .search(&query, &SearchScope::default(), 10)
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].text, "integration test drawer");
    }
}
