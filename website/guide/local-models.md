# Local Models

MemPalace works with any local LLM — Llama, Mistral, or any offline model. Since local models generally don't speak MCP yet, there are two main approaches: pipe CLI output into the model's context, or expose the palace over the local REST API.

## Wake-Up Command

Load your world into the model's context:

```bash
mpr wake-up > context.txt
# Paste context.txt into your local model's system prompt
```

This gives your local model a bounded wake-up context, typically around **~600-900 tokens** in the current implementation. It includes:
- **Layer 0**: Your identity — who you are, what you work on
- **Layer 1**: Top moments from the palace — key decisions, recent work

For project-specific context:

```bash
mpr wake-up --wing driftwood > context.txt
```

## CLI Search

Query on demand, feed results into your prompt:

```bash
mpr search "auth decisions" > results.txt
# Include results.txt in your prompt
```

You can also dump results as JSON for structured pipelines:

```bash
mpr search "auth decisions" --json | jq '.results[] | {wing, room, text}'
```

## Rust API

For programmatic integration with your local model pipeline:

```rust
use mempalace_core::searcher::search_memories_with_rerank;

let response = searcher::search_memories_with_rerank(
    "auth decisions",       // query
    &palace_path,          // path to palace directory
    Some("myapp"),         // optional wing filter
    Some("auth"),          // optional room filter
    5,                     // n_results
    None,                  // custom embedding model (None = config default)
    true,                  // enable BM25 rerank
    None,                  // max_per_session
    None,                  // fusion_mode: "vector" | "ppr" | "hybrid"
)?;

let context = response.results
    .iter()
    .map(|r| format!("[{}/{}] {}", r.wing, r.room, r.text))
    .collect::<Vec<_>>()
    .join("\n");

// Inject into your local model's prompt
let prompt = format!("Context from memory:\n{}\n\nUser: What did we decide about auth?", context);
```

## HTTP REST API

For local models wrapped in HTTP services, expose the palace as a REST API:

```bash
# Build the http-server feature into mpr (default in release builds)
cargo build --release --features http-server

# Start the REST server on port 3111
mpr serve --http --port 3111

# In read-only mode (block mutations)
mpr serve --http --read-only
```

Then call the API from your model wrapper:

```bash
curl -X POST http://localhost:3111/v1/search \
  -H 'Content-Type: application/json' \
  -d '{"query":"auth decisions","limit":5}'
```

## AAAK for Compression

Use [AAAK dialect](/concepts/aaak-dialect) to compress wake-up context further:

```bash
mpr compress --wing myapp --dry-run
mpr compress --wing myapp
```

AAAK is readable by any LLM that reads text — Claude, GPT, Gemini, Llama, Mistral — without a decoder.

## Full Offline Stack

The core memory stack can run offline:

- **SQLite + usearch vector store** on your machine — vector storage and search
- **Local model** on your machine — reasoning and responses
- **AAAK** for compression — optional, no cloud dependency
- **ONNX embeddings** (`MiniLM`, 384 dim) — runs entirely local, no API calls

## Embedding Model Configuration

MemPalace uses word-overlap by default (`embedding_model: "naive"` in `~/.mempalace/config.json`). For semantic search, it can load ONNX MiniLM via the `tract` runtime. Recognised values for `embedding_model`:

- `naive` (default) — token overlap, 0MB
- `onnx` — ONNX MiniLM, 384 dim, ~90MB
- `paraphrase-multilingual-MiniLM-L12-v2` — multilingual ONNX model
- `all-MiniLM-L6-v2` — fast English ONNX model

You can also configure different search strategies at init time:

```bash
mpr init ~/projects/myapp --search-strategy embedding
```

See [Configuration](/guide/configuration) for the full key list.
