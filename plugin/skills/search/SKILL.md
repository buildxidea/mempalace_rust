---
name: search
description: Hybrid search over the mempalace corpus (BM25 + vectors + optional graph)
argument-hint: "[query] [--wing name] [--limit N]"
user-invocable: true
---

The user wants to search mempalace for: $ARGUMENTS

## Quick start

Call `mempalace_search` (or `memory_smart_search`) with the query and a small limit.

## Why

Find drawers/observations that match a topic without re-mining or re-reading the tree.

## Workflow

1. Extract query from `$ARGUMENTS`.
2. Optional wing/room filters when the user names a project area.
3. Call search with `limit` 5–15 (default 10).
4. Present top hits with score, wing/room, short snippet, and source path when present.
5. If empty, suggest narrower/broader terms or a different wing.

## Anti-patterns

**WRONG** -- dumping the entire result payload:

```text
// printing raw JSON blobs of every drawer field
```

**RIGHT** -- ranked shortlist with snippets and paths.

**WRONG** -- inventing hits when the tool returns empty.

**RIGHT** -- say no results and propose 2–3 alternate queries.

## MCP tool unavailable

1. CLI fallback: `mpr search "<query>" --limit 10`.
2. REST: `POST $MEMPALACE_URL/search` with bearer token if configured.
