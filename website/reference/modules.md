# Module Map

Source file reference for the MemPalace Rust workspace. Three crates:

```
mempalace_rust/
├── crates/
│   ├── core/                       ← core library (120+ modules)
│   │   ├── src/
│   │   │   ├── lib.rs              ← crate root + re-exports
│   │   │   ├── cli.rs              ← CLI entry: ~50 clap commands
│   │   │   ├── mcp_server.rs       ← MCP server (84 tools, prefix mempalace_)
│   │   │   ├── rest_api.rs         ← HTTP REST API (feature: http-server)
│   │   │   ├── searcher.rs         ← semantic + BM25 + hybrid search
│   │   │   ├── layers.rs           ← 4-layer memory stack (L0..L3)
│   │   │   ├── knowledge_graph.rs  ← temporal KG (SQLite-backed)
│   │   │   ├── palace_graph.rs     ← room navigation graph + tunnels
│   │   │   ├── dialect.rs          ← AAAK compression
│   │   │   ├── config.rs           ← serde-based config (~30 fields)
│   │   │   ├── palace_db.rs        ← unified SQLite + vector store facade
│   │   │   ├── drawer_store.rs     ← drawer persistence + FTS5
│   │   │   ├── embed/              ← embedding backends (ONNX, naive, hash)
│   │   │   ├── compress.rs         ← AAAK compression orchestration
│   │   │   ├── compress_file.rs    ← per-file AAAK pass
│   │   │   ├── compress_synthetic.rs ← AAAK over drawer metadata
│   │   │   ├── convo_miner.rs      ← conversation ingest (8+ formats)
│   │   │   ├── general_extractor.rs ← 5-type memory extraction
│   │   │   ├── normalize.rs        ← chat-format → transcript normaliser
│   │   │   ├── entity_detector.rs  ← auto-detect people/projects
│   │   │   ├── entity_registry.rs  ← entity name → AAAK code registry
│   │   │   ├── room_detector_local.rs ← room detection from directories
│   │   │   ├── spellcheck.rs       ← optional spell checking
│   │   │   ├── coordination/       ← multi-agent coordination primitives
│   │   │   ├── consolidation.rs / consolidation_pipeline.rs
│   │   │   ├── auto_forget.rs      ← retention / eviction
│   │   │   ├── evict.rs            ← drawer eviction
│   │   │   ├── doctor.rs           ← diagnostics (`mpr diagnose`)
│   │   │   ├── heal.rs             ← repair helpers (`mpr repair`)
│   │   │   ├── audit.rs            ← governance audit log
│   │   │   ├── governance.rs       ← delete + retention policies
│   │   │   ├── observe.rs          ← lifecycle hook observation handler
│   │   │   ├── session.rs          ← SessionStore (Stop / PreCompact counters)
│   │   │   ├── observe_types.rs    ← HookType enum + HookPayload struct
│   │   │   ├── access_tracker.rs   ← drawer access counters (for auto_forget)
│   │   │   ├── facet.rs / facets.rs ← structured per-drawer metadata
│   │   │   ├── branch_aware.rs / claude_bridge.rs
│   │   │   ├── auth.rs             ← MCP token auth
│   │   │   ├── connector.rs        ← external integration connector
│   │   │   ├── event_capture.rs / diary_ingest.rs
│   │   │   ├── cascade.rs / cascade_retrieval.rs
│   │   │   ├── crystallize.rs      ← promote slot → drawer
│   │   │   ├── export.rs / exporter.rs / corpus_origin.rs
│   │   │   ├── dedup.rs / dedup_window.rs
│   │   │   ├── flow_compress.rs    ← flow-based AAAK compression
│   │   │   ├── closet_llm.rs       ← LLM-backed closet (summary) builder
│   │   │   ├── health.rs           ← liveness checks
│   │   │   ├── enrich.rs           ← metadata enrichment
│   │   │   ├── file_index.rs       ← file → drawer index
│   │   │   ├── context.rs          ← ContextBuilder (XML context output)
│   │   │   ├── mesh.rs             ← agent mesh sync
│   │   │   ├── trace.rs / tracing_setup.rs
│   │   │   ├── signal_handler.rs   ← graceful shutdown
│   │   │   ├── bm25.rs             ← BM25 index wrapper
│   │   │   ├── usearch_store.rs    ← usearch HNSW wrapper
│   │   │   ├── palace.rs           ← in-memory Palace facade
│   │   │   ├── snapshot.rs         ← memory snapshots
│   │   │   ├── profile.rs          ← per-wing profile aggregation
│   │   │   ├── frontier.rs         ← pending-work queue
│   │   │   ├── actions.rs          ← ActionStore (PRs, migrations, …)
│   │   │   ├── lessons.rs / lessons_store.rs
│   │   │   ├── reflect.rs          ← reflection pass over observations
│   │   │   ├── commit_correlator.rs / commits.rs
│   │   │   ├── query_sanitizer.rs  ← search query sanitisation
│   │   │   ├── tier_classifier.rs  ← drawer importance tiering
│   │   │   ├── wal.rs              ← write-ahead log for tool calls
│   │   │   ├── md_index.rs         ← markdown index
│   │   │   ├── notes.rs            ← AGENT.md / USER.md notes
│   │   │   ├── clusters.rs / cluster_engine.rs
│   │   │   └── … (full list: `ls crates/core/src`)
│   │   └── tests/                  ← 1,400+ tests (unit + integration)
│   ├── cli/                        ← thin binary entry point
│   │   └── src/main.rs             ← forwards to `mempalace_core::cli::run()`
│   └── bench/                      ← benchmark harness
│       └── src/
│           ├── main.rs             ← CLI entry for `mempalace-bench`
│           ├── runner.rs           ← bench orchestration
│           ├── longmemeval_fetch.rs
│           ├── longmemeval_harness.rs
│           ├── dataset.rs
│           └── metrics.rs
├── plugin/
│   ├── hooks/                      ← Claude Code / Codex hook wrappers
│   │   ├── mempal_hook.sh
│   │   ├── mempal_save_hook.sh
│   │   └── mempal_precompact_hook.sh
│   ├── scripts/                    ← plugin install scripts
│   ├── skills/                     ← Claude skill definitions
│   └── plugin.json                 ← Claude plugin manifest
├── integrations/
│   └── openclaw/                   ← OpenClaw skill bundle
├── instructions/                   ← embedded instruction markdown
│   ├── init.md  search.md  mine.md  help.md  status.md
├── docs/                           ← design notes, RFCs, and audits
├── specs/                          ← spec documents
├── website/                        ← this site (VitePress)
└── Cargo.toml                      ← workspace manifest
```

