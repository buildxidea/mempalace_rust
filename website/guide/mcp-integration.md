# MCP Integration

MemPalace exposes **84 tools** through the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/), giving any MCP-compatible AI full read/write access to your palace. All tools use the `mempalace_` prefix.

## Setup

### Setup Helper

MemPalace includes a setup helper that prints the exact configuration commands for your environment:

```bash
mpr mcp
```

Typical output:

```
MemPalace MCP quick setup:
  claude mcp add mpr -- mpr serve

Run the server directly:
  mpr serve
```

### Manual Connection

```bash
claude mcp add mpr -- mpr serve
```

### With Custom Palace Path

```bash
claude mcp add mpr -- mpr --palace /path/to/palace serve
```

### HTTP Transport

MemPalace also supports an HTTP REST API (useful for non-MCP clients like the Hermes plugin). Build with `--features http-server`:

```bash
mpr serve --http                 # default port 3111
mpr serve --http --port 4111     # custom port
mpr serve --http --read-only     # block mutations
```

## Compatible Tools

MemPalace works with any tool that supports MCP:

- **Claude Code** — native MCP support, see [Claude Code Plugin](/guide/claude-code)
- **Codex CLI** — native MCP support
- **Cursor** — native MCP support
- **Windsurf** — native MCP support
- **VS Code** — via Continue / Cline extensions
- **Gemini CLI** — see [Gemini CLI guide](/guide/gemini-cli)
- **OpenCode** — native MCP support
- **Amp / Droid** — auto-detected by the installer
- **OpenClaw** — see [OpenClaw Skill](/guide/openclaw)
- **Warp / Kiro / Cline / Zed / Qwen / Antigravity** — wire up with `mpr connect <adapter>`

## Memory Protocol

When the AI first calls `mempalace_status`, it receives the **Memory Protocol** — a behavior guide that teaches it to:

1. **On wake-up**: Call `mempalace_status` to load the palace overview
2. **Before responding** about any person, project, or past event: search first, never guess
3. **If unsure**: Say "let me check" and query the palace
4. **After each session**: Write diary entries to record what happened
5. **When facts change**: Invalidate old facts, add new ones

This protocol is what turns storage into memory — the AI knows to verify before speaking.

## Tool Overview

The full tool reference is in [MCP Tools Reference](/reference/mcp-tools). A quick tour:

### Palace (read)

| Tool | What |
|------|------|
| `mempalace_status` | Palace overview + AAAK spec + memory protocol |
| `mempalace_health` | Liveness check + uptime |
| `mempalace_list_wings` | Wings with counts |
| `mempalace_list_rooms` | Rooms within a wing |
| `mempalace_get_taxonomy` | Full wing → room → count tree |
| `mempalace_search` | Search with wing/room filters |
| `mempalace_smart_search` | AI-reranked search |
| `mempalace_hybrid_search` | Hybrid keyword + vector + graph search |
| `mempalace_check_duplicate` | Check before filing |
| `mempalace_get_aaak_spec` | AAAK dialect reference |

### Palace (write)

| Tool | What |
|------|------|
| `mempalace_add_drawer` | File verbatim content |
| `mempalace_delete_drawer` | Remove by ID |
| `mempalace_governance_delete` | Audited delete |
| `mempalace_obsidian_export` | Export to an Obsidian vault |

### Knowledge Graph

| Tool | What |
|------|------|
| `mempalace_kg_query` | Entity relationships with time filtering |
| `mempalace_kg_add` | Add facts |
| `mempalace_kg_invalidate` | Mark facts as ended |
| `mempalace_kg_timeline` | Chronological entity story |
| `mempalace_kg_stats` | Graph overview |
| `mempalace_kg_snapshot_rebuild` / `mempalace_kg_reset` | Rebuild or reset |

### Navigation

| Tool | What |
|------|------|
| `mempalace_traverse` | Walk the graph from a room across wings |
| `mempalace_find_tunnels` | Find rooms bridging two wings |
| `mempalace_graph_stats` | Graph connectivity overview |
| `mempalace_list_hallways` / `mempalace_delete_hallway` | Manage explicit edges |

### Sessions, Slots & Working Memory

| Tool | What |
|------|------|
| `mempalace_sessions` | List recent mining sessions |
| `mempalace_observe` | Record a hook observation |
| `mempalace_slot_*` | Manage named slots (TODO lists, scratchpads) |
| `mempalace_working_memory` | Bundled slots + recent observations |
| `mempalace_context_build` | Assemble prompt context |

### Frontier, Lessons & Reflections

| Tool | What |
|------|------|
| `mempalace_frontier` | Pending work queue |
| `mempalace_next` | Suggested next action |
| `mempalace_action_create` / `mempalace_action_update` | Manage action items |
| `mempalace_lesson_save` / `mempalace_lesson_recall` | Persistent lessons |
| `mempalace_reflect` | Reflection pass over recent observations |
| `mempalace_insight_list` | Derived insights |

### Commits & File History

| Tool | What |
|------|------|
| `mempalace_commits` | Recent commits |
| `mempalace_commit_lookup` | Commit that introduced a path |
| `mempalace_file_history` | Mutations on a file |

For the canonical list (84 tools), see [MCP Tools Reference](/reference/mcp-tools).
