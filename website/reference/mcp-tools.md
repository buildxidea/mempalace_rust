# MCP Tools Reference

The MemPalace MCP server currently exposes **84 tools** (all prefixed `mempalace_`). They cover palace read/write, drawer CRUD, knowledge graph, navigation/tunnels, agent diary, sessions, slots, signals, sentinels, lessons, reflections, working memory, observation hooks, commit correlation, and more.

This page lists the most-used tools with parameter schemas. For the canonical list, see `crates/core/src/mcp_server.rs` (`make_tools()`).

## Palace — Read Tools

### `mempalace_status`

Palace overview: total drawers, wing and room counts, AAAK spec, and memory protocol.

**Parameters:** None

**Returns:** `{ total_drawers, wings, rooms, palace_path, protocol, aaak_dialect }`

---

### `mempalace_health`

Liveness check for the running MCP server. Returns process info and palace connectivity.

**Parameters:** None

**Returns:** `{ status, palace_path, uptime_s, ... }`

---

### `mempalace_list_wings`

List all wings with drawer counts.

**Parameters:** None

**Returns:** `{ wings: { "wing_name": count } }`

---

### `mempalace_list_rooms`

List rooms within a wing (or all rooms if no wing given).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `wing` | string | No | Wing to list rooms for |

**Returns:** `{ wing, rooms: { "room_name": count } }`

---

### `mempalace_get_taxonomy`

Full wing → room → drawer count tree.

**Parameters:** None

**Returns:** `{ taxonomy: { "wing": { "room": count } } }`

---

### `mempalace_search`

Semantic or keyword search. Returns verbatim drawer content with similarity / relevance scores.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | **Yes** | What to search for |
| `limit` | integer | No | Max results (default: 5) |
| `wing` | string | No | Filter by wing |
| `room` | string | No | Filter by room |
| `bm25` | boolean | No | Enable BM25 rerank |
| `fusion_mode` | string | No | `vector`, `ppr`, or `hybrid` |
| `strategy` | string | No | `contains`, `naive`, `bm25`, or `embedding` |

**Returns:** `{ query, filters, results: [{ text, wing, room, source_file, similarity }] }`

---

### `mempalace_smart_search`

AI-reranked search across wings. Same shape as `mempalace_search` but routes through the smart-rerank pipeline.

---

### `mempalace_hybrid_search`

Hybrid keyword + vector + graph search. Useful for queries that mix entity names with descriptive text.

---

### `mempalace_graph_search` / `mempalace_graph_expand`