## Core Modules

### `cli.rs` — CLI Entry Point

Clap-based CLI with ~50 subcommands: `init`, `mine`, `search`, `wake-up`, `compress`, `split`, `context`, `consolidate`, `export`, `import`, `snapshot`, `forget`, `evolve`, `actions`, `frontier`, `signals`, `mesh`, `vision`, `connect`, `remove`, `deinit`, `demo`, `upgrade`, `stop`, `serve`, `mcp`, `hook`, `remember`, `recall`, `user`, `config`, `instructions`, `repair`, `status`, `sessions`, `diagnose`, `profile`. See [CLI Commands](/reference/cli).

### `mcp_server.rs` — MCP Server

JSON-RPC over stdin/stdout (default `mpr serve`) or HTTP (with `--features http-server`). Implements the MCP protocol with **84 tools** (all prefixed `mempalace_`) covering palace read/write, drawer CRUD, knowledge graph, navigation, tunnels, agent diary, sessions, slots, signals, sentinels, lessons, reflections, working memory, observation hooks, commit correlation, and more. Includes the Memory Protocol and AAAK Spec in status responses. See [MCP Tools](/reference/mcp-tools).

### `rest_api.rs` — HTTP REST API

Axum-based HTTP server gated behind the `http-server` cargo feature. Wraps the same tool surface as the MCP server. Default port `3111`, overridable with `MEMPALACE_HTTP_PORT`. Used by the Hermes plugin and other external clients.

