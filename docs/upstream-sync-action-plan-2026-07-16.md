# Upstream Sync Action Plan — mempalace_rust

**Date:** 2026-07-16
**Local HEAD:** ad3f7dd (branch: main)
**Workspace version:** 0.6.6
**Upstream HEAD (mempalace/mempalace, v3.6.0):** ec8788c
**Upstream HEAD (rohitg00/agentmemory, v0.9.27+):** 93ae9bc

## TL;DR

Local Rust port (workspace 0.6.6) is broadly current with mempalace JS v3.6.0 for headline surface and **exceeds** agentmemory's MCP catalog (97 vs 53 tools). The blocking gap is a cluster of correctness/security fixes that haven't been ported: atomic `KG.supersede()` w/ half-open intervals, NUL-byte FTS5 sanitization, `MEMPALACE_STARTUP_INTEGRITY_MAX_MB` probe gate, `exclude_patterns` config plumbing, and `mempalace serve --host/--token/--tls-cert/--tls-key` turnkey remote serve. The single most urgent fix is **#1 (atomic `KG.supersede`)**: without it, superseded facts and their successors both return at the boundary, breaking KG semantics for any caller using `valid_from`/`valid_to`. The single most impactful UX unlock is **#5 (`serve` turnkey flags)**: unblocks Docker/systemd/hosted deployments.

Port the P0 set first as a single batched PR (`fix/p0-kg-supersede-and-fence`); the ten top items below are ordered by risk × leverage, not alphabetically.

---

## 🔴 P0 — Critical fixes to port immediately

### P0-1. Atomic `KG.supersede()` + `mempalace_kg_supersede` tool
- **Upstream:** `9815f0a` — `/tmp/upstream-check/mempalace-js/mempalace/knowledge_graph.py:370` + `/tmp/upstream-check/mempalace-js/mempalace/mcp_server.py:3412`
- **Why:** Hard correctness fix — superseded fact + successor must both return at `[valid_from, valid_to)` boundary. Currently `KG.supersede()` returns null and `mempalace_kg_supersede` MCP tool doesn't exist.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/knowledge_graph.rs:289-344` (add `pub fn supersede(...)`) + `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/mcp_server.rs:611` (add `tool_kg_supersede` to `make_tools()` and to `MUTATION_TOOLS` at line 258)
- **Change:** New `supersede(old_fact_id, new_fact: FactCreate) -> Result<Fact>` that bumps `valid_to` of old fact to `new.valid_from` (half-open), then inserts successor in same transaction. Tool schema: `{fact_id: String, replacement: {subject, predicate, object, valid_from}}`. Register in `MUTATION_TOOLS` for invalidation.
- **Verify:** `cargo test --package mempalace-core --lib knowledge_graph::tests::supersede_atomicity` — assert (a) old fact has `valid_to == new.valid_from`, (b) both facts returned by `query(at=new.valid_from-1ns)` and `query(at=new.valid_from)` (old first, then new).
- **Effort:** M

### P0-2. `MEMPALACE_STARTUP_INTEGRITY_MAX_MB` gate + async preflight
- **Upstream:** `c54531a` (max-MB gate) + `e360a3a` (background-thread preflight) — `/tmp/upstream-check/mempalace-js/mempalace/mcp_server.py:342,350`
- **Why:** A 4.6 GB palace hangs `initialize` for 60s; can stall every client. Affects every hosted install.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/mcp_server.rs:1186+` (MempalaceServer init) + `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/health.rs`
- **Change:** (1) Add env `MEMPALACE_STARTUP_INTEGRITY_MAX_MB` (default `512`). (2) In `ServerHandler::initialize`, check palace size; if over threshold, skip probe. (3) Move sqlite-integrity probe into `tokio::spawn`, immediately return `ServerInfo` from `initialize`, let preflight run in background.
- **Verify:** `cargo run --bin mempalace -- serve` then attach with `mcpc`; `initialize` returns < 100 ms even with a 1 GB synthetic palace.
- **Effort:** S

### P0-3. NUL-byte sanitization before FTS5 indexing
- **Upstream:** `4e20ad3` — `/tmp/upstream-check/mempalace-js/mempalace/backends/chroma.py:1375`
- **Why:** One `\0` in text (e.g. from `Bash` tool output) corrupts FTS5 inverted index; miner fails with banner. Affects every Bash transcript.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/normalize.rs` (new `pub fn sanitize_for_fts5<'a>(t: &'a str) -> Cow<'a, str>`) + `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/drawer_store.rs` (apply inside `DrawerStore::add_drawer` on `text` and `title`)
- **Change:** Strip `\0` (and surrogate pairs for completeness); return `Cow<str>` to avoid alloc when clean. Apply *before* any FTS5 `INSERT`, *before* BM25 tokenization, *before* embedder call.
- **Verify:** `cargo test draw_store::tests::nul_byte_does_not_corrupt_fts5` — insert `"helloworld"`, run `SELECT rowid FROM drawers_fts WHERE drawers_fts MATCH 'world'`, assert 1 row.
- **Effort:** S

