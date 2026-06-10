---
name: mempalace-mcp-tools
description: Reference table of all MCP tools provided by the mempalace server
---

The mempalace MCP server exposes 81 tools. Each canonical `mempalace_*` name has a `memory_*` alias for backward compatibility.

## Core tools

| Canonical name | Alias | Description |
|---------------|-------|-------------|
| `mempalace_status` | `memory_status` | Palace overview: total drawers, wing/room counts |
| `mempalace_list_wings` | `memory_list_wings` | List all wings with drawer counts |
| `mempalace_list_rooms` | `memory_list_rooms` | List rooms within a wing |
| `mempalace_get_taxonomy` | `memory_get_taxonomy` | Full taxonomy: wing -> room -> drawer count |
| `mempalace_search` | `memory_search` | Semantic search with metadata filters |
| `mempalace_smart_search` | `memory_smart_search` | Hybrid BM25 + vector + graph search |
| `mempalace_hybrid_search` | -- | BM25 + vector fusion search |
| `mempalace_check_duplicate` | `memory_check_duplicate` | Check if content exists before filing |
| `mempalace_add_drawer` | `memory_add` | File verbatim content into palace |
| `mempalace_delete_drawer` | `memory_delete` | Delete by drawer ID |

## Knowledge graph tools

| Canonical name | Description |
|---------------|-------------|
| `mempalace_kg_query` | Query entity relationships with temporal filters |
| `mempalace_kg_add` | Add fact: subject -> predicate -> object |
| `mempalace_kg_invalidate` | Mark fact as no longer true |
| `mempalace_kg_timeline` | Chronological timeline of facts |
| `mempalace_kg_stats` | KG overview: entities, triples, relationship types |
| `mempalace_traverse` | Walk palace graph from a room |
| `mempalace_find_tunnels` | Find rooms bridging two wings |
| `mempalace_graph_search` | Graph-aware search |
| `mempalace_graph_expand` | Expand from a node in the graph |
| `mempalace_graph_stats` | Graph overview: rooms, tunnels, edges |

## Diary tools

| Canonical name | Description |
|---------------|-------------|
| `mempalace_diary_write` | Write diary entry in AAAK format |
| `mempalace_diary_read` | Read recent diary entries |

## Slots tools

| Canonical name | Description |
|---------------|-------------|
| `mempalace_slot_list` | List all slots |
| `mempalace_slot_get` | Get slot by ID |
| `mempalace_slot_create` | Create a new slot |
| `mempalace_slot_append` | Append to slot |
| `mempalace_slot_replace` | Replace slot content |
| `mempalace_slot_delete` | Delete slot |

## Session and commit tools

| Canonical name | Description |
|---------------|-------------|
| `mempalace_sessions` | List agent sessions |
| `mempalace_commits` | List agent-linked commits |
| `mempalace_commit_lookup` | Look up session by commit SHA |

## Governance tools

| Canonical name | Description |
|---------------|-------------|
| `mempalace_governance_delete` | Delete by policy with audit trail |
| `mempalace_heal` | Auto-fix blocked actions and expired leases |
| `mempalace_verify` | Verify memory by tracing citation chain |

## Action / frontier tools

`mempalace_action_create`, `mempalace_action_update`, `mempalace_frontier`, `mempalace_next`, `mempalace_lease`, `mempalace_routine_run`, `mempalace_signal_send`, `mempalace_signal_read`

## Sentinel / checkpoint tools

`mempalace_sentinel_create`, `mempalace_sentinel_trigger`, `mempalace_sentinel_list`, `mempalace_sentinel_delete`, `mempalace_checkpoint`, `mempalace_checkpoint_list`, `mempalace_checkpoint_resolve`

## Smart feature tools

`mempalace_sketch_create`, `mempalace_sketch_promote`, `mempalace_crystallize`, `mempalace_diagnose`, `mempalace_facet_tag`, `mempalace_facet_query`, `mempalace_lesson_save`, `mempalace_lesson_recall`, `mempalace_reflect`, `mempalace_insight_list`, `mempalace_consolidate`, `mempalace_enrich`, `mempalace_flow_compress`, `mempalace_cascade_update`, `mempalace_context_build`, `mempalace_retention_score`, `mempalace_access_stats`, `mempalace_working_memory`, `mempalace_file_history`, `mempalace_snapshot_create`, `mempalace_team_share`, `mempalace_team_feed`, `mempalace_mesh_sync`, `mempalace_branch_detect`, `mempalace_branch_sessions`, `mempalace_branch_worktrees`, `mempalace_replay_import`, `mempalace_detect_worktree`, `mempalace_obsidian_export`, `mempalace_compress_file`, `mempalace_claude_bridge_sync`
