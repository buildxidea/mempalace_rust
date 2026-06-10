# Phase 0 Audit Report — Post-Merge Gap Matrix

**Date:** 2026-06-10
**Plan:** docs/plan/MASTER_PLAN_AGENTMEMORY_PARITY_v0.3.md
**Epic:** mr-mp-105-agentmemory-post-merge-24g2

## Summary

| Status | Count |
|--------|-------|
| ✅ Present in mempalace_rust | 2 |
| ❌ Missing — needs implementation | 10 |
| **Total audited** | **12** |

## Updated Scope

**Major win:** Connect adapters (Q11) already has **17 adapters** — plan's "10 missing adapters" gap in Phase 5 is **eliminated**. Cross-provider fallback (Q5) already works. The plan was over-estimating; this cuts ~40h off the critical path.

### ❌ AGENT_SCOPE isolation (Q1)
**Evidence:** `crates/core/src/config.rs:269` defines `pub agent_scope: Option<String>` but it is never read or enforced. `tool_recall` at `mcp_server.rs:5383` calls `db.query_sync()` with no agent_id filtering. `tool_smart_search` at `mcp_server.rs:5723` calls `db.hybrid_search()` with no agent_id filtering. `rest_api.rs:261/276` search handlers delegate to MCP dispatch with no agent filtering.
**Action:** Implement MEMPALACE_AGENT_SCOPE=isolated filtering.

### ❌ GET /sessions summary join (Q2)
**Evidence:** `tool_sessions` at `mcp_server.rs:5219` fetches drawers with room='session' and returns session_id, content (truncated 200 chars), created_at. No KV.summaries join.
**Action:** Extend tool_sessions to join/retrieve summaries.

### ❌ Graph query pagination (Q3)
**Evidence:** `tool_kg_query` at `mcp_server.rs:1874` accepts only entity, as_of, tt_as_of, direction — no limit, offset, totalNodes, truncated.
**Action:** Add pagination to kg_query.

### ❌ Graph snapshot/reset (Q4)
**Evidence:** No GraphSnapshot struct, no `/graph/snapshot-rebuild`, no `/graph/reset`, no topDegrees, resetAt, fromSnapshot, warning.
**Action:** Implement GraphSnapshot + both endpoints.

### ❌ Q6: Markdown XML fence stripping
**Evidence:** `parse_summary_xml()` in `summarize.rs` uses raw regex on LLM response — no ```xml fence stripping.
**Action:** Add fence detection + stripping before XML parsing.

### ❌ Q7: Obsidian-export null-record hardening
**Evidence:** `obsidian_export.rs` has `memory_to_obsidian_md()` and `observation_to_obsidian_md()` with no null-record checking.
**Action:** Add null-record validation before exporting.

### ❌ Q8: pi integration tool_input/tool_output fix
**Evidence:** `integrations/pi/index.ts` uses `input`/`output` field names, not `tool_input`/`tool_output`.
**Action:** Add fallback to read both field name patterns.

### ❌ Q9: Followup diagnostic
**Evidence:** `smart_search.rs` has no followup-rate counter, no `GET /diagnostics/followup`.
**Action:** Implement followup tracking + endpoint.

### ❌ Q10: Skills system
**Evidence:** 8 flat SKILL.md dirs only — no EXAMPLES.md, no 7 reference skills, no tiered format, no auto-gen.
**Action:** Restructure skills per agentmemory PR #854.

### ❌ Q12: CLI --instance flag
**Evidence:** No `--instance` flag in `crates/cli/src/main.rs` or anywhere.
**Action:** Implement --instance N flag.

### ✅ Q5: Cross-provider fallback — already present
**Evidence:** Each provider reads its own env (ANTHROPIC_MODEL, OPENAI_MODEL, MINIMAX_MODEL).

### ✅ Q11: Connect adapters — already present (17 adapters)
**Evidence:** `all_adapters()` in `connect/mod.rs:80-100` registers 17 adapters including kiro, warp, cline, continue_dev, zed, openhuman, qwen, antigravity, claude_code, copilot_cli, codex, cursor, gemini_cli, windsurf, vscode, amp, droid.

## Revised Implementation Plan (v0.3.0 updated)

| Phase | Area | File:Line | Effort | v0.3.0? |
|-------|------|-----------|--------|---------|
| 1.0 | AGENT_SCOPE isolation | `mcp_server.rs:5383,5723` | 8h | ✅ |
| 1.2 | XML fence stripping | `summarize.rs` | 4h | ✅ |
| 1.3 | Obsidian null-record hardening | `obsidian_export.rs` | 4h | ✅ |
| 1.4 | pi field fix | `integrations/pi/index.ts:269-270` | 2h | ✅ |
| 2.1 | `--instance N` CLI flag | `crates/cli/src/main.rs` | 8h | ✅ |
| 2.2 | Port collision fix | `crates/core/src/mcp_server.rs` | 4h | ✅ |
| 2.3 | Sessions summary join | `mcp_server.rs:5219` | 4h | ✅ |
| 3.1 | Graph query pagination | `mcp_server.rs:1874` | 8h | ✅ |
| 3.2-3.6 | Graph snapshot/reset system | new structs + endpoints | 32h | ✅ |
| 4 | Followup diagnostic | `search/smart_search.rs` + new route | 24h | ✅ |
| 7 | Skills system | `plugin/skills/` | 16h | ✅ |
| — | Sharded index (deferred) | — | 80h | ❌ v0.3.1 |
| — | Adapters (already present!) | — | 0h | ✅ **DONE** |
| — | Cross-provider fallback (already present!) | — | 0h | ✅ **DONE** |

**Revised total:** ~114h (~3 weeks)