### P0-4. SQLite busy-timeout bump to 15 s on integrity probe
- **Upstream:** `7267de2` — repair path integration
- **Why:** 5 s default timeout flaps spuriously under load, triggering false-positive "corrupt" reports in `mempalace repair`.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/repair.rs:573` + `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/health.rs` (probe)
- **Change:** `Connection::busy_timeout(Duration::from_secs(15))` (matching upstream `7267de2`) at the top of every probe/repair path that opens the palace DB.
- **Verify:** `cargo test repair::tests::busy_timeout_is_at_least_15s` — assert `pragma busy_timeout >= 15000`.
- **Effort:** S

### P0-5. `mempalace_list_drawers` MCP tool with `since`/`before` date filter
- **Upstream:** `833b6ab` — `/tmp/upstream-check/mempalace-js/mempalace/mcp_server.py:3118` + `drawer_store.py`
- **Why:** Tool is referenced by upstream #1128 for housekeeping/dashboards and is missing entirely from local catalog (only `list_wings`/`list_rooms` exist).
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/mcp_server.rs:611` (new `tool_list_drawers` in `make_tools()`) + `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/drawer_store.rs:167` (extend `list_all` to accept `since: Option<NaiveDate>`, `before: Option<NaiveDate>` → emit `WHERE filed_at >= ? AND filed_at < ?`)
- **Change:** Schema: `{since?: "YYYY-MM-DD", before?: "YYYY-MM-DD", wing?: Option<String>, limit?: u32, offset?: u32}`. Output `{drawers: [{id, title, source_file, filed_at, wing}]}`.
- **Verify:** `cargo test mcp::tests::list_drawers_date_filter` — seed 5 drawers across 3 days, filter to a 2-day window, assert 3 returned in `filed_at DESC` order.
- **Effort:** S

---

## 🟡 P1 — Feature gaps to close this sprint

### P1-1. `exclude_patterns` Config field + `mempalace mine --exclude` CLI flag
- **Upstream:** `b70f06a` + `3f49e15` — `/tmp/upstream-check/mempalace-js/mempalace/miner.py` + `/tmp/upstream-check/mempalace-js/mempalace/config.py`
- **Why:** Dead surface — `miner.rs:1175,1181,1210` already *accepts* `exclude_patterns` argument but `Config` has no such field, so users can't configure it via `mempalace.yaml`.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/config.rs:506+` (add `pub exclude_patterns: Vec<String>` to `Config`, deserialize from YAML) + `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/cli.rs` (add `--exclude <GLOB>` to `Commands::Mine`, repeatable) + thread into `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/miner.rs:1175`
- **Verify:** Write `mempalace.yaml` with `exclude_patterns: ["*.log", "node_modules/**"]`; run mine; assert `*.log` files skipped before any I/O.
- **Effort:** S

