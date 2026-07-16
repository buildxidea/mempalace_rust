# Upstream Sync Audit — 2026-07

**Date:** 2026-07-16  
**Branch:** `feat/p1-p2-upstream-parity`  
**Base HEAD:** `859eb25` (`fix(p0): port 5 upstream correctness fixes…`)  
**Plan:** `docs/upstream-sync-action-plan-2026-07-16.md`  
**Scope:** Verify P0 (prior commit) + P1/P2 items applied in this PR; residual gaps.

Verdicts:

| Verdict | Meaning |
|---------|---------|
| **verified OK** | Present at base HEAD or prior; re-checked this PR |
| **implemented this PR** | Landed on this branch (uncommitted at audit time) |
| **needs fix** | Still missing / incomplete |
| **deferred** | Explicitly skipped (size / profile / N/A) |

---

## Scoreboard

| Tier | DONE | PARTIAL | MISSING / deferred | Notes |
|------|------|---------|--------------------|-------|
| **P0** (5) | 5 | 0 | 0 | All in `859eb25` |
| **P1** (10) | 8 | 0 | 2 deferred | P1-7 Milvus, — |
| **P2** (19) | 14 | 1 | 4 deferred | P2-13 partial skills; 9/10/12/18 deferred |
| **Total** | **27** | **1** | **6** | |

P1 deferred: **P1-7** (Milvus).  
P2 deferred: **P2-9** (COCA), **P2-10** (office extract), **P2-12** (README i18n), **P2-18** (KG indexes, measure-first).  
P2 partial: **P2-13** (21 skill dirs vs target 35).

---

## P0 — verified OK (base `859eb25`)

| ID | Verdict | Evidence | Residual risk |
|----|---------|----------|---------------|
| **P0-1** KG.supersede + tool | verified OK | `mcp_server.rs:401` `mempalace_kg_supersede` in `MUTATION_TOOLS`; commit message | Boundary half-open query must stay covered in KG tests |
| **P0-2** STARTUP_INTEGRITY_MAX_MB | verified OK | `mcp_server.rs:42-78` env gate default 512 MB | Large-palace CI path not in matrix |
| **P0-3** NUL FTS sanitize | verified OK | `normalize.rs:68` `sanitize_for_fts5`; `drawer_store.rs:402-404` | Must stay on every insert path |
| **P0-4** busy_timeout 15s | verified OK | `repair.rs:74`, test `busy_timeout_is_at_least_15s` ~1300 | — |
| **P0-5** list_drawers | verified OK | `mcp_server.rs:787-789`, handler `2181`, test `list_drawers_date_filter` | — |

---

## P1 — this PR + prior

| ID | Verdict | Evidence | Residual risk |
|----|---------|----------|---------------|
| **P1-1** exclude_patterns + `--exclude` | **implemented this PR** | `config.rs:586` field; `cli.rs:192,1482,5581` tests; `miner.rs:569-705` `scan_project_with_excludes` / `is_excluded` | Glob semantics vs gitignore edge cases |
| **P1-2** authored_at | **implemented this PR** | `drawer_store.rs:84,128-131` column+ALTER; `convo_miner` extract; `searcher.rs:102-151` recency; `migrate.rs` backfill | Legacy rows need migrate for full recency quality |
| **P1-3** serve host/token/TLS | **implemented this PR** | `cli.rs:338-349` flags; `http_transport.rs:97-103` `HttpServeOptions`; `Cargo.toml` `http-tls`; tests `test_p1_3_*` | Live TLS e2e deferred; `http-tls` not default feature |
| **P1-4** embedder identity on open | **implemented this PR** | `palace_db.rs:3226-3273` `classify_against` + strict; `embed/manifest.rs:221` | Force-rebuild is env-only (`MEMPALACE_SKIP_MANIFEST_CHECK`) |
| **P1-5** delete_by_source closets | **implemented this PR** | `drawer_store.rs:625,642-659`; test `test_p1_5_*` ~1503 | Closets table is new local surface — confirm AAAK parity |
| **P1-6** search `source_file` | **implemented this PR** | `searcher.rs:198-213` `filter_by_source_file`; MCP schema + `test_p1_6_*` | Exact path match only (no glob) |
| **P1-7** Milvus backend | **deferred** | `palace/store/`: embedvec, pgvector, qdrant, usearch_sqlite only | SKIP_TOO_LARGE |
| **P1-8** repair from-sqlite | **verified OK** + alias this PR | `repair.rs:490`; `cli.rs:796-799` `alias = "from-sqlite"`; `cli_tests.rs:343-352` | — |
| **P1-9** tilde `palace_path` | **implemented this PR** | `config.rs:81,722-734`; tests `test_p1_9_*` ~1414 | — |
| **P1-10** HTTP token non-loopback | **implemented this PR** | `http_transport.rs:52,181-230` `resolve_http_auth` fail-closed / auto-gen; `rest_api.rs:2785` | Auto-gen prints token to stderr — ops must capture |

---

## P2 — this PR + prior

