---
name: recall
description: Search mempalace for past observations, sessions, and learnings about a topic
argument-hint: "[search query]"
user-invocable: true
---

The user wants to recall past context about: $ARGUMENTS

## Quick start

Call `memory_smart_search` with the user's query (limit 10).

## Why

Retrieve context from past sessions so the agent can answer accurately without hallucinating.

## Workflow

1. Call `memory_smart_search` with `query` from `$ARGUMENTS` and `limit: 10`.
2. Group results by session ID.
3. For each observation, show type, title, and narrative.
4. Highlight observations with `importance >= 7`.
5. If empty, suggest 2-3 alternative search terms.

## Anti-patterns

**WRONG** -- making up observations to fill gaps:

```text
// No results from tool, but agent fabricates "I recall we discussed X"
```

**RIGHT** -- honest empty state with suggestions:

```text
// No results found. Try: "deployment config", "build system", "cli flags"
```

**WRONG** -- ignoring the limit parameter:

```text
// Calling memory_smart_search without limit, getting hundreds of results
```

**RIGHT** -- using explicit limit:

```json
memory_smart_search({"query": "config file parsing", "limit": 10})
```

## MCP tool unavailable

If `memory_smart_search` is not available:

1. Run `/plugin list` in Claude Code, confirm `mempalace` is enabled.
2. Restart Claude Code (`.mcp.json` is only read on startup).
3. Check `/mcp` to see if the `mempalace` MCP server is connected.

> See `_shared/TROUBLESHOOTING.md` and `EXAMPLES.md` for more.