### P1-2. `authored_at` metadata key + extraction + backfill
- **Upstream:** `cff43ad` — `/tmp/upstream-check/mempalace-js/mempalace/convo_miner.py:456` + `/tmp/upstream-check/mempalace-js/scripts/backfill_authored_at.py`
- **Why:** Distinguishes original transcript timestamp (`authored_at`) from mine time (`filed_at`). Required for recency tie-break search, correct diary ordering, and audit trails.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/convo_miner.rs` (extract `authored_at` from transcript first-message header) + `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/drawer_store.rs:61` (add `authored_at TEXT` column, nullable) + `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/searcher.rs` (tie-break `ORDER BY COALESCE(authored_at, filed_at) DESC, filed_at DESC`) + `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/migrate.rs` (new `migrate authored-at` subcommand)
- **Verify:** Mine a fixture transcript with known timestamp; assert drawer row carries `authored_at` = expected ISO-8601 string.
- **Effort:** M

### P1-3. `mempalace serve --host --token --tls-cert --tls-key` turnkey remote serve
- **Upstream:** `afd0428` — `/tmp/upstream-check/mempalace-js/mempalace/cli.py:1417`
- **Why:** Unblocks Docker/systemd/hosted deployments. Currently `--http`/`--mcp-http`/`--read-only`/`--port`/`--instance` exist; no host bind, no auth, no TLS.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/cli.rs:295` (`Commands::Serve`) + `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/mcp/http_transport.rs`
- **Change:** Add flags `--host <IP>`, `--token <BEARER>` (auto-gen UUIDv7 if omitted + print to stderr), `--tls-cert <PEM>`, `--tls-key <PEM>`. Add `axum-server` dep with `tls-rustls` feature to `/Users/tranquangdang21/Projects/mempalace_rust/Cargo.toml`. Enforce bearer when `host` is non-loopback.
- **Verify:** `mempalace serve --host 0.0.0.0 --port 8443 --tls-cert cert.pem --tls-key key.pem`; `curl -k -H "Authorization: Bearer $TOKEN" https://localhost:8443/health` → 200.
- **Effort:** M

### P1-4. Embedder-identity 3-state enforcement + sidecar file
- **Upstream:** RFC 001 §10 + `_sidecar.py` — `/tmp/upstream-check/mempalace-js/mempalace/backends/_sidecar.py` + `mempalace/backends/base.py`
- **Why:** Silent embedder model swap → corrupt vector index across *all* backends. Currently Rust has no enforcement on backend switch.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/embed/` (new `EmbedderIdentity { model: String, dim: u32, hash: [u8;8] }` + `enum IdentityMatch { Match, Mismatch, Unknown }`) + new `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/palace/embedder_sidecar.rs` (read/write `<palace>/.embedder.json`) + enforce in `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/palace_db.rs::open`
- **Change:** On open, compare live identity against sidecar: `Match` = OK, `Mismatch` = refuse to open with actionable error, `Unknown` = emit warning + write sidecar. CLI flag `--force-embedder-rebuild` to override.
- **Verify:** Test (a) match — open twice with same embedder, (b) mismatch — swap embedder, open, assert refuses with clear error, (c) unknown — first open writes sidecar + emits one-shot warning.
- **Effort:** M

### P1-5. `mempalace_delete_by_source` purge closets/AAAK too
- **Upstream:** `5ae2315` — `/tmp/upstream-check/mempalace-js/mempalace/mcp_server.py:2061` (`_purge_source_closets`)
- **Why:** Stale AAAK index pointers remain after source-file deletion, causing spurious hits via closet path.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/drawer_store.rs:412` (`delete_by_source` — extend to also `DELETE FROM closets WHERE source_file = ?`)
- **Verify:** Insert 5 drawers + 3 closets from `foo.txt`; `delete_by_source("foo.txt")`; assert 0 closet rows remain; `mempalace_search "needle from foo.txt"` returns 0.
- **Effort:** S

