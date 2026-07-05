//! Source adapter contract — RFC 002.
//!
//! Defines the [`SourceAdapter`] trait, typed record types, schema
//! metadata, and error types for the source adapter subsystem. No
//! first-party adapters ship in this module — only the contract.

use std::collections::HashMap;
use std::fmt;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors originating from source adapter operations.
#[derive(Debug, Error)]
pub enum SourceError {
    /// The requested adapter name is not registered.
    #[error("adapter not found: {0}")]
    AdapterNotFound(String),

    /// Discovery failed (e.g. unreadable path, network error).
    #[error("discovery failed for adapter '{adapter}': {reason}")]
    DiscoveryFailed {
        adapter: String,
        reason: String,
    },

    /// Ingestion failed (parse error, schema mismatch, I/O).
    #[error("ingestion failed for adapter '{adapter}': {reason}")]
    IngestionFailed {
        adapter: String,
        reason: String,
    },

    /// Transform pipeline rejected a record.
    #[error("transform rejected for adapter '{adapter}': {reason}")]
    TransformRejected {
        adapter: String,
        reason: String,
    },

    /// The adapter's schema does not match the expected shape.
    #[error("schema mismatch for adapter '{adapter}': {detail}")]
    SchemaMismatch {
        adapter: String,
        detail: String,
    },

    /// Generic wrapper for `anyhow::Error` propagated through adapters.
    #[error("adapter '{adapter}' error: {source}")]
    Internal {
        adapter: String,
        #[source]
        source: anyhow::Error,
    },
}

/// Convenience alias for results from source adapter operations.
pub type SourceResult<T> = Result<T, SourceError>;

// ---------------------------------------------------------------------------
// SourceRecord — a single typed record produced by an adapter
// ---------------------------------------------------------------------------

/// A single typed record produced by a source adapter.
///
/// Every record carries provenance metadata (`source`, `record_id`,
/// `timestamp`) plus a free-form `payload` that downstream consumers
/// interpret according to the schema returned by
/// [`SourceAdapter::schema`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRecord {
    /// Adapter name that produced this record.
    pub source: String,
    /// Opaque record identifier within the source.
    pub record_id: String,
    /// When the record was created or last modified at the origin.
    pub timestamp: DateTime<Utc>,
    /// Arbitrary key-value payload — shape described by [`SourceSchema`].
    pub payload: HashMap<String, serde_json::Value>,
    /// Optional content blob (raw text, markdown, etc.).
    pub content: Option<String>,
    /// Tags or labels applied by the adapter.
    pub tags: Vec<String>,
}

impl SourceRecord {
    /// Create a minimal record with defaults.
    pub fn new(source: impl Into<String>, record_id: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            record_id: record_id.into(),
            timestamp: Utc::now(),
            payload: HashMap::new(),
            content: None,
            tags: Vec::new(),
        }
    }

    /// Builder-style setter for `content`.
    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    /// Builder-style setter for `tags`.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Builder-style setter for `timestamp`.
    pub fn with_timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.timestamp = ts;
        self
    }

    /// Insert a single payload key-value pair.
    pub fn with_payload(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.payload.insert(key.into(), value);
        self
    }
}

// ---------------------------------------------------------------------------
// SourceSchema — describes the shape of records an adapter produces
// ---------------------------------------------------------------------------

/// Metadata describing the shape of records a source adapter produces.
///
/// Returned by [`SourceAdapter::schema`] so consumers can validate or
/// index records without inspecting every payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSchema {
    /// Human-readable name of the schema.
    pub name: String,
    /// Version string (semver recommended).
    pub version: String,
    /// Per-field type descriptions (`"string"`, `"integer"`, `"datetime"`, etc.).
    pub fields: HashMap<String, String>,
    /// Required fields — every record MUST carry these keys in its payload.
    pub required: Vec<String>,
    /// Optional human-readable description.
    pub description: Option<String>,
}

impl SourceSchema {
    /// Validate that `record` carries all required fields.
    pub fn validate(&self, record: &SourceRecord) -> Result<(), String> {
        for field in &self.required {
            if !record.payload.contains_key(field) {
                return Err(format!(
                    "record '{}' is missing required field '{}'",
                    record.record_id, field
                ));
            }
        }
        Ok(())
    }
}

impl fmt::Display for SourceSchema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} v{} ({} fields, {} required)",
            self.name,
            self.version,
            self.fields.len(),
            self.required.len()
        )
    }
}

// ---------------------------------------------------------------------------
// SourceCapability — what an adapter can do
// ---------------------------------------------------------------------------

