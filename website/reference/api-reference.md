# API Reference

Comprehensive parameter-level documentation for the public Rust API. MemPalace is a Rust workspace; the core types live in `crates/core` and are re-exported under the umbrella crate name `mempalace` for backward compatibility.

## `mempalace_core::searcher`

### `search_memories(query, palace_path, wing, room, n_results, embedding_model) → Result<SearchResponse, SearchError>`

Programmatic search returning a structured result. Used by the MCP server and CLI.

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `query` | `&str` | — | Search query text |
| `palace_path` | `&Path` | — | Path to palace directory |
| `wing` | `Option<&str>` | `None` | Filter by wing name |
| `room` | `Option<&str>` | `None` | Filter by room name |
| `n_results` | `usize` | `5` | Maximum number of results |
| `embedding_model` | `Option<&str>` | `None` | Override the configured embedding model |

**Returns `SearchResponse`:**

```rust
pub struct SearchResponse {
    pub query: String,
    pub filters: Filters,
    pub results: Vec<SearchHit>,
}

pub struct SearchHit {
    pub text: String,           // verbatim drawer content
    pub wing: String,           // wing name
    pub room: String,           // room name
    pub source_file: String,    // original file basename
    pub similarity: f32,        // 0.0 to 1.0
}
```

On error: `SearchError` with descriptive message.

### `search_memories_with_rerank(query, palace_path, wing, room, n_results, embedding_model, use_bm25, max_per_session, fusion_mode)`

Same as `search_memories` but with optional BM25 reranking and PPR fusion mode.

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `query` | `&str` | — | Search query |
| `palace_path` | `&Path` | — | Path to palace directory |
| `wing` | `Option<&str>` | `None` | Wing filter |
| `room` | `Option<&str>` | `None` | Room filter |
| `n_results` | `usize` | `5` | Maximum number of results |
| `embedding_model` | `Option<&str>` | `None` | Custom embedding model |
| `use_bm25` | `bool` | `false` | Enable BM25 reranking |
| `max_per_session` | `Option<usize>` | `None` | Cap results per session |
| `fusion_mode` | `Option<FusionMode>` | `None` | `vector`, `ppr`, or `hybrid` |

---

## `mempalace_core::layers`

### `struct MemoryStack`

Unified 4-layer interface.

```rust
use mempalace_core::layers::MemoryStack;

let mut stack = MemoryStack::new(
    Some("~/.mempalace/palace".into()),  // palace_path
    None,                                // identity_path
);
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `wake_up(wing)` | `async fn(&mut self, Option<&str>) -> String` | L0 + L1 context (~600–900 tokens) |
| `recall(wing, room, n_results)` | `fn(&self, Option<&str>, Option<&str>, usize) -> String` | L2 on-demand retrieval (sync) |
| `search(query, wing, room, n_results)` | `async fn(&self, &str, Option<&str>, Option<&str>, usize) -> String` | L3 deep search |
| `status()` | `fn(&self) -> LayerStatus` | All layer status info |

### `struct Layer0`

Identity layer (~50–100 tokens). Reads from `~/.mempalace/identity.txt`.

| Method | Returns | Description |
|--------|---------|-------------|
| `render()` | `String` | Identity text or default message |
| `token_estimate()` | `usize` | Approximate token count |

### `struct Layer1`

Essential story layer (~500–800 tokens). Auto-generated from top drawers.

| Constant | Value | Description |
|----------|-------|-------------|
| `MAX_DRAWERS` | `15` | Max moments in wake-up |
| `MAX_CHARS` | `3200` | Hard cap on L1 text |

---

## `mempalace_core::knowledge_graph`

### `struct KnowledgeGraph`

```rust
use mempalace_core::knowledge_graph::KnowledgeGraph;