### P1-6. `mempalace_search` accepts optional `source_file` filter
- **Upstream:** `7fb7bd3` — `/tmp/upstream-check/mempalace-js/mempalace/mcp_server.py` (search schema)
- **Why:** Source-scoped search is a common ops request. Currently `source_file` is in result rows but not in query schema.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/mcp_server.rs` (`tool_search` schema) + `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/searcher.rs` (thread param through FTS5/BM25/embedding paths)
- **Verify:** Seed drawers from 2 source files; `tool_search({q: "x", source_file: "a.rs"})` returns only `a.rs` rows.
- **Effort:** S

### P1-7. Milvus storage backend
- **Upstream:** `9b395fe` + 4 sibling commits, 1216-line `MilvusStore` module
- **Why:** Listed in headline v3.6.0 backend matrix; Rust only has `pgvector` and `qdrant` behind features. Closes the multi-backend story for users with existing Milvus infra.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/palace/store.rs:33-57` (add `#[cfg(feature = "backend-milvus")] pub mod milvus;`) + new `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/palace/store/milvus.rs` + new feature flag in `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/Cargo.toml` (likely pull `milvus-client` from crates.io or REST-proxy approach)
- **Verify:** `cargo test --features backend-milvus palace::store::milvus::tests::round_trip_insert_search` against embedded mock.
- **Effort:** M-L

### P1-8. `repair --mode from-sqlite` (analog reconstruction path)
- **Upstream:** `d7e182a` + `65d0704` + `44016ad` + `5dcc46b` (mixed) — `/tmp/upstream-check/mempalace-js/mempalace/repair.py` + upstream `chroma.sqlite3` path
- **Why:** Recovery path when FTS5/BM25 index diverges from `palace.sqlite3`. Note: Rust has no `chroma.sqlite3`; this is *analogous* reconstruction from native `palace.sqlite3`.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/repair.rs` (new mode branch that reads every row from `palace.sqlite3`, tokenizes BM25, rebuilds `drawers_fts`, in one transaction)
- **Verify:** Deliberately corrupt `drawers_fts`; `mempalace repair --mode from-sqlite`; assert `MATCH 'unique_term'` returns the right row.
- **Effort:** M

### P1-9. Tilde expansion in `palace_path` from `config.json`
- **Upstream:** `0f3f8ec` — small one-liner
- **Why:** `~/palaces` is the natural path users write; currently raw string used → silent "palace not found" until first run.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/config.rs:506+` (add `pub fn expand_tilde(p: &Path) -> PathBuf` and call in `Config::load()`)
- **Verify:** `mempalace.yaml` with `palace_path: "~/palaces"`, `Config::load()` returns `/home/user/palaces`.
- **Effort:** S

### P1-10. `MEMPALACE_HTTP_TOKEN` enforced on non-loopback HTTP transport
- **Upstream:** `afd0428` (companion flag) + `--token` per P1-3
- **Why:** DNS-rebinding guard is already in `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/mcp/http_transport.rs` but loopback default remains unauthenticated. Once `--host` lands (P1-3), token enforcement must land with it.
- **Local target files:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/mcp/http_transport.rs`
- **Verify:** Start with `--host 0.0.0.0` + no token → connect refused on non-loopback; with `--token xxx` → connect succeeds only with `Authorization: Bearer xxx`.
- **Effort:** S

---

## 🟢 P2 — Nice-to-have parity improvements

### P2-1. SQLite magic-header check in `palace_db.rs::open`
- **Upstream:** `cd7a865`
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/palace_db.rs` — verify 16-byte `b"SQLite format 3\x00"` magic before opening
- **Effort:** S

### P2-2. Append `kg_supersede`, `checkpoint`, `delete_by_source`, `mine` to `MUTATION_TOOLS`
- **Upstream:** `578e577`
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/mcp_server.rs:258 MUTATION_TOOLS` — verify completeness after P0-1 lands
- **Effort:** S

### P2-3. Auto-heal FTS5 mid-mine, not just in `repair`
- **Upstream:** `adc0ef8` — `/tmp/upstream-check/mempalace-js/mempalace/miner.py`
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/drawer_store.rs::add_drawer` error path → call `maybe_autoheal_fts5()`
- **Effort:** S

