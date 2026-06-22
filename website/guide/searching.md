# Searching Memories

MemPalace supports four search strategies, picked at init time or per-call. Whichever you pick, results are **verbatim text** — the exact words from your source files, never summaries.

## Search Strategies

| Strategy | How it works | Cost |
|----------|--------------|------|
| `contains` *(default)* | Exact-word substring match | 0MB, instant |
| `naive` | Jaccard-style token overlap | 0MB, instant |
| `bm25` | BM25 ranking via the `bm25` crate | 0MB, fast |
| `embedding` | Vector similarity via ONNX `MiniLM` (384 dim) | ~90MB, best recall |

Set the default at init:

```bash
mpr init ~/projects/myapp --search-strategy fts5
```

…or override per-call:

```bash
mpr search "auth decision" --strategy bm25 --bm25
mpr search "auth decision" --strategy embedding
```

## CLI Search

```bash
# Search everything
mpr search "why did we switch to GraphQL"

# Filter by wing (project)
mpr search "database decision" --wing myapp

# Filter by room (topic)
mpr search "auth decisions" --room auth-migration

# Filter by both
mpr search "pricing" --wing driftwood --room costs

# More results
mpr search "deploy process" --results 10

# BM25 rerank
mpr search "auth" --bm25

# Hybrid fusion mode (vector + PPR)
mpr search "auth" --strategy embedding --fusion-mode hybrid

# JSON output for piping into other tools
mpr search "auth" --json | jq '.results[0].text'
```

## How Search Works

1. Your query is embedded (when the strategy is `embedding`) using the ONNX MiniLM model.
2. For `contains` / `naive`, the query is matched directly against drawer text.
3. Optional wing/room filters narrow the search scope — standard metadata filtering.
4. With `--bm25`, results are reranked using BM25 relevance scoring.
5. Results are returned with similarity / relevance scores and source metadata.

### Why Scoping Matters

Wing/room filtering is useful when a single palace contains many unrelated projects or people. Narrowing the search to a specific wing (or wing + room) means the store only scores candidates inside that scope, which keeps retrieval predictable as the palace grows.

This is a metadata-filter feature, not a novel retrieval mechanism. Treat it as an operational convenience: clear scoping rules that a human or an agent can apply predictably.

## Programmatic Search

For integrations, use the Rust `searcher` module (positional arguments — Rust doesn't have keyword args):

```rust
use mempalace_core::searcher::search_memories_with_rerank;

let response = searcher::search_memories_with_rerank(
    "auth decisions",
    &palace_path,           // Path to palace directory
    Some("myapp"),          // Optional wing filter
    Some("auth"),           // Optional room filter
    5,                      // n_results
    None,                   // Optional custom embedding model
    true,                   // enable BM25 rerank
    None,                   // max_per_session
    None,                   // fusion_mode ("vector" | "ppr" | "hybrid")
)?;

for hit in &response.results {
    println!("[{:.3}] {}/{}", hit.similarity, hit.wing, hit.room);
    println!("  {}", &hit.text[..hit.text.len().min(200)]);
}
```

The response shape:

```rust
pub struct SearchResponse {
    pub query: String,
    pub filters: Filters,
    pub results: Vec<SearchHit>,
}

pub struct SearchHit {
    pub text: String,           // "We decided to migrate auth to Clerk because..."
    pub wing: String,            // "myapp"
    pub room: String,           // "auth-migration"
    pub source_file: String,     // "session_2026-01-15.md"
    pub similarity: f32,        // 0.892
}
```

## MCP Search

When connected via MCP, your AI searches automatically:

> *"What did we decide about auth last month?"*

The AI calls `mempalace_search` behind the scenes. You never type a search command.

See [MCP Integration](/guide/mcp-integration) for setup.

## Wake-Up Context

Instead of searching, you can load a compact context of your world:

```bash
# Load identity + top memories (~600-900 tokens in typical use)
mpr wake-up

# Project-specific context
mpr wake-up --wing driftwood
```

This loads Layer 0 (identity) and Layer 1 (essential story) as bounded startup context before the first retrieval call.

See [Memory Stack](/concepts/memory-stack) for details on the 4-layer architecture.