Search the knowledge graph and expand by N hops. See [Knowledge Graph](#knowledge-graph-tools).

---

### `mempalace_check_duplicate`

Check if content already exists in the palace before filing.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `content` | string | **Yes** | Content to check |
| `threshold` | number | No | Similarity threshold 0–1 (default: 0.85–0.87) |

**Returns:** `{ is_duplicate, matches: [{ id, wing, room, similarity, content }] }`

---

### `mempalace_get_aaak_spec`

Returns the AAAK dialect specification.

**Parameters:** None

**Returns:** `{ aaak_spec: "..." }`

---

## Palace — Write Tools

### `mempalace_add_drawer`

File verbatim content into the palace. Identical content (same deterministic drawer ID) is silently skipped. For similarity-based duplicate detection before filing, use `mempalace_check_duplicate`.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `wing` | string | **Yes** | Wing (project name) |
| `room` | string | **Yes** | Room (aspect: backend, decisions, etc.) |
| `content` | string | **Yes** | Verbatim content to store |
| `source_file` | string | No | Where this came from |
| `added_by` | string | No | Who is filing (default: `"mcp"`) |

**Returns:** `{ success, drawer_id, wing, room }`

---

### `mempalace_delete_drawer`

Delete a drawer by ID. Irreversible.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `drawer_id` | string | **Yes** | ID of the drawer to delete |

**Returns:** `{ success, drawer_id }`

---

### `mempalace_governance_delete`

Governance-gated drawer delete. Adds an audit trail and policy check on top of raw deletion.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `drawer_id` | string | **Yes** | ID of the drawer to delete |
| `reason` | string | No | Reason recorded in the audit log |

---

### `mempalace_compress_file`

Run AAAK compression on the contents of a file (preview only, by default).

---

### `mempalace_obsidian_export`

Export the palace to a Markdown vault at the given output path.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `output_dir` | string | **Yes** | Output directory for the vault |

---

## Knowledge Graph Tools

### `mempalace_kg_query`

Query entity relationships with time filtering.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `entity` | string | **Yes** | Entity to query (e.g. `"Max"`, `"MyProject"`) |
| `as_of` | string | No | Date filter — only facts valid at this date (`YYYY-MM-DD`) |
| `direction` | string | No | `outgoing`, `incoming`, or `both` (default: `both`) |

**Returns:** `{ entity, as_of, facts: [{ direction, subject, predicate, object, valid_from, valid_to, current }], count }`

---

### `mempalace_kg_add`

Add a fact to the knowledge graph.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `subject` | string | **Yes** | The entity doing/being something |
| `predicate` | string | **Yes** | Relationship type |
| `object` | string | **Yes** | The entity receiving/being described |
| `valid_from` | string | No | When the fact became true (`YYYY-MM-DD`) |
| `confidence` | number | No | 0.0–1.0 (default 1.0) |

---

### `mempalace_kg_invalidate`

Mark a fact as ended (sets `valid_to`).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `subject` | string | **Yes** | Subject entity |
| `predicate` | string | **Yes** | Relationship type |
| `object` | string | **Yes** | Object entity |
| `ended` | string | **Yes** | When the fact stopped being true (`YYYY-MM-DD`) |

---

### `mempalace_kg_timeline`

Chronological entity story.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `entity` | string | No | Entity to get timeline for (omit for full timeline) |

---

### `mempalace_kg_stats`

Knowledge graph overview.

**Parameters:** None

**Returns:** `{ entities, triples, current_facts, expired_facts, relationship_types }`

---

### `mempalace_kg_snapshot_rebuild` / `mempalace_kg_reset`

Rebuild the in-memory KG snapshot from disk, or fully reset (destructive — use with care).

---

## Navigation Tools

### `mempalace_traverse`

Walk the palace graph from a room. Find connected ideas across wings.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `start_room` | string | **Yes** | Room to start from |
| `max_hops` | integer | No | How many connections to follow (default: 2) |

**Returns:** `[{ room, wings, halls, count, hop, connected_via }]`

---

### `mempalace_find_tunnels`

Find rooms that bridge two wings.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `wing_a` | string | No | First wing |
| `wing_b` | string | No | Second wing |

---

### `mempalace_list_hallways` / `mempalace_delete_hallway`

List or remove explicit hallways (edges between rooms within a wing).

---

### `mempalace_graph_stats`

Palace graph overview: nodes, tunnels, edges, connectivity.

**Parameters:** None

**Returns:** `{ total_rooms, tunnel_rooms, total_edges, rooms_per_wing, top_tunnels }`

---

## Sessions, Slots & Working Memory

### `mempalace_sessions`

List recent mining sessions, optionally filtered by wing.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `wing` | string | No | Filter by wing |
| `limit` | integer | No | Max results (default 20) |

---

### `mempalace_observe`

Record an observation (typically invoked by `mpr hook` from a lifecycle hook).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `session_id` | string | **Yes** | Session ID |
| `hook_type` | string | **Yes** | `notification`, `stop`, `precompact`, `session_end`, `post_tool_use`, … |
| `project` | string | No | Project name |
| `data` | object | No | Arbitrary JSON payload |

---

### `mempalace_slot_list` / `mempalace_slot_get` / `mempalace_slot_create` / `mempalace_slot_append` / `mempalace_slot_replace` / `mempalace_slot_delete`

Manage named slots — durable per-session scratchpads that survive context compactions. Slots are the recommended place to put TODO lists and in-progress reasoning.

| Operation | Key parameters |
|-----------|---------------|
| `slot_list` | `wing`, `session_id` |
| `slot_get` | `slot_id` |
| `slot_create` | `wing`, `session_id`, `name`, `body` |
| `slot_append` | `slot_id`, `body` |
| `slot_replace` | `slot_id`, `body` |
| `slot_delete` | `slot_id` |

---

### `mempalace_working_memory`

Combined read of slots + recent observations for the current session. Use this on wake-up instead of fetching slots one-by-one.

---

### `mempalace_context_build`

Assemble a context block for the AI prompt. Combines wake-up, slots, and recent memories.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `levels` | integer | No | Context depth (default 3) |
| `wing` | string | No | Restrict to a wing |

---

## Commits & File History

### `mempalace_commits`

Recent git commits correlated with the current session (used by `mpr status` and the palace UI).

---

### `mempalace_commit_lookup`

Look up the commit that introduced a given drawer or file change.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | **Yes** | Repository-relative path |

---

### `mempalace_file_history`

Chronological list of mutations on a file (drawer creates, updates, deletes).

---

## Signals, Sentinels, Routines

### `mempalace_signal_send` / `mempalace_signal_read`

Send and read agent-to-agent signals. Same shape as `mpr signals`.

---

### `mempalace_sentinel_create` / `mempalace_sentinel_trigger` / `mempalace_sentinel_list` / `mempalace_sentinel_delete`

Watchers that fire on a condition (e.g. "wake me when keyword X appears in any new drawer"). Sentinels run in the background engine started by `mpr serve`.

---

### `mempalace_routine_run`

Trigger a named routine (a saved sequence of MCP calls) on demand.

---

## Lessons, Reflections, Insights

### `mempalace_lesson_save` / `mempalace_lesson_recall`

Save and recall reusable lessons the AI has learned. Lessons persist across sessions and are surfaced during wake-up.

---

### `mempalace_reflect`

Run a reflection pass: take recent observations, summarise, and write back as lessons.

---

### `mempalace_insight_list`

List derived insights (aggregations over drawers + KG facts).

---

## Crystallize, Checkpoints, Snapshots

### `mempalace_crystallize`

Promote a slot / observation cluster into a long-lived drawer.

---

### `mempalace_checkpoint` / `mempalace_checkpoint_list` / `mempalace_checkpoint_resolve`

Manage context-window checkpoints — explicit save points the AI can return to after compaction.

---

### `mempalace_snapshot_create`

Create a full memory snapshot. Mirrors the CLI `mpr snapshot --name <name>`.

---

## Action Store & Frontier

### `mempalace_action_create` / `mempalace_action_update`

Track discrete actions (PR opened, file migrated, etc.) for the [frontier queue](#).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `description` | string | **Yes** | Action description |
| `status` | string | No | `pending`, `running`, `completed`, `failed` |

---

### `mempalace_frontier`

Show the frontier queue — pending work items grouped by priority.

---

### `mempalace_next`

Suggest the next action to take based on the frontier queue, recent observations, and the current session.

---

## Lease, Team, Mesh

### `mempalace_lease`

Acquire/release advisory file leases so multiple agents can coordinate without clobbering each other's drawers.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | **Yes** | Glob to lease |
| `mode` | string | No | `acquire` or `release` |
| `ttl_seconds` | integer | No | Lease TTL |

---

### `mempalace_team_share` / `mempalace_team_feed`

Share a drawer with the team wing, or read the shared feed.

---

### `mempalace_mesh_sync`

Sync the mesh with peers. Same shape as `mpr mesh --operation sync`.

---

## Consolidate, Retention, Access

### `mempalace_consolidate`

Run the consolidation pipeline. Mirrors `mpr consolidate`.

---

### `mempalace_retention_score`

Return the retention score for a drawer (used by `mpr forget` and auto-forget).

---

### `mempalace_access_stats`

Return drawer access counts. Useful for tuning what to keep / evict.

---

### `mempalace_enrich`

Enrich a drawer with metadata (entities, topics, embedding refresh).

---

## Mine & Heal

### `mempalace_mine`

Run `mpr mine` from MCP — given a target path, file its content as drawers.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | **Yes** | Directory or file to mine |
| `mode` | string | No | `projects`, `convos`, `auto` |

---

### `mempalace_heal`

Attempt to repair corrupt or unfetchable drawer entries (analogous to `mpr repair scan` + `prune`).

---

### `mempalace_verify`

Run diagnostics on the palace (analogous to `mpr diagnose`).

---

### `mempalace_diagnose`

Deep diagnostics (analogous to `mpr diagnose --deep`).

---

### `mempalace_replay_import`

Replay an earlier import, useful for debugging.

---

## Worktrees & Branches

### `mempalace_detect_worktree` / `mempalace_branch_detect` / `mempalace_branch_sessions` / `mempalace_branch_worktrees`

Detect git worktrees and branch-aware sessions. Used when mining across multi-branch repos.

---

## Claude Bridge

### `mempalace_claude_bridge_sync`

Sync a Claude.ai project with the local palace. See [Claude Code Plugin](/guide/claude-code) for context.

---

## Facets

### `mempalace_facet_tag` / `mempalace_facet_query`

Tag and query drawer facets (per-drawer structured metadata beyond wing/room).

| Operation | Key parameters |
|-----------|---------------|
| `facet_tag` | `drawer_id`, `facet`, `value` |
| `facet_query` | `facet`, `value`, `limit` |

---

## System Tools

### `mempalace_hook_settings`

Get or set auto-save hook behaviour. `silent_save=true` saves directly without MCP-level clutter; `silent_save=false` uses the legacy blocking path. `desktop_toast=true` surfaces a desktop notification when a save completes. Call with no arguments to view the current settings.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `silent_save` | boolean | No | `true` = silent direct save, `false` = blocking MCP calls |
| `desktop_toast` | boolean | No | `true` = show desktop toast via `notify-send` |

---

### `mempalace_reconnect`

Force a reconnect to the palace database. Use this after external scripts or CLI commands modified the palace directly, which can leave the in-memory HNSW index stale.

**Parameters:** None