### P2-4. mtime-keyed file-already-mined cache (re-mine support)
- **Upstream:** `9b43269`
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/convo_miner.rs:189` — replace `bool` state with `HashMap<PathBuf, SystemTime>`; re-mine if mtime newer
- **Effort:** S

### P2-5. Recognize both SQLite FTS5 corruption wordings
- **Upstream:** `f9db9b7`
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/repair.rs` — broaden `_is_fts5_corruption()` to match both `"database disk image is malformed"` and the newer `"malformed inverted index"` strings
- **Effort:** S

### P2-6. `try_init()` not `init()` for log subscribers
- **Upstream:** `c17d1aa`
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/mcp_server.rs` + `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/cli.rs` — switch from `tracing_subscriber::fmt().init()` to `.try_init()`
- **Effort:** S

### P2-7. `MEMPALACE_MCP_IDLE_EXIT_SECONDS` watchdog for HTTP transport
- **Upstream:** `2f92774`
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/mcp/http_transport.rs` — add `tokio::time::timeout` watchdog on idle connection
- **Effort:** S

### P2-8. LaTeX + 5 new locales (pt-br, it, id, zh-TW, be)
- **Upstream:** `6cec591` + `833b6ab` (locales)
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/miner.rs` (add `.tex`, `.bib` to readable extensions) + `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/locales/` (add `pt-BR.yml`, `it.yml`, `id.yml`, `zh-TW.yml`, `be.yml`)
- **Effort:** S

### P2-9. COCA wordlist filter for entity detection
- **Upstream:** (v3.3.6) — `/tmp/upstream-check/mempalace-js/mempalace/data/coca_content_words.json`
- **Local:** new `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/data/coca_content_words.json` + wire into `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/entity_detector.rs`
- **Effort:** S

### P2-10. Office-doc mining (`--mode extract`, PDF/DOCX/PPTX/XLSX/RTF/EPUB)
- **Upstream:** (v3.3.6)
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/miner.rs` (add `--mode extract`) + new `extract` feature pulling `pdf-extract`, `docx-rs`, `zip` etc.
- **Effort:** M

### P2-11. Plugin marketplace packages (.cursor-plugin, .antigravity-plugin, .copilot-plugin)
- **Upstream:** (v3.4.1 / post-tag)
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/plugin/` — add 3 marketplace manifests
- **Effort:** S

### P2-12. 11 README translations
- **Upstream:** `26980a8`
- **Local:** new `/Users/tranquangdang21/Projects/mempalace_rust/READMEs/` dir (en, de, es, fr, ja, ko, pt-br, ru, zh-CN, zh-TW, hi)
- **Effort:** M (mostly translation work)

### P2-13. Skill corpus to 35 dirs (currently 16)
- **Upstream:** `45de643` — `/tmp/upstream-check/agentmemory-py/skills/`
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/plugin/skills/` — add `remember`, `forget`, `commit-context`, `agentmemory-hooks`, `agentmemory-rest-api`, etc.
- **Effort:** S per skill

### P2-14. `mempalace entities` / `mempalace hallways` CLI subcommands
- **Upstream:** `5dcc46b` — `/tmp/upstream-check/mempalace-js/mempalace/cli.py`
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/cli.rs` — add `Commands::Entities` + `Commands::Hallways` (logic exists in `entity_registry.rs`/`palace_graph.rs`)
- **Effort:** S

### P2-15. `mempalace init --auto-mine` + `mempalace mine --redetect-origin`
- **Upstream:** (v3.3.4)
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/cli.rs` — add flags
- **Effort:** S

### P2-16. `MemoExit` stress test for SIGINT-mid-mine determinism
- **Upstream:** `4b98dd7`
- **Local:** new test in `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/mine_palace_lock.rs`
- **Effort:** S

### P2-17. Self-heal writer lease after peer exit (test coverage)
- **Upstream:** `9e871b4`
- **Local:** `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/coordination/leases.rs` — add cross-peer recovery test
- **Effort:** S

### P2-18. KG side-index tables (graphNameIndex / graphEdgeKey / graphNodeDegree)
- **Upstream:** `2a58140`
- **Note:** Mostly redundant with SQLite-native indexes in Rust; only worth doing if `tool_kg_snapshot_rebuild` profiling shows missing indexes.
- **Effort:** M (low priority — measure first)