/// Capability flags advertised by an adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCapability {
    pub discover: bool,
    pub ingest: bool,
    pub transform: bool,
}

impl SourceCapability {
    pub const FULL: Self = Self {
        discover: true,
        ingest: true,
        transform: true,
    };

    pub const READ_ONLY: Self = Self {
        discover: true,
        ingest: false,
        transform: false,
    };

    pub const INGEST_ONLY: Self = Self {
        discover: false,
        ingest: true,
        transform: false,
    };
}

// ---------------------------------------------------------------------------
// SourceAdapter trait — the core contract
// ---------------------------------------------------------------------------

/// The core trait every source adapter must implement.
///
/// # Lifecycle
///
/// 1. **`discover`** — enumerate available items from the source
///    (file paths, URLs, database rows, etc.) without reading content.
/// 2. **`ingest`** — read and parse the discovered items into
///    [`SourceRecord`]s.
/// 3. **`transform`** — post-process records (normalise, enrich,
///    merge). The default implementation is a pass-through.
///
/// # Implementors
///
/// First-party adapters are intentionally *not* shipped in this module
/// (RFC 002 §3). External crates implement `SourceAdapter` and register
/// them via [`super::registry::AdapterRegistry`].
#[async_trait]
pub trait SourceAdapter: Send + Sync {
    /// Unique adapter name (e.g. `"obsidian"`, `"github_issues"`).
    fn name(&self) -> &str;

    /// Advertised capabilities.
    fn capabilities(&self) -> SourceCapability;

    /// Schema describing the records this adapter produces.
    fn schema(&self) -> SourceSchema;

    /// Discover available items without reading their full content.
    ///
    /// Returns opaque identifiers / paths that can later be passed to
    /// [`Self::ingest`].
    async fn discover(&self) -> SourceResult<Vec<DiscoverItem>>;

    /// Ingest previously discovered items into typed [`SourceRecord`]s.
    async fn ingest(&self, items: &[DiscoverItem]) -> SourceResult<Vec<SourceRecord>>;

    /// Transform (normalise / enrich) records. Default: pass-through.
    async fn transform(&self, records: Vec<SourceRecord>) -> SourceResult<Vec<SourceRecord>> {
        Ok(records)
    }
}

/// An opaque item discovered by [`SourceAdapter::discover`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverItem {
    /// Adapter-local identifier.
    pub id: String,
    /// Optional human-readable label.
    pub label: Option<String>,
    /// Optional path or URL hint.
    pub location: Option<String>,
    /// Arbitrary metadata surfaced during discovery.
    pub metadata: HashMap<String, serde_json::Value>,
}

impl DiscoverItem {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: None,
            location: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_location(mut self, loc: impl Into<String>) -> Self {
        self.location = Some(loc.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Transform pipeline
// ---------------------------------------------------------------------------

/// A named transform step applied in sequence by the pipeline.
pub struct TransformStep {
    pub name: String,
    pub f: Box<dyn Fn(Vec<SourceRecord>) -> SourceResult<Vec<SourceRecord>> + Send + Sync>,
}

impl fmt::Debug for TransformStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransformStep")
            .field("name", &self.name)
            .finish()
    }
}

/// Ordered pipeline of transform steps.
pub struct TransformPipeline {
    steps: Vec<TransformStep>,
}

impl TransformPipeline {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    /// Append a named transform step.
    pub fn push(
        mut self,
        name: impl Into<String>,
        f: Box<dyn Fn(Vec<SourceRecord>) -> SourceResult<Vec<SourceRecord>> + Send + Sync>,
    ) -> Self {
        self.steps.push(TransformStep {
            name: name.into(),
            f,
        });
        self
    }

    /// Run all steps in order.
    pub fn run(&self, records: Vec<SourceRecord>) -> SourceResult<Vec<SourceRecord>> {
        let mut current = records;
        for step in &self.steps {
            current = (step.f)(current)?;
        }
        Ok(current)
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }
}

impl Default for TransformPipeline {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_source_record_defaults() {
        let r = SourceRecord::new("test_adapter", "rec-1");
        assert_eq!(r.source, "test_adapter");
        assert_eq!(r.record_id, "rec-1");
        assert!(r.payload.is_empty());
        assert!(r.content.is_none());
        assert!(r.tags.is_empty());
    }

    #[test]
    fn test_source_record_builder() {
        let r = SourceRecord::new("a", "1")
            .with_content("hello")
            .with_tags(vec!["tag1".into()])
            .with_payload("key", json!("value"));
        assert_eq!(r.content.as_deref(), Some("hello"));
        assert_eq!(r.tags, vec!["tag1"]);
        assert_eq!(r.payload.get("key"), Some(&json!("value")));
    }

