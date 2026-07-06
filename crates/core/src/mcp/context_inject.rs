//! Context injection for MemPalace.
//!
//! On `PreToolUse`, injects pinned slots + project profile + lessons +
//! session summaries into context within a ~4000-character budget.
//! Context is filtered by the file touched by the tool when possible.
//!
//! Guard: skips injection for SDK child contexts (`MEMPALACE_SDK=1`).
//! Config: enabled via `MEMPALACE_INJECT_CONTEXT` env var (default: false).
//! Performance: debounced DB reads with configurable TTL cache.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::mcp_server::AppState;
use crate::normalize::safe_truncate;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Character budget for injected context.
const INJECT_CHAR_BUDGET: usize = 4000;

/// Cache TTL for debounced DB reads (seconds).
const CACHE_TTL_SECS: u64 = 30;

/// Max number of pinned slots to include.
const MAX_PINNED_SLOTS: usize = 5;

/// Max number of lessons to include.
const MAX_LESSONS: usize = 5;

/// Max number of session summaries to include.
const MAX_SESSION_SUMMARIES: usize = 5;

/// Max characters per individual block (prevents one huge block eating budget).
const MAX_BLOCK_CHARS: usize = 800;

/// Environment flags that enable context injection.
const ENV_INJECT_FLAGS: &[&str] = &["MEMPALACE_INJECT_CONTEXT", "MEMPALACE_INJECT"];

/// Env var that marks an SDK child context (injection is skipped).
const ENV_SDK_CHILD: &str = "MEMPALACE_SDK";

// ---------------------------------------------------------------------------
// Cache — debounced DB reads with TTL expiry
// ---------------------------------------------------------------------------

/// Cached value from a single DB table read.
#[derive(Debug, Clone)]
struct CacheEntry {
    value: String,
    cached_at: Instant,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > Duration::from_secs(CACHE_TTL_SECS)
    }
}

/// Debounced cache for context DB reads.
///
/// Each category (slots, profile, lessons, sessions) is cached
/// independently with its own TTL so a hot tool path avoids
/// repeated SQLite queries.
struct ContextCache {
    pinned_slots: Mutex<Option<CacheEntry>>,
    project_profile: Mutex<Option<CacheEntry>>,
    lessons: Mutex<Option<CacheEntry>>,
    session_summaries: Mutex<Option<CacheEntry>>,
}

impl ContextCache {
    fn new() -> Self {
        Self {
            pinned_slots: Mutex::new(None),
            project_profile: Mutex::new(None),
            lessons: Mutex::new(None),
            session_summaries: Mutex::new(None),
        }
    }

    /// Get a cached value if still valid, or compute and store it.
    fn get_or_compute<F>(&self, slot: &Mutex<Option<CacheEntry>>, compute: F) -> String
    where
        F: FnOnce() -> String,
    {
        // Fast path: check cache under read lock
        {
            let guard = slot.lock().expect("context cache lock poisoned");
            if let Some(entry) = guard.as_ref() {
                if !entry.is_expired() {
                    return entry.value.clone();
                }
            }
        }
        // Slow path: compute and store
        let value = compute();
        let entry = CacheEntry {
            value: value.clone(),
            cached_at: Instant::now(),
        };
        let mut guard = slot.lock().expect("context cache lock poisoned");
        *guard = Some(entry);
        value
    }

    /// Clear all cached entries (e.g. after a write operation).
    fn invalidate(&self) {
        *self.pinned_slots.lock().expect("cache lock") = None;
        *self.project_profile.lock().expect("cache lock") = None;
        *self.lessons.lock().expect("cache lock") = None;
        *self.session_summaries.lock().expect("cache lock") = None;
    }
}

// ---------------------------------------------------------------------------
// Environment helpers
// ---------------------------------------------------------------------------

/// Returns true if context injection is enabled via env vars.
pub fn is_context_injection_enabled() -> bool {
    for flag in ENV_INJECT_FLAGS {
        if let Ok(val) = std::env::var(flag) {
            if val == "1" || val.to_lowercase() == "true" {
                return true;
            }
        }
    }
    false
}

/// Returns true if we are inside an SDK child context where
/// injection should be skipped to avoid context loops.
fn is_sdk_child_context() -> bool {
    matches!(
        std::env::var(ENV_SDK_CHILD).as_deref(),
        Ok("1") | Ok("true")
    )
}