### P2-19. Verify cluster items (no code change, just audits + tests)
The following were reported PARTIAL by the cross-compare sweep — confirm via a focused grep/test pass per item before merging:
- `477aa36` `graph_stats` SQL-aggregate fast path — `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/knowledge_graph.rs:135K`
- `bacb935` L1 wake-up recency ordering — `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/layers.rs`
- `c9dc4c4` `--limit` counts only new work — `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/miner.rs`
- `d486aef` purges matching closets — already P1-5
- `faa8643` HTTP transport re-entrancy — add cross-thread test
- `bf71ae7` tunnel file scoping — already correct in Rust
- `4291fec` `tool_checkpoint` batch path — verify N×check_duplicate batching
- `b8027c9` `npx skills add` hint after wire — `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/connect/mod.rs:8.7K`
- `249d8ff` `/agentmemory/commits`, `/session/commit`, `/session/by-commit` REST routes — `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/rest_api.rs:108K`

For each, write a brief "verified OK" or "needs fix" note in `/Users/tranquangdang21/Projects/mempalace_rust/docs/upstream-sync-audit-2026-07.md`.

---

## ℹ️  N/A — Acceptable divergences

These items are intentionally NOT ported because the Rust stack differs fundamentally from the upstream JS/Python stack:

- **`dc43382` Stop quarantining all-layer-0 HNSW segments** — Rust uses `usearch` + SQLite, not Chroma HNSW. No analogous quarantine needed.
- **`fd3b4e8` pgvector skips document column** — Worth a quick audit of `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/palace/store.rs:57` pgvector module but no upstream analog.
- **`38253b1` / `31fff1c` Percent-encode sqlite read-only URIs** — Rust uses direct `rusqlite::Connection::open` paths, no URI form. Skip.
- **`4fd1231` Drop `_HALLWAY_FILE` back-compat shim** — Local has no shim. N/A.
- **`5024e90` Length-prefixed ID recipe v3** — Local uses `uuid::Uuid`. Acceptable divergence.
- **`93d1bdd` Pin `iii-sdk` to 0.11.2** — Local uses `rmcp` 1.3. Different MCP framework.
- **`a35de80` `trigger + TriggerAction.Void()`** — Rust uses `rmcp` directly, no trigger abstraction. N/A.
- **`f40631e` `AGENTMEMORY_VIEWER_HOST`** — Rust has no viewer server. Roadmap item, not a gap.
- **`3e7b61e` Reject non-loopback Host headers in viewer** — Same as above.
- **`d4c4061` Viewer "Memories" tab sort** — No viewer.
- **`0ee3e65` Viewer handles missing ids** — No viewer.
- **`7fb72f4` In-house benchmark corpus for agentmemory** — Verify presence of `coding-agent-life-v1` in `/Users/tranquangdang21/Projects/mempalace_rust/crates/core/src/eval/`. If absent, P2 priority to add; not blocking release sync.

---

## Recommended worktree branches