let kg = KnowledgeGraph::open(Path::new("~/.mempalace/knowledge.db"))?;
```

Default path: `~/.mempalace/knowledge.db`

#### Write Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `add_entity(name, kind, properties)` | `(&str, &str, &serde_json::Value) -> Result<EntityId>` | Add or upsert an entity |
| `add_triple(subject, predicate, object, valid_from, confidence)` | `(&str, &str, &str, Option<&str>, Option<f32>) -> Result<TripleId>` | Add relationship triple |
| `invalidate(subject, predicate, object, ended)` | `(&str, &str, &str, &str) -> Result<()>` | Mark relationship as ended |

#### Query Methods

| Method | Signature | Returns |
|--------|-----------|---------|
| `query_entity(name, as_of, direction)` | `(&str, Option<&str>, &str) -> Result<EntityQueryResult>` | Facts touching the entity |
| `query_relationship(predicate, as_of)` | `(&str, Option<&str>) -> Result<Vec<Triple>>` | All triples with this predicate |
| `timeline(entity_name)` | `Option<&str> -> Result<Vec<Triple>>` | Chronological entity story |
| `stats()` | `() -> Result<KgStats>` | Graph overview counts |

**Direction values:** `"outgoing"` (entity→?), `"incoming"` (?→entity), `"both"` (default).

---

## `mempalace_core::palace_graph`

### `PalaceGraph::new()` + `build(palace_path)`

Build the palace graph from palace metadata. The graph is in-memory; cache helpers `cached_graph(palace_path)` and `invalidate_cache(palace_path)` are exposed in `palace_graph` for repeated traversals.

### `PalaceGraph::traverse(start_room, max_hops) -> TraverseOutcome`

BFS graph traversal from a room across wings.

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `start_room` | `&str` | — | Room slug to start from |
| `max_hops` | `usize` | `2` | Max connection depth |

### `PalaceGraph::find_tunnels(wing_a, wing_b) -> Vec<Tunnel>`

Find rooms spanning multiple wings.

### `PalaceGraph::stats(palace_path) -> Result<GraphStats>`

Aggregate counts: total rooms, tunnel rooms, top tunnels, rooms-per-wing.

### Free functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `create_tunnel` | `(source, target, label, …) -> Result<String>` | Create explicit cross-wing tunnel |
| `list_tunnels` | `(Option<&str>) -> Vec<ExplicitTunnel>` | List explicit tunnels |
| `delete_tunnel` | `(&str) -> bool` | Remove tunnel by ID |

---

## `mempalace_core::dialect`

### `fn compress(text, people_map) -> String`

Top-level helper that produces an AAAK-formatted summary for `text`, using `people_map` for entity codes.

### `struct Dialect`

```rust
use mempalace_core::dialect::Dialect;

let dialect = Dialect::new();
let dialect = Dialect::with_entities(entities)?;
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `compress(text)` | `(&str) -> String` | AAAK-formatted summary |
| `compress_with_metadata(text, metadata)` | `(&str, &serde_json::Value) -> String` | Compression with wing/room context |
| `encode_entity(name)` | `(&str) -> Option<String>` | 3-letter entity code |
| `compression_stats(original, compressed)` | `(&str, &str) -> CompressionStats` | Compression ratio stats |

---

## `mempalace_core::config`

### `struct Config`

Reads from `~/.mempalace/config.json` and environment variables.

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `palace_path` | `PathBuf` | `~/.mempalace/palace` | Palace storage path |
| `collection_name` | `String` | `mempalace_drawers` | Collection name |
| `search_strategy` | `String` | `contains` | `contains` / `naive` / `bm25` / `embedding` |
| `max_cache_size_mb` | `usize` | `128` | In-memory cache cap |
| `embedding_model` | `String` | `naive` | Active embedder |
| `embedder_identity_strict` | `bool` | `true` | Hard-fail on fingerprint mismatch |
| `people_map` | `HashMap<String, String>` | `{}` | Entity name → AAAK code |
| `languages` | `Vec<String>` | `[]` | Languages the AI should expect |

```rust
use mempalace_core::config::Config;

let config = Config::load()?;
```

Environment overrides:

| Variable | Effect |
|----------|--------|
| `MEMPALACE_PALACE_PATH` | Override palace path |
| `MEMPAL_PALACE_PATH` | Legacy alias |
| `MEMPALACE_HTTP_PORT` | REST API port for `mpr serve --http` |
| `MEMPALACE_MAX_CHUNKS_PER_FILE` | Default chunk cap |