// ---------------------------------------------------------------------------
// ContextInjector
// ---------------------------------------------------------------------------

/// Context injector that implements [`crate::EventCapture`].
///
/// Caches DB reads to avoid hammering SQLite on rapid tool calls,
/// and formats context within a fixed character budget.
pub struct ContextInjector {
    state: Arc<AppState>,
    cache: ContextCache,
}

impl ContextInjector {
    /// Create a new injector wrapping the given app state.
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            cache: ContextCache::new(),
        }
    }

    /// Build context for a tool invocation, filtering by touched file.
    ///
    /// Returns a markdown-formatted context string within the character
    /// budget, or an empty string if injection is disabled.
    pub fn build_context(&self, tool_name: &str, tool_params: &str) -> String {
        // Injection is enabled if env var is set OR config field is true.
        let enabled = is_context_injection_enabled()
            || self.state.config.inject_context_enabled.unwrap_or(false);
        if !enabled || is_sdk_child_context() {
            return String::new();
        }

        let mut output = String::with_capacity(256);
        let mut used = 0usize;

        // 1. Pinned slots (highest priority — always injected first)
        let pinned = self.get_pinned_slots();
        if !pinned.is_empty() {
            let block = format_section("Pinned Slots", &pinned);
            let cost = block.len();
            if used + cost <= INJECT_CHAR_BUDGET {
                output.push_str(&block);
                used += cost;
            }
        }

        // 2. Project profile
        let profile = self.get_project_profile();
        if !profile.is_empty() {
            let block = format_section("Project Profile", &profile);
            let cost = block.len();
            if used + cost <= INJECT_CHAR_BUDGET {
                output.push_str(&block);
                used += cost;
            }
        }

        // 3. Lessons (filtered by file if available)
        let files = extract_file_paths(tool_params);
        let lessons = if let Some(first_file) = files.first() {
            self.get_lessons_for_file(first_file)
        } else {
            self.get_lessons()
        };
        if !lessons.is_empty() {
            let block = format_section("Lessons", &lessons);
            let cost = block.len();
            if used + cost <= INJECT_CHAR_BUDGET {
                output.push_str(&block);
                used += cost;
            }
        }

        // 4. Session summaries (filtered by file if available)
        let summaries = if let Some(first_file) = files.first() {
            self.get_summaries_for_file(first_file)
        } else {
            self.get_session_summaries()
        };
        if !summaries.is_empty() {
            let block = format_section("Recent Sessions", &summaries);
            let cost = block.len();
            if used + cost <= INJECT_CHAR_BUDGET {
                output.push_str(&block);
                used += cost;
            }
        }

        // 5. File-related search context (if a file was detected)
        if let Some(first_file) = files.first() {
            let remaining = INJECT_CHAR_BUDGET.saturating_sub(used);
            if remaining > 100 {
                let file_ctx = self.get_file_context(first_file, remaining);
                if !file_ctx.is_empty() {
                    output.push_str(&file_ctx);
                }
            }
        }

        if output.is_empty() {
            String::new()
        } else {
            format!("## MemPalace Context\n{}", output)
        }
    }

    /// Invalidate all cached data (call after mutations).
    pub fn invalidate(&self) {
        self.cache.invalidate();
    }

    // -- Cached DB reads --------------------------------------------------

    /// Get pinned slots from DB (cached).
    fn get_pinned_slots(&self) -> String {
        self.cache.get_or_compute(&self.cache.pinned_slots, || {
            let db = match crate::palace_db::PalaceDb::open(&self.state.palace_path) {
                Ok(db) => db,
                Err(e) => {
                    eprintln!("[mempalace] context inject: failed to open db: {}", e);
                    return String::new();
                }
            };
            let slots = match db.slot_list(None) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[mempalace] context inject: slot_list failed: {}", e);
                    return String::new();
                }
            };
            let pinned: Vec<_> = slots.into_iter().filter(|s| s.pinned).collect();
            if pinned.is_empty() {
                return String::new();
            }
            let mut out = String::new();
            for slot in pinned.into_iter().take(MAX_PINNED_SLOTS) {
                let preview = truncate_block(&slot.content);
                out.push_str(&format!("- **{}**: {}\n", slot.label, preview));
            }
            out
        })
    }

    /// Get project profile summary (cached).
    fn get_project_profile(&self) -> String {
        self.cache.get_or_compute(&self.cache.project_profile, || {
            let db = match crate::palace_db::PalaceDb::open(&self.state.palace_path) {
                Ok(db) => db,
                Err(e) => {
                    eprintln!("[mempalace] context inject: failed to open db: {}", e);
                    return String::new();
                }
            };
            let all_drawers = db.get_all(None, None, 500);
            if all_drawers.is_empty() {
                return String::new();
            }

            let total = all_drawers.iter().map(|qr| qr.ids.len()).sum::<usize>();
            if total == 0 {
                return String::new();
            }

            // Top concepts
            let mut concept_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for qr in &all_drawers {
                if let Some(meta) = qr.metadatas.first() {
                    if let Some(c) = meta.get("concepts").and_then(|v| v.as_str()) {
                        for concept in c.split(',') {
                            let c = concept.trim();
                            if !c.is_empty() {
                                *concept_counts.entry(c.to_string()).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
            let mut top_concepts: Vec<_> = concept_counts.into_iter().collect();
            top_concepts.sort_by(|a, b| b.1.cmp(&a.1));
            let concepts_str: String = top_concepts
                .iter()
                .take(8)
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>()
                .join(", ");

            // Top rooms
            let mut room_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for qr in &all_drawers {
                if let Some(meta) = qr.metadatas.first() {
                    if let Some(r) = meta.get("room").and_then(|v| v.as_str()) {
                        *room_counts.entry(r.to_string()).or_insert(0) += 1;
                    }
                }
            }
            let mut top_rooms: Vec<_> = room_counts.into_iter().collect();
            top_rooms.sort_by(|a, b| b.1.cmp(&a.1));
            let rooms_str: String = top_rooms
                .iter()
                .take(5)
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>()
                .join(", ");

            format!(
                "Memories: {} total\nConcepts: {}\nRooms: {}\n",
                total,
                if concepts_str.is_empty() {
                    "n/a"
                } else {
                    &concepts_str
                },
                if rooms_str.is_empty() {
                    "n/a"
                } else {
                    &rooms_str
                },
            )
        })
    }

    /// Get recent lessons (cached).
    fn get_lessons(&self) -> String {
        self.cache.get_or_compute(&self.cache.lessons, || {
            let db = match crate::palace_db::PalaceDb::open(&self.state.palace_path) {
                Ok(db) => db,
                Err(e) => {
                    eprintln!("[mempalace] context inject: failed to open db: {}", e);
                    return String::new();
                }
            };
            let lessons = match db.lesson_list(None, None) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[mempalace] context inject: lesson_list failed: {}", e);
                    return String::new();
                }
            };
            let mut out = String::new();
            for lesson in lessons.iter().take(MAX_LESSONS) {
                let preview = truncate_block(&lesson.content);
                out.push_str(&format!("- {}\n", preview));
            }
            out
        })
    }

    /// Get lessons filtered by file name.
    fn get_lessons_for_file(&self, file: &str) -> String {
        let all = self.get_lessons();
        let needle = file_name_stem(file);
        if needle.is_empty() || all.is_empty() {
            return all; // fall back to unfiltered
        }
        let filtered: String = all
            .lines()
            .filter(|line| line.to_lowercase().contains(&needle.to_lowercase()))
            .collect::<Vec<_>>()
            .join("\n");
        if filtered.is_empty() {
            all
        } else {
            format!("{}\n", filtered)
        }
    }

    /// Get recent session summaries (cached).
    fn get_session_summaries(&self) -> String {
        self.cache
            .get_or_compute(&self.cache.session_summaries, || {
                let store = match crate::session::SessionStore::open(
                    &self.state.palace_path.join("sessions"),
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("[mempalace] context inject: session store failed: {}", e);
                        return String::new();
                    }
                };
                let sessions = match store.list_sessions(None) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("[mempalace] context inject: list_sessions failed: {}", e);
                        return String::new();
                    }
                };
                let mut out = String::new();
                for session in sessions.iter().take(MAX_SESSION_SUMMARIES) {
                    if let Some(ref summary) = session.summary {
                        let preview = truncate_block(summary);
                        let started = session.started_at.format("%Y-%m-%d %H:%M").to_string();
                        out.push_str(&format!("- [{}] {} {}\n", started, session.id, preview));
                    }
                }
                out
            })
    }

    /// Get summaries filtered by file name.
    fn get_summaries_for_file(&self, file: &str) -> String {
        let all = self.get_session_summaries();
        let needle = file_name_stem(file);
        if needle.is_empty() || all.is_empty() {
            return all;
        }
        let filtered: String = all
            .lines()
            .filter(|line| line.to_lowercase().contains(&needle.to_lowercase()))
            .collect::<Vec<_>>()
            .join("\n");
        if filtered.is_empty() {
            all
        } else {
            format!("{}\n", filtered)
        }
    }

    /// Search for file-related memories (not cached — freshness matters).
    fn get_file_context(&self, file: &str, budget: usize) -> String {
        let file_stem = file_name_stem(file);
        if file_stem.is_empty() {
            return String::new();
        }
        let query = format!("file {}", file_stem);
        let result = crate::searcher::search_memories_with_rerank(
            &query,
            &self.state.palace_path,
            None,
            None,
            3,
            None,
            false,
            Some(2),
            None,
            false,
        );
        match result {
            Ok(response) => {
                if response.results.is_empty() {
                    return String::new();
                }
                let mut out = String::from(&format!("\n### Related: `{}`\n", file_name_stem(file)));
                for result in response.results.iter().take(3) {
                    let preview = truncate_block(&result.text);
                    if !preview.is_empty() {
                        out.push_str(&format!("- {}\n", preview.replace('\n', " ")));
                    }
                }
                if out.len() > budget {
                    out.truncate(budget);
                    out.push_str("...");
                }
                out
            }
            Err(e) => {
                eprintln!("[mempalace] context inject: search failed: {}", e);
                String::new()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// EventCapture implementation
// ---------------------------------------------------------------------------

impl crate::EventCapture for ContextInjector {
    fn on_session_start(&self, _event: crate::SessionStartEvent) {
        // Invalidate cache on new session so stale data from prior session is cleared.
        self.invalidate();
    }

    fn on_user_prompt_submit(&self, _event: crate::UserPromptEvent) {
        // No-op — context is injected on PreToolUse.
    }

    fn on_pre_tool_use(&self, event: crate::PreToolEvent) {
        let ctx = self.build_context(&event.tool_name, &event.params_preview);
        if !ctx.is_empty() {
            // Print context for the agent to consume.
            // In a real integration this would be injected into the tool
            // call metadata; here we emit to stderr for the agent loop.
            eprintln!("[mempalace-context]\n{}", ctx);
        }
    }

    fn on_post_tool_use(&self, _event: crate::PostToolEvent) {
        // No-op — post-tool context is not injected.
    }

    fn on_memory_write(&self, _event: crate::MemoryWriteEvent) {
        // Invalidate cache when memories are written so the next
        // PreToolUse reads fresh data.
        self.invalidate();
    }

    fn on_stop(&self, _event: crate::StopEvent) {
        // No-op.
    }

    fn on_embedder_ready(&self, _event: crate::EmbedderEvent) {
        // No-op.
    }
}

// ---------------------------------------------------------------------------
// Public MCP tool handler
// ---------------------------------------------------------------------------

/// Build context on demand via MCP tool call.
///
/// This allows callers (CLI, SDK) to request context explicitly.
pub fn build_context_for_tool(
    state: &AppState,
    tool_name: &str,
    tool_params: &str,
) -> Result<String, String> {
    if !is_context_injection_enabled() {
        return Ok(String::new());
    }
    let injector = ContextInjector::new(std::sync::Arc::new(
        AppState::new(state.config.clone(), state.read_only).map_err(|e| e.to_string())?,
    ));
    Ok(injector.build_context(tool_name, tool_params))
}

// ---------------------------------------------------------------------------
// Backward-compatible public API
// ---------------------------------------------------------------------------

/// Inject context after session_start by calling hybrid_search and
/// formatting results as markdown.
///
/// Returns the context string that should be prepended to the prompt.
pub fn inject_session_context(
    session_id: &str,
    palace_path: &std::path::Path,
    project: Option<&str>,
) -> String {
    if !is_context_injection_enabled() || is_sdk_child_context() {
        return String::new();
    }

    let query = project
        .map(|p| format!("recent session context project {}", p))
        .unwrap_or_else(|| format!("recent session {} context", session_id));

    let result = crate::searcher::search_memories_with_rerank(
        &query,
        palace_path,
        None,    // wing filter - none
        None,    // room filter - none
        5,       // limit
        None,    // embedding model
        false,   // no BM25 reranking
        Some(3), // max_per_session
        None,    // fusion mode
        false,   // no query expansion
    );

    match result {
        Ok(response) => format_context_as_markdown(&response),
        Err(e) => {
            eprintln!("[mempalace] context injection failed: {}", e);
            String::new()
        }
    }
}

/// Format search results as markdown context string.
fn format_context_as_markdown(response: &crate::searcher::SearchResponse) -> String {
    if response.results.is_empty() {
        return String::new();
    }

    let mut output = String::from("## Recent Context\n");
    for result in response.results.iter() {
        let preview = if result.text.len() > 200 {
            format!("{}...", safe_truncate(&result.text, 200))
        } else {
            result.text.clone()
        };
        output.push_str(&format!("- {}\n", preview.replace('\n', " ")));
    }
    output
}

/// Inject context using the MCP app state (async version for use from async dispatch).
pub async fn inject_context_async(
    state: &std::sync::Arc<AppState>,
    session_id: &str,
    project: Option<&str>,
) -> Result<String, String> {
    if !is_context_injection_enabled() || is_sdk_child_context() {
        return Ok(String::new());
    }

    let query = project
        .map(|p| format!("recent session context project {}", p))
        .unwrap_or_else(|| format!("recent session {} context", session_id));

    let result = crate::searcher::search_memories_with_rerank(
        &query,
        &state.palace_path,
        None,
        None,
        5,
        None,
        false,
        Some(3),
        None,
        false,
    )
    .map_err(|e| e.to_string())?;

    Ok(format_context_as_markdown(&result))
}

// ---------------------------------------------------------------------------
// Internal formatting helpers
// ---------------------------------------------------------------------------

/// Format a section with a header and bulleted items.
fn format_section(header: &str, items: &str) -> String {
    if items.is_empty() {
        return String::new();
    }
    format!("### {}\n{}", header, items)
}

/// Truncate a block to MAX_BLOCK_CHARS.
fn truncate_block(text: &str) -> String {
    if text.len() > MAX_BLOCK_CHARS {
        format!("{}...", safe_truncate(text, MAX_BLOCK_CHARS))
    } else {
        text.to_string()
    }
}

/// Extract file paths from tool params JSON.
///
/// Supports common MCP tool parameter patterns:
/// - `"path"`: "some/file.rs"
/// - `"file_path"`: "some/file.rs"
/// - `"file"`: "some/file.rs"
/// - `"target"`: { "type": "path", "path": "some/file.rs" }
/// - `"command"`: "cat some/file.rs"
fn extract_file_paths(params_json: &str) -> Vec<String> {
    let params: serde_json::Value = match serde_json::from_str(params_json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut paths = Vec::new();
    let file_keys = ["path", "file_path", "file", "target"];

    if let Some(obj) = params.as_object() {
        for key in &file_keys {
            if let Some(val) = obj.get(*key) {
                if let Some(s) = val.as_str() {
                    if looks_like_path(s) {
                        paths.push(s.to_string());
                    }
                } else if let Some(nested) = val.as_object() {
                    // Handle target: { type: "path", path: "..." }
                    if let Some(p) = nested.get("path").and_then(|v| v.as_str()) {
                        if looks_like_path(p) {
                            paths.push(p.to_string());
                        }
                    }
                }
            }
        }

        // Extract paths from command strings (e.g. "cat foo/bar.rs")
        if let Some(cmd) = obj.get("command").and_then(|v| v.as_str()) {
            paths.extend(extract_paths_from_command(cmd));
        }
    }

    // Deduplicate
    paths.sort();
    paths.dedup();
    paths
}

/// Check if a string looks like a file path (has an extension or contains `/`).
fn looks_like_path(s: &str) -> bool {
    s.contains('.') || s.contains('/') || s.contains('\\')
}

/// Extract potential file paths from a command string.
fn extract_paths_from_command(cmd: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for token in cmd.split_whitespace() {
        if looks_like_path(token) && !token.starts_with('-') {
            // Strip common command prefixes
            let cleaned = token.trim_start_matches('/').trim_start_matches("./");
            if cleaned.contains('/') && cleaned.len() > 3 {
                paths.push(cleaned.to_string());
            }
        }
    }
    paths
}

/// Get the file name stem (without path and extension) for fuzzy matching.
fn file_name_stem(path: &str) -> String {
    let name = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    name.to_string()
}

// ---------------------------------------------------------------------------
// Global injector instance
// ---------------------------------------------------------------------------

static GLOBAL_INJECTOR: OnceLock<ContextInjector> = OnceLock::new();

/// Get or initialize the global context injector.
pub fn init_global_injector(state: Arc<AppState>) -> &'static ContextInjector {
    GLOBAL_INJECTOR.get_or_init(|| ContextInjector::new(state))
}

/// Get a reference to the global injector if initialized.
pub fn global_injector() -> Option<&'static ContextInjector> {
    GLOBAL_INJECTOR.get()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_is_sdk_child_context_default() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::remove_var(ENV_SDK_CHILD);
        assert!(!is_sdk_child_context());
    }

    #[test]
    fn test_is_sdk_child_context_enabled() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var(ENV_SDK_CHILD, "1");
        assert!(is_sdk_child_context());
        std::env::remove_var(ENV_SDK_CHILD);
    }

    #[test]
    fn test_is_sdk_child_context_true_string() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var(ENV_SDK_CHILD, "true");
        assert!(is_sdk_child_context());
        std::env::remove_var(ENV_SDK_CHILD);
    }

    #[test]
    fn test_extract_file_paths_empty_json() {
        assert!(extract_file_paths("{}").is_empty());
    }

    #[test]
    fn test_extract_file_paths_invalid_json() {
        assert!(extract_file_paths("not json").is_empty());
    }

    #[test]
    fn test_extract_file_paths_path_field() {
        let json = r#"{"path": "src/main.rs"}"#;
        let paths = extract_file_paths(json);
        assert_eq!(paths, vec!["src/main.rs".to_string()]);
    }

    #[test]
    fn test_extract_file_paths_file_path_field() {
        let json = r#"{"file_path": "crates/core/src/lib.rs"}"#;
        let paths = extract_file_paths(json);
        assert_eq!(paths, vec!["crates/core/src/lib.rs".to_string()]);
    }

    #[test]
    fn test_extract_file_paths_file_field() {
        let json = r#"{"file": "README.md"}"#;
        let paths = extract_file_paths(json);
        assert_eq!(paths, vec!["README.md".to_string()]);
    }

    #[test]
    fn test_extract_file_paths_target_object() {
        let json = r#"{"target": {"type": "path", "path": "src/lib.rs"}}"#;
        let paths = extract_file_paths(json);
        assert_eq!(paths, vec!["src/lib.rs".to_string()]);
    }

    #[test]
    fn test_extract_file_paths_command_string() {
        let json = r#"{"command": "cat src/main.rs"}"#;
        let paths = extract_file_paths(json);
        assert_eq!(paths, vec!["src/main.rs".to_string()]);
    }

    #[test]
    fn test_extract_file_paths_deduplication() {
        let json = r#"{"path": "src/lib.rs", "file": "src/lib.rs"}"#;
        let paths = extract_file_paths(json);
        assert_eq!(paths, vec!["src/lib.rs".to_string()]);
    }

    #[test]
    fn test_extract_file_paths_multiple_files() {
        let json = r#"{"path": "src/a.rs", "file_path": "src/b.rs"}"#;
        let paths = extract_file_paths(json);
        assert_eq!(paths, vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
    }

    #[test]
    fn test_extract_file_paths_ignores_non_paths() {
        let json = r#"{"path": "hello world", "name": "test"}"#;
        let paths = extract_file_paths(json);
        assert!(paths.is_empty());
    }

    #[test]
    fn test_file_name_stem_with_path() {
        assert_eq!(file_name_stem("src/main.rs"), "main");
    }

    #[test]
    fn test_file_name_stem_without_path() {
        assert_eq!(file_name_stem("lib.rs"), "lib");
    }

    #[test]
    fn test_file_name_stem_empty() {
        assert_eq!(file_name_stem(""), "");
    }

    #[test]
    fn test_format_section_empty() {
        assert_eq!(format_section("Header", ""), "");
    }

    #[test]
    fn test_format_section_with_content() {
        let result = format_section("Lessons", "- lesson 1\n");
        assert_eq!(result, "### Lessons\n- lesson 1\n");
    }

    #[test]
    fn test_truncate_block_short() {
        let text = "short text";
        assert_eq!(truncate_block(text), "short text");
    }

    #[test]
    fn test_truncate_block_long() {
        let text = &"x".repeat(1000);
        let result = truncate_block(text);
        assert!(result.len() <= MAX_BLOCK_CHARS + 4); // +4 for "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_context_cache_new() {
        let cache = ContextCache::new();
        assert!(cache.pinned_slots.lock().unwrap().is_none());
        assert!(cache.project_profile.lock().unwrap().is_none());
        assert!(cache.lessons.lock().unwrap().is_none());
        assert!(cache.session_summaries.lock().unwrap().is_none());
    }

    #[test]
    fn test_context_cache_get_or_compute() {
        let cache = ContextCache::new();
        let call_count = std::sync::atomic::AtomicUsize::new(0);

        let result = cache.get_or_compute(&cache.lessons, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            "cached value".to_string()
        });
        assert_eq!(result, "cached value");
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Second call should use cache
        let result2 = cache.get_or_compute(&cache.lessons, || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            "new value".to_string()
        });
        assert_eq!(result2, "cached value");
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn test_context_cache_invalidate() {
        let cache = ContextCache::new();

        // Populate cache
        let _ = cache.get_or_compute(&cache.pinned_slots, || "pinned".to_string());
        let _ = cache.get_or_compute(&cache.lessons, || "lessons".to_string());
        assert!(cache.pinned_slots.lock().unwrap().is_some());

        // Invalidate
        cache.invalidate();
        assert!(cache.pinned_slots.lock().unwrap().is_none());
        assert!(cache.lessons.lock().unwrap().is_none());
    }

    #[test]
    fn test_build_context_disabled() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("MEMPALACE_INJECT_CONTEXT");
        std::env::remove_var("MEMPALACE_INJECT");

        // Without a real palace, build_context should return empty when disabled.
        // We can't easily create a real AppState in tests, so test the guard.
        assert!(!is_context_injection_enabled());
    }

    #[test]
    fn test_build_context_sdk_child_skip() {
        let _guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("MEMPALACE_INJECT_CONTEXT", "1");
        std::env::set_var(ENV_SDK_CHILD, "1");
        assert!(is_sdk_child_context());
        std::env::remove_var("MEMPALACE_INJECT_CONTEXT");
        std::env::remove_var(ENV_SDK_CHILD);
    }

    #[test]
    fn test_looks_like_path() {
        assert!(looks_like_path("src/main.rs"));
        assert!(looks_like_path("Cargo.toml"));
        assert!(looks_like_path("a/b/c"));
        assert!(!looks_like_path("hello"));
        assert!(!looks_like_path("12345"));
    }

    #[test]
    fn test_extract_paths_from_command_empty() {
        assert!(extract_paths_from_command("").is_empty());
    }

    #[test]
    fn test_extract_paths_from_command_with_paths() {
        let paths = extract_paths_from_command("cat src/main.rs > out.txt");
        assert!(paths.contains(&"src/main.rs".to_string()));
    }

    #[test]
    fn test_extract_paths_from_command_ignores_flags() {
        // "-r" and "pattern" are not paths, but "src/" is a valid path fragment
        let paths = extract_paths_from_command("grep -r pattern src/");
        assert_eq!(paths, vec!["src/".to_string()]);
    }

    #[test]
    fn test_format_context_as_markdown_empty() {
        use crate::searcher::{SearchFilters, SearchResponse};
        let response = SearchResponse {
            query: "test".to_string(),
            filters: SearchFilters {
                wing: None,
                room: None,
            },
            results: vec![],
        };
        assert_eq!(format_context_as_markdown(&response), "");
    }
}
