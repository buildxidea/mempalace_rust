//! Standalone in-memory KV store with TTL support.
//!
//! When MEMPALACE_URL is set but the server is unavailable, falls back to
//! local in-memory storage. Also used for context injection cache.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{de::DeserializeOwned, Serialize, Deserialize};
use tokio::sync::RwLock;

/// An entry with optional TTL.
/// Uses u64 for expiry timestamp for serde compatibility.
#[derive(Debug, Serialize, Deserialize)]
struct Entry<V> {
    value: V,
    /// Expiry time as Unix timestamp in seconds, or 0 if no expiry
    expires_at_secs: u64,
}

impl<V> Entry<V> {
    fn is_expired(&self) -> bool {
        if self.expires_at_secs == 0 {
            return false;
        }
        let expiry_instant = Instant::now() - Duration::from_secs(self.expires_at_secs);
        Instant::now() > expiry_instant
    }
}

/// Thread-safe in-memory KV store with JSON serialization and TTL support.
#[derive(Debug, Clone)]
pub struct InMemoryStore {
    store: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get a value by key, deserializing to the expected type.
    /// Returns None if key doesn't exist or is expired.
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let store = self.store.read().await;
        let entry_val = store.get(key)?;
        let entry: Entry<T> = match serde_json::from_value(entry_val.clone()) {
            Ok(e) => e,
            Err(_) => return None,
        };
        if entry.is_expired() {
            drop(store);
            self.delete(key).await;
            return None;
        }
        Some(entry.value)
    }

    /// Put a value with optional TTL (in seconds). None = no expiry.
    pub async fn put<T: Serialize>(&self, key: &str, value: &T, ttl_seconds: Option<u64>) -> bool {
        let expires_at_secs = ttl_seconds.unwrap_or(0);
        let entry = Entry {
            value,
            expires_at_secs,
        };
        let json_value = match serde_json::to_value(entry) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let mut store = self.store.write().await;
        store.insert(key.to_string(), json_value);
        true
    }

    /// Delete a key.
    pub async fn delete(&self, key: &str) -> bool {
        let mut store = self.store.write().await;
        store.remove(key).is_some()
    }

    /// Check if a key exists and is not expired.
    pub async fn contains(&self, key: &str) -> bool {
        let store = self.store.read().await;
        match store.get(key) {
            Some(entry_val) => {
                let entry: Entry<serde_json::Value> = match serde_json::from_value(entry_val.clone()) {
                    Ok(e) => e,
                    Err(_) => return false,
                };
                !entry.is_expired()
            }
            None => false,
        }
    }

    /// Clear all keys.
    pub async fn clear(&self) {
        let mut store = self.store.write().await;
        store.clear();
    }

    /// Get the number of non-expired entries.
    pub async fn len(&self) -> usize {
        let store = self.store.read().await;
        let mut count = 0;
        for entry_val in store.values() {
            let entry: Entry<serde_json::Value> = match serde_json::from_value(entry_val.clone()) {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.is_expired() {
                count += 1;
            }
        }
        count
    }

    /// Remove all expired entries (passive cleanup).
    pub async fn cleanup_expired(&self) {
        let mut store = self.store.write().await;
        store.retain(|_, v| {
            let entry: Entry<serde_json::Value> = match serde_json::from_value(v.clone()) {
                Ok(e) => e,
                Err(_) => return true, // keep if we can't parse
            };
            !entry.is_expired()
        });
    }
}

// ---------------------------------------------------------------------------
// Global instance for standalone mode
// ---------------------------------------------------------------------------

use std::sync::OnceLock;

static STANDALONE_STORE: OnceLock<InMemoryStore> = OnceLock::new();

/// Get the global standalone store instance.
pub fn get_standalone_store() ->&'static InMemoryStore {
    STANDALONE_STORE.get_or_init(InMemoryStore::new)
}

// ---------------------------------------------------------------------------
// Standalone mode detection
// ---------------------------------------------------------------------------

/// Returns true if MEMPALACE_URL is set but the server is unreachable.
/// This means we should use the in-memory fallback.
pub fn should_use_standalone_mode() -> bool {
    if let Some(url) = std::env::var("MEMPALACE_URL").ok() {
        if !url.is_empty() {
            // URL is set — check if server is reachable
            return !is_server_reachable(&url);
        }
    }
    false
}

/// Check if the MemPalace server at the given URL is reachable.
fn is_server_reachable(url:&str) -> bool {
    // Simple TCP check — try to connect to the host
    let url = url.trim_end_matches('/');
    let has_scheme = url.starts_with("http://") || url.starts_with("https://");
    let host_port = if has_scheme {
        url.replace("http://", "").replace("https://", "")
    } else {
        url.to_string()
    };

    let parts: Vec<&str> = host_port.split(':').collect();
    let host = parts.first().unwrap_or(&"localhost");
    let port: u16 = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(3030);

    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_secs(1),
    )
    .is_ok()
}

/// Execute a tool via HTTP fallback when main server is unavailable.
pub async fn fallback_tool_call(
    tool_name: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = std::env::var("MEMPALACE_URL").map_err(|_| "MEMPALACE_URL not set")?;
    let full_url = format!("{}/mcp", url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let body = serde_json::json!({
        "tool": tool_name,
        "args": args,
    });

    let response = client
        .post(&full_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if response.status().is_success() {
        response.json().await.map_err(|e| e.to_string())
    } else {
        Err(format!("HTTP error: {}", response.status()))
    }
}