    #[test]
    fn test_source_schema_validate_ok() {
        let schema = SourceSchema {
            name: "test".into(),
            version: "1.0.0".into(),
            fields: HashMap::from([("title".into(), "string".into())]),
            required: vec!["title".into()],
            description: None,
        };
        let record = SourceRecord::new("a", "1")
            .with_payload("title", json!("My Title"));
        assert!(schema.validate(&record).is_ok());
    }

    #[test]
    fn test_source_schema_validate_missing_field() {
        let schema = SourceSchema {
            name: "test".into(),
            version: "1.0.0".into(),
            fields: HashMap::new(),
            required: vec!["title".into()],
            description: None,
        };
        let record = SourceRecord::new("a", "1");
        let err = schema.validate(&record).unwrap_err();
        assert!(err.contains("missing required field"));
    }

    #[test]
    fn test_source_schema_display() {
        let schema = SourceSchema {
            name: "my_schema".into(),
            version: "2.1.0".into(),
            fields: HashMap::new(),
            required: vec!["a".into(), "b".into()],
            description: None,
        };
        let display = format!("{schema}");
        assert!(display.contains("my_schema"));
        assert!(display.contains("2.1.0"));
        assert!(display.contains("2 required"));
    }

    #[test]
    fn test_discover_item_builder() {
        let item = DiscoverItem::new("item-1").with_location("/tmp/file.txt");
        assert_eq!(item.id, "item-1");
        assert_eq!(item.location.as_deref(), Some("/tmp/file.txt"));
    }

    #[test]
    fn test_capability_flags() {
        assert!(SourceCapability::FULL.discover);
        assert!(SourceCapability::FULL.ingest);
        assert!(SourceCapability::FULL.transform);

        assert!(SourceCapability::READ_ONLY.discover);
        assert!(!SourceCapability::READ_ONLY.ingest);

        assert!(!SourceCapability::INGEST_ONLY.discover);
        assert!(SourceCapability::INGEST_ONLY.ingest);
    }

    #[test]
    fn test_transform_pipeline_empty() {
        let pipeline = TransformPipeline::new();
        let records = vec![SourceRecord::new("a", "1")];
        let result = pipeline.run(records).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_transform_pipeline_chains_steps() {
        let pipeline = TransformPipeline::new()
            .push("tag_all", Box::new(|mut records| {
                for r in &mut records {
                    r.tags.push("processed".into());
                }
                Ok(records)
            }))
            .push("add_content", Box::new(|mut records| {
                for r in &mut records {
                    r.content = Some("enriched".into());
                }
                Ok(records)
            }));

        let records = vec![SourceRecord::new("a", "1")];
        let result = pipeline.run(records).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].tags, vec!["processed"]);
        assert_eq!(result[0].content.as_deref(), Some("enriched"));
        assert_eq!(pipeline.len(), 2);
    }

    #[test]
    fn test_transform_pipeline_error_propagation() {
        let pipeline = TransformPipeline::new().push("fail", Box::new(|_| {
            Err(SourceError::TransformRejected {
                adapter: "test".into(),
                reason: "intentional".into(),
            })
        }));

        let records = vec![SourceRecord::new("a", "1")];
        let result = pipeline.run(records);
        assert!(result.is_err());
    }

    #[test]
    fn test_source_error_display() {
        let err = SourceError::AdapterNotFound("obsidian".into());
        assert_eq!(err.to_string(), "adapter not found: obsidian");

        let err = SourceError::DiscoveryFailed {
            adapter: "github".into(),
            reason: "network timeout".into(),
        };
        assert!(err.to_string().contains("github"));
        assert!(err.to_string().contains("network timeout"));
    }

    #[test]
    fn test_source_record_serde_roundtrip() {
        let record = SourceRecord::new("test", "id-1")
            .with_content("body")
            .with_tags(vec!["t1".into()])
            .with_payload("k", json!(42));
        let json_str = serde_json::to_string(&record).unwrap();
        let parsed: SourceRecord = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.source, "test");
        assert_eq!(parsed.record_id, "id-1");
        assert_eq!(parsed.content.as_deref(), Some("body"));
        assert_eq!(parsed.payload.get("k"), Some(&json!(42)));
    }

    #[test]
    fn test_discover_item_serde_roundtrip() {
        let item = DiscoverItem::new("x").with_location("http://example.com");
        let json_str = serde_json::to_string(&item).unwrap();
        let parsed: DiscoverItem = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.id, "x");
        assert_eq!(parsed.location.as_deref(), Some("http://example.com"));
    }
}
