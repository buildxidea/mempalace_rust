//! Source adapter subsystem — RFC 002.
//!
//! Provides the [`base`] trait contract and [`registry`] for
//! discovering, ingesting, and transforming data from arbitrary
//! external sources.
//!
//! # Architecture
//!
//! ```text
//!  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
//!  │ SourceAdapter │────▶│ Transform    │────▶│ SourceRecord │
//!  │  (trait)      │     │  Pipeline    │     │  (typed)     │
//!  └──────────────┘     └──────────────┘     └──────────────┘
//!         ▲
//!         │ register
//!  ┌──────────────┐
//!  │ Adapter      │
//!  │ Registry     │
//!  └──────────────┘
//! ```
//!
//! No first-party adapters ship in this module — only the contract.

pub mod base;
pub mod registry;

// Re-exports for convenience
pub use base::{
    DiscoverItem, SourceAdapter, SourceCapability, SourceError, SourceRecord, SourceResult,
    SourceSchema, TransformPipeline, TransformStep,
};
pub use registry::{get_global, global_registry, register_global, AdapterRegistry, PalaceContext};

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashMap;

    /// Integration test: register adapter, discover, ingest, transform.
    struct IntegrationAdapter;

    #[async_trait]
    impl SourceAdapter for IntegrationAdapter {
        fn name(&self) -> &str {
            "integration_test"
        }

        fn capabilities(&self) -> SourceCapability {
            SourceCapability::FULL
        }

        fn schema(&self) -> SourceSchema {
            SourceSchema {
                name: "integration_test".into(),
                version: "1.0.0".into(),
                fields: HashMap::from([("title".into(), "string".into())]),
                required: vec!["title".into()],
                description: Some("integration test adapter".into()),
            }
        }

        async fn discover(&self) -> SourceResult<Vec<DiscoverItem>> {
            Ok(vec![
                DiscoverItem::new("item-1").with_location("/test/file1.txt"),
                DiscoverItem::new("item-2").with_location("/test/file2.txt"),
            ])
        }

        async fn ingest(&self, items: &[DiscoverItem]) -> SourceResult<Vec<SourceRecord>> {
            Ok(items
                .iter()
                .map(|i| {
                    SourceRecord::new("integration_test", &i.id)
                        .with_payload("title", serde_json::json!(format!("Title for {}", i.id)))
                })
                .collect())
        }

        async fn transform(&self, records: Vec<SourceRecord>) -> SourceResult<Vec<SourceRecord>> {
            Ok(records
                .into_iter()
                .map(|r| r.with_tags(vec!["transformed".into()]))
                .collect())
        }
    }

    #[tokio::test]
    async fn test_full_adapter_lifecycle() {
        let registry = AdapterRegistry::new();
        let adapter = std::sync::Arc::new(IntegrationAdapter);
        registry.register(adapter.clone());

        // Verify registration
        let retrieved = registry.get_adapter("integration_test").unwrap();
        assert_eq!(retrieved.name(), "integration_test");

        // Schema
        let schema = retrieved.schema();
        assert_eq!(schema.name, "integration_test");
        assert!(schema
            .validate(&SourceRecord::new("a", "1").with_payload("title", serde_json::json!("x")))
            .is_ok());

        // Discover
        let items = retrieved.discover().await.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "item-1");

        // Ingest
        let records = retrieved.ingest(&items).await.unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].payload.get("title").unwrap(), "Title for item-1");

        // Transform
        let transformed = retrieved.transform(records).await.unwrap();
        assert_eq!(transformed[0].tags, vec!["transformed"]);
    }

    #[test]
    fn test_palace_context_integration() {
        let mut reg = AdapterRegistry::new();
        reg.register(std::sync::Arc::new(IntegrationAdapter));
        let ctx = PalaceContext::with_registry(reg);

        let schemas = ctx.adapter_schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "integration_test");

        let names = ctx.adapter_names();
        assert_eq!(names, vec!["integration_test"]);
    }
}