```bash
cd /Users/tranquangdang21/Projects/mempalace_rust

# Branch 1 — correctness fence (P0 items)
git worktree add ../mempalace_rust.wt-p0 -b fix/p0-kg-supersede-and-fence main
# files touched:
#   crates/core/src/knowledge_graph.rs (P0-1)
#   crates/core/src/mcp_server.rs        (P0-1, P0-2, P0-5)
#   crates/core/src/normalize.rs         (P0-3)
#   crates/core/src/drawer_store.rs      (P0-3, P0-5)
#   crates/core/src/repair.rs            (P0-4)
#   crates/core/src/health.rs            (P0-2, P0-4)

# Branch 2 — config + CLI gap close (P1-1, P1-9, P1-5, P1-6, P1-15, P2-14)
git worktree add ../mempalace_rust.wt-p1-config -b feat/p1-config-and-cli main

# Branch 3 — remote serve / TLS (P1-3, P1-10, P2-7)
git worktree add ../mempalace_rust.wt-p1-serve -b feat/p1-remote-serve main

# Branch 4 — embedder identity + repair (P1-4, P1-8)
git worktree add ../mempalace_rust.wt-p1-identity -b feat/p1-embedder-identity main

# Branch 5 — author/origin metadata (P1-2)
git worktree add ../mempalace_rust.wt-p1-authored -b feat/p1-authored-at main

# Branch 6 — Milvus + office-doc (P1-7, P2-10) — independent, larger effort
git worktree add ../mempalace_rust.wt-p1-backends -b feat/p1-milvus-and-extract main

# Branch 7 — i18n + plugins + docs (P2-8, P2-11, P2-12, P2-13)
git worktree add ../mempalace_rust.wt-p2-i18n -b chore/p2-locales-and-plugins main

# Branch 8 — verify-audit batch (P2-19 + all PARTIAL items)
git worktree add ../mempalace_rust.wt-p2-audit -b chore/p2-verify-partials main
```

Merge order for safe stacking: `p0` → `p1-config` → `p1-serve` → `p1-identity` → `p1-authored` → `p1-backends` → `p2-*`. P0 should ship first as a single semver-patch release (0.6.7); the P1 cluster ships as 0.7.0.

---

## Verification plan

Run from `/Users/tranquangdang21/Projects/mempalace_rust` after each branch lands:

```bash
# Format + lint gate
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings

# Unit + integration tests (including new tests added per P0/P1 item)
cargo test --all --all-features

# Specific targeted tests for ported behaviour
cargo test --package mempalace-core --lib \
  knowledge_graph::tests::supersede_atomicity \     # P0-1
  draw_store::tests::nul_byte_does_not_corrupt_fts5 \  # P0-3
  repair::tests::busy_timeout_is_at_least_15s \        # P0-4
  mcp::tests::list_drawers_date_filter \               # P0-5
  config::tests::tilde_expansion \                     # P1-9
  draw_store::tests::delete_by_source_purges_closets \ # P1-5
  mcp::tests::tool_search_source_file_filter \         # P1-6
  embedder_sidecar::tests::identity_match_mismatch_unknown  # P1-4

# Doc tests
cargo test --doc

# Bench regression (if P1-7 lands new backend; else skip)
cargo bench --features backend-milvus --no-run  # P1-7 only

# MCP smoke test: spawn server, connect with `mcpc`, verify tool catalog has:
#   mempalace_kg_supersede       (P0-1)
#   mempalace_list_drawers       (P0-5)
#   mempalace_search             accepts source_file (P1-6)
cargo run --bin mempalace -- serve --port 8443 &
SERVER_PID=$!
sleep 1
# (verify with `mcpc` or curl /mcp/tools/list)
kill $SERVER_PID

# End-to-end mine smoke: ensure `--exclude` (P1-1) + `authored_at` (P1-2) + NUL sanitization (P0-3) coexist
mkdir -p /tmp/mempalace-smoke
cd /tmp/mempalace-smoke
printf 'a\0b\nhello world' > sample.txt
printf 'exclude_patterns:\n  - "*.log"\n' > mempalace.yaml
cargo run --manifest-path /Users/tranquangdang21/Projects/mempalace_rust/Cargo.toml \
  --bin mempalace -- mine /tmp/mempalace-smoke --exclude "*.tmp"
# assert: sample.txt inserted, NUL stripped, .log files skipped, .tmp files skipped
```

CI: the `Mempalace_STARTUP_INTEGRITY_MAX_MB=512` gate (P0-2) belongs in `/.env.test` so the existing CI matrix exercises both the small and large paths.

Definition of done for the sync milestone:
- All P0 items merged + green CI
- All P1 items merged + green CI
- Audit notes for P2-19 committed at `/Users/tranquangdang21/Projects/mempalace_rust/docs/upstream-sync-audit-2026-07.md`
- CHANGELOG.md bumped to 0.7.0 with sections per P-tier
- README "Upstream Sync" badge updated with the commit-date diff