### `searcher.rs` — Semantic Search

Two top-level functions: `search_memories()` for simple queries and `search_memories_with_rerank()` for BM25/PPR/hybrid pipelines. Both query the underlying `PalaceDb` (a SQLite + optional usearch HNSW facade) with optional wing/room filters and return verbatim drawer content with similarity / relevance scores.

### `layers.rs` — Memory Stack

Four structs (`Layer0` through `Layer3`) and the unified `MemoryStack`. Layer 0 reads identity, Layer 1 auto-generates from top drawers, Layer 2 does filtered retrieval, Layer 3 does semantic search.

### `knowledge_graph.rs` — Temporal KG

SQLite-backed entity-relationship graph with temporal validity windows. Supports `add_entity`, `add_triple`, `invalidate`, `query_entity`, `query_relationship`, `timeline`, and `stats`. Auto-creates entities on triple insertion.

### `palace_graph.rs` — Navigation Graph

`PalaceGraph::new()` + `build(palace_path)` builds an in-memory graph where nodes = rooms and edges = tunnels (rooms spanning multiple wings). Supports BFS traversal (`traverse`), tunnel finding (`find_tunnels`), and explicit cross-wing tunnels (`create_tunnel` / `list_tunnels` / `delete_tunnel`).

### `dialect.rs` — AAAK Compression

Lossy abbreviation system with entity encoding, emotion detection, topic extraction, and flag identification. Works on both plain text and structured zettel data.

## Ingest Modules

### `convo_miner.rs` — Conversation Ingest

Imports conversation exports (Claude Code JSONL, Claude.ai JSON, ChatGPT JSON, Slack JSON, Codex CLI JSONL, SoulForge JSONL, OpenCode SQLite, plain text / Markdown). Chunks by exchange pair. Supports `exchange` and `general` extraction modes.

### `normalize.rs` — Format Converter

Converts 8+ chat formats to a standard transcript format before mining.

### `general_extractor.rs` — Memory Type Extraction

Classifies conversation content into decisions, preferences, milestones, problems, and emotional context.

### `compress.rs`, `compress_file.rs`, `compress_synthetic.rs`, `flow_compress.rs`

Four AAAK compression paths covering live drawers, file-level compression, synthetic compression from metadata, and flow-based compression.

## Detection Modules

### `entity_detector.rs` — Entity Detection

Scans file content to auto-detect people and projects using regex patterns and heuristics.

### `entity_registry.rs` — Entity Registry

Manages entity name → code mappings for AAAK compression. Persisted as `entities.json`.

### `room_detector_local.rs` — Room Detection

Maps folder structure to room names during `mpr init`.

## Coordination & Multi-Agent

### `coordination/`

Multi-agent coordination primitives: file leases (`lease.rs`), sentinels (`sentinel.rs`), signal bus (`signal.rs`), team feed (`team.rs`), routines (`routine.rs`).

### `mesh.rs`

Mesh sync between agents (`mpr mesh sync|status|peers`).

### `observe.rs` + `observe_types.rs` + `session.rs`

Lifecycle hook observation pipeline. `mpr hook --hook stop` records an observation via `observe::process_observation`, which is then persisted by `SessionStore`. Sessions track per-session counters for the auto-save cadence (`SAVE_INTERVAL = 15` in `cli.rs`).

## Lifecycle & Repair

### `auto_forget.rs` / `evict.rs` / `retention.rs`

Retention scoring, eviction, and tier-based forgetting. Drives `mpr forget`.

### `consolidation.rs` / `consolidation_pipeline.rs`

Consolidation passes over the palace. Drives `mpr consolidate`.

### `doctor.rs` / `heal.rs` / `audit.rs` / `governance.rs`

Diagnostics and governance: `mpr diagnose`, `mpr repair`, and audited deletion paths.

### `wal.rs`

Write-ahead log for tool calls (used by MCP `mempalace_health` and audit replay).
