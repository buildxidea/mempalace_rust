//! Adapter registry — thread-safe lookup for source adapters.
//!
//! The [`AdapterRegistry`] holds boxed trait objects keyed by adapter name.
//! Use [`AdapterRegistry::global`] for the process-wide singleton or create
//! isolated instances for testing.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::base::{SourceAdapter, SourceSchema};

/// Thread-safe registry of source adapters.
///
/// Wraps a `HashMap<String, Arc<dyn SourceAdapter>>` behind an `RwLock`
/// so concurrent readers never block and writes are serialized.
pub struct AdapterRegistry {
    adapters: RwLock<HashMap<String, Arc<dyn SourceAdapter>>>,
}

impl AdapterRegistry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self {
            adapters: RwLock::new(HashMap::new()),
        }
    }

    /// Register an adapter. Overwrites any existing adapter with the same name.
    pub fn register(&self, adapter: Arc<dyn SourceAdapter>) {
        let name = adapter.name().to_string();
        self.adapters
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name, adapter);
    }

    /// Get an adapter by name.
    pub fn get_adapter(&self, name: &str) -> Option<Arc<dyn SourceAdapter>> {
        self.adapters
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .cloned()
    }

    /// List all registered adapter names.
    pub fn list_adapters(&self) -> Vec<String> {
        self.adapters
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    /// Get schema information for all registered adapters.
    pub fn list_schemas(&self) -> Vec<SourceSchema> {
        self.adapters
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(|a| a.schema())
            .collect()
    }

    /// Remove an adapter by name. Returns `true` if it was present.
    pub fn unregister(&self, name: &str) -> bool {
        self.adapters
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name)
            .is_some()
    }

    /// Number of registered adapters.
    pub fn len(&self) -> usize {
        self.adapters
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Global singleton
// ---------------------------------------------------------------------------

static GLOBAL_REGISTRY: std::sync::OnceLock<AdapterRegistry> = std::sync::OnceLock::new();

/// Return a reference to the process-wide adapter registry.
pub fn global_registry() -> &'static AdapterRegistry {
    GLOBAL_REGISTRY.get_or_init(AdapterRegistry::new)
}

/// Convenience re-export: register an adapter into the global registry.
pub fn register_global(adapter: Arc<dyn SourceAdapter>) {
    global_registry().register(adapter);
}

/// Convenience re-export: look up an adapter in the global registry.
pub fn get_global(name: &str) -> Option<Arc<dyn SourceAdapter>> {
    global_registry().get_adapter(name)
}

// ---------------------------------------------------------------------------
// PalaceContext facade (lightweight integration point)
// ---------------------------------------------------------------------------

/// Facade that bundles adapter access with the global registry.
///
/// Intended to be held by the MCP server and CLI dispatchers so they can
/// route source operations without passing the registry around.
pub struct PalaceContext {
    pub registry: Arc<AdapterRegistry>,
}

impl PalaceContext {
    /// Create a context backed by the global registry.
    pub fn global() -> Self {
        Self {
            registry: Arc::new(AdapterRegistry::new()),
        }
    }

    /// Create a context with a custom registry (for testing).
    pub fn with_registry(registry: AdapterRegistry) -> Self {
        Self {
            registry: Arc::new(registry),
        }
    }

    /// Look up an adapter by name.
    pub fn adapter(&self, name: &str) -> Option<Arc<dyn SourceAdapter>> {
        self.registry.get_adapter(name)
    }

    /// List all registered adapter names.
    pub fn adapter_names(&self) -> Vec<String> {
        self.registry.list_adapters()
    }

    /// List schemas for all registered adapters.
    pub fn adapter_schemas(&self) -> Vec<SourceSchema> {
        self.registry.list_schemas()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::base::{
        DiscoverItem, SourceCapability, SourceError, SourceRecord, SourceResult,
    };
    use super::*;
    use async_trait::async_trait;

    /// Minimal no-op adapter for testing.
    struct DummyAdapter {
        name: String,
    }

    #[async_trait]
    impl SourceAdapter for DummyAdapter {
        fn name(&self) -> &str {
            &self.name
        }

        fn capabilities(&self) -> SourceCapability {
            SourceCapability::FULL
        }

        fn schema(&self) -> SourceSchema {
            SourceSchema {
                name: self.name.clone(),
                version: "0.1.0".into(),
                fields: HashMap::new(),
                required: vec![],
                description: Some("dummy adapter for testing".into()),
            }
        }

        async fn discover(&self) -> SourceResult<Vec<DiscoverItem>> {
            Ok(vec![DiscoverItem::new("item-1")])
        }

        async fn ingest(&self, items: &[DiscoverItem]) -> SourceResult<Vec<SourceRecord>> {
            Ok(items
                .iter()
                .map(|i| SourceRecord::new(&self.name, &i.id))
                .collect())
        }
    }

    #[test]
    fn test_registry_register_and_get() {
        let registry = AdapterRegistry::new();
        let adapter = Arc::new(DummyAdapter {
            name: "test".into(),
        });
        registry.register(adapter.clone());

        let got = registry.get_adapter("test").unwrap();
        assert_eq!(got.name(), "test");
    }

    #[test]
    fn test_registry_get_missing() {
        let registry = AdapterRegistry::new();
        assert!(registry.get_adapter("nonexistent").is_none());
    }

    #[test]
    fn test_registry_list_adapters() {
        let registry = AdapterRegistry::new();
        registry.register(Arc::new(DummyAdapter { name: "a".into() }));
        registry.register(Arc::new(DummyAdapter { name: "b".into() }));

        let mut names = registry.list_adapters();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn test_registry_len_and_empty() {
        let registry = AdapterRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);

        registry.register(Arc::new(DummyAdapter { name: "x".into() }));
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_registry_unregister() {
        let registry = AdapterRegistry::new();
        registry.register(Arc::new(DummyAdapter { name: "x".into() }));
        assert!(registry.unregister("x"));
        assert!(!registry.unregister("x")); // already removed
        assert!(registry.is_empty());
    }

    #[test]
    fn test_registry_overwrites_same_name() {
        let registry = AdapterRegistry::new();
        registry.register(Arc::new(DummyAdapter { name: "x".into() }));
        registry.register(Arc::new(DummyAdapter { name: "x".into() }));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_registry_list_schemas() {
        let registry = AdapterRegistry::new();
        registry.register(Arc::new(DummyAdapter {
            name: "alpha".into(),
        }));
        let schemas = registry.list_schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "alpha");
    }

    #[test]
    fn test_palace_context_adapter_lookup() {
        let mut reg = AdapterRegistry::new();
        reg.register(Arc::new(DummyAdapter {
            name: "test".into(),
        }));
        let ctx = PalaceContext::with_registry(reg);

        assert!(ctx.adapter("test").is_some());
        assert!(ctx.adapter("missing").is_none());
        assert_eq!(ctx.adapter_names(), vec!["test".to_string()]);
    }

    #[test]
    fn test_global_registry_singleton() {
        let reg = global_registry();
        let before = reg.len();
        // Register a dummy adapter in the global registry
        reg.register(Arc::new(DummyAdapter {
            name: "__global_test__".into(),
        }));
        assert!(reg.get_adapter("__global_test__").is_some());
        // Clean up
        reg.unregister("__global_test__");
        assert_eq!(reg.len(), before);
    }

    #[test]
    fn test_default_is_empty() {
        let registry = AdapterRegistry::default();
        assert!(registry.is_empty());
    }
}
