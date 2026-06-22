# Rust API

MemPalace is implemented in Rust and exposes a programmatic API through the `mempalace-core` crate (re-exported as `mempalace` for backwards compatibility). This page gives a high-level tour of the main entry points you'd use to integrate MemPalace into your own tool.

The full parameter-level documentation is in [API Reference](/reference/api-reference).

## Search

The primary way to query the palace programmatically. `search_memories` is the simple wrapper; `search_memories_with_rerank` adds BM25 reranking and PPR fusion.

```rust
use std::path::Path;
use mempalace_core::searcher::{search_memories, search_memories_with_rerank};

// Simple keyword / vector search
let response = search_memories(
    "why did we switch to GraphQL",   // query
    Path::new("~/.mempalace/palace"), // palace path
    Some("myapp"),                    // optional wing filter
    Some("architecture"),             // optional room filter
    5,                                // n_results
    None,                             // custom embedding model (None = config default)
)?;

// With BM25 rerank + hybrid fusion
let response = search_memories_with_rerank(
    "why did we switch to GraphQL",
    Path::new("~/.mempalace/palace"),
    Some("myapp"),
    Some("architecture"),
    5,
    None,       // embedding model
    true,       // enable BM25
    None,       // max_per_session
    None,       // fusion_mode (Some("hybrid"))
)?;

for hit in &response.results {
    println!("[{:.3}] {}/{}: {}", hit.similarity, hit.wing, hit.room, hit.text);
}
```

## Memory Stack

The 4-layer memory system with a unified interface.

```rust
use mempalace_core::layers::MemoryStack;

let mut stack = MemoryStack::new(
    Some("~/.mempalace/palace".into()),  // palace_path
    None,                                // identity_path (default: ~/.mempalace/identity.txt)
);

// Wake-up: L0 (identity) + L1 (essential story) — async
let context = stack.wake_up(Some("myapp")).await;   // ~600-900 tokens

// On-demand: L2 retrieval (sync)
let recall: String = stack.recall(Some("myapp"), Some("auth"), 10);

// Deep search: L3 semantic search (async)
let results: String = stack.search("pricing change", Some("myapp"), None, 5).await;

// Status (sync)
let status = stack.status();
```

## Knowledge Graph

Temporal entity-relationship graph built on SQLite.

```rust
use mempalace_core::knowledge_graph::KnowledgeGraph;

let kg = KnowledgeGraph::open("~/.mempalace/knowledge.db")?;

// Write
kg.add_triple("Kai", "works_on", "Orion", Some("2025-06-01"), Some(1.0))?;
kg.invalidate("Kai", "works_on", "Orion", "2026-03-01")?;

// Read
let facts = kg.query_entity("Kai", Some("2026-01-15"), "both")?;
let relationships = kg.query_relationship("works_on", None)?;
let timeline = kg.timeline(Some("Orion"))?;
let stats = kg.stats()?;
```

## Palace Graph

Room-based navigation graph built from metadata.

```rust
use mempalace_core::palace_graph::{build_graph, PalaceGraph};

let graph = PalaceGraph::new();
graph.build(&palace_path)?;
let path    = graph.traverse("auth-migration", 2);          // 2 hops
let tunnels = graph.find_tunnels(Some("wing_code"), Some("wing_team"));
let stats   = graph.stats(&palace_path)?;
```

## AAAK Dialect

Lossy compression for token density at scale.

```rust
use std::collections::HashMap;
use mempalace_core::dialect::{compress, Dialect};

// Top-level helper
let people = HashMap::from([("Alice".into(), "ALC".into())]);
let compressed = compress("We decided to use GraphQL because REST was too chatty.", &people);

// Or the full Dialect struct
let dialect = Dialect::new();
let stats   = dialect.compression_stats(text, &compressed);
```

## Configuration

```rust
use mempalace_core::config::Config;

let config = Config::load()?;
println!("{}", config.palace_path.display());  // ~/.mempalace/palace
println!("{}", config.collection_name);        // mempalace_drawers
println!("{}", config.search_strategy);        // contains
println!("{}", config.max_cache_size_mb);      // 128
```

For detailed parameter documentation, see [API Reference](/reference/api-reference).