| ID | Verdict | Evidence | Residual risk |
|----|---------|----------|---------------|
| **P2-1** SQLite magic header | **implemented this PR** | `drawer_store.rs:63-64,1058-1088`; tests `test_p2_1_*` | Plan said palace_db; implemented at DrawerStore open (correct for drawers.db) |
| **P2-2** MUTATION_TOOLS + mine | **implemented this PR** | `mcp_server.rs:392-435` includes `mempalace_mine`; `test_p2_2_*` | — |
| **P2-3** mid-mine FTS autoheal | **implemented this PR** | `drawer_store.rs:445` insert retry; uses `repair::is_fts5_corruption` | Single retry only |
| **P2-4** convo mtime re-mine | **implemented this PR** | `convo_miner.rs:192-194` `check_mtime=true` | — |
| **P2-5** FTS5 corruption matchers | **implemented this PR** | `repair.rs:116-144` `is_fts5_corruption` / `maybe_autoheal_fts5`; tests ~1667 | — |
| **P2-6** try_init tracing | **implemented this PR** | `logging.rs:11` `try_init_tracing`; wired `cli.rs:3446`, `mcp_server.rs:7505` | — |
| **P2-7** idle exit watchdog | **implemented this PR** | `http_transport.rs:69,250-254,792-813`; `test_resolve_idle_exit_seconds` | `process::exit(0)` — no graceful drain |
| **P2-8** LaTeX + locales | **implemented this PR** | `miner.rs:205-206` `.tex`/`.bib`; locales `pt-BR,it,id,zh-TW,be`; i18n `it,id` | Translation quality not natively reviewed |
| **P2-9** COCA wordlist | **deferred** | no `coca_content_words` hook / data file | No wire point without binary bloat |
| **P2-10** Office extract | **deferred** | extract=exchange/general only | SKIP_TOO_LARGE (feature + deps) |
| **P2-11** plugin marketplaces | **implemented this PR** | `plugin/.cursor-plugin/`, `.antigravity-plugin/`, `.copilot-plugin/` each `plugin.json` | Marketplace publish not validated |
| **P2-12** README translations | **deferred** | README.md only | SKIP_TOO_LARGE |
| **P2-13** skills → 35 | **needs fix** (partial) | 21 skill dirs (+`_shared`); added mine/search/status/doctor/entities | ~14 skills still missing vs upstream 35 |
| **P2-14** entities / hallways CLI | **implemented this PR** | `cli.rs:704,3919-3922`; `test_p2_14_*` ~5603 | — |
| **P2-15** auto-mine / redetect-origin | **verified OK** | `cli.rs:129,181` + handlers ~1283,1425 | Already present pre-PR |
| **P2-16** SIGINT mid-mine lock | **implemented this PR** | `mine_palace_lock.rs:252-270` `test_p2_16_*` | Flag-based stress, not real signal inject |
| **P2-17** peer-exit lease heal | **implemented this PR** | `coordination/leases.rs:278-300` `test_p2_17_*` | Test coverage only; runtime path was cleanup_expired |
| **P2-18** KG side indexes | **deferred** | plan: measure first | No graphNameIndex tables |
| **P2-19** this audit doc | **implemented this PR** | `docs/upstream-sync-audit-2026-07.md` | — |

---

## Cluster → commit mapping

Uncommitted on `feat/p1-p2-upstream-parity` at audit time (single PR batch expected):

| Cluster | Items | Status |
|---------|-------|--------|
| A | P1-1, P1-5, P1-6, P1-9, P2-14, P2-15 | applied / skip-done |
| B | P1-3, P1-10, P2-7 | applied |
| C | P1-2 | applied |
| D | P1-4, P1-8 | applied |
| E | P2-1..6, P2-16, P2-17 | applied |
| F | P2-8, P2-11, P2-13 partial; 9/10/12/P1-7 skip | mixed |
| — | P0-1..5 | **verified OK** @ `859eb25` |
| — | P2-19 | this file |

---

## Residual / follow-ups (priority)

1. **P2-13** — grow skills corpus toward 35 (high-value first; no empty stubs).
2. **P1-3** — optional: default-feature or install docs for `--features http-tls`; live TLS smoke.
3. **P1-7 / P2-10** — separate large PR if product needs Milvus / office extract.
4. **P2-9** — only if entity_detector gains a content-word filter hook.
5. **P2-7** — consider graceful shutdown vs hard `exit(0)`.
6. **P2-1 site** — magic check is on `DrawerStore::open` (drawers.db), not `palace_db` open; acceptable, document for upstream parity notes.

---

## How to re-verify

```bash
cargo test -p mempalace-core --lib \
  test_p1_1_ test_p1_2_ test_p1_3_ test_p1_4_ test_p1_5_ test_p1_6_ test_p1_9_ \
  test_p2_1_ test_p2_2_ test_p2_3_ test_p2_5_ test_p2_6_ test_p2_14_ test_p2_16_ test_p2_17_ \
  test_resolve_http_auth test_resolve_idle_exit test_validate_tls

cargo test -p mempalace-core --test cli_tests test_cli_repair_from_sqlite_alias
cargo build -p mempalace-core --lib --features http-server,http-tls
```

Cluster reports: A 12 tests; B 21 http_transport + 2 p1_3; C authored_at suite; D 4 p1_4 + prior; E 11/11; F locales 24 + i18n 6.

---

*P2-19 deliverable. No product code change beyond this audit document.*
