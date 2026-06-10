---
name: session-history
description: Show what happened in recent past sessions on this project
user-invocable: true
---

Fetch recent session history using the `memory_sessions` MCP tool with `limit: 20`.

## Quick start

Call `memory_sessions` with `limit: 20`, render a reverse-chronological timeline.

## Why

Get a quick overview of recent work across all projects without digging into individual sessions.

## Workflow

1. Call `memory_sessions` with `limit: 20`.
2. Present sessions in reverse chronological order.
3. For each session: id (first 8 chars), project, start time, status.
4. For sessions with observations, show type + title of key highlights.
5. Note total observation count per session.
6. If a summary exists, surface the title and key decisions.

## Anti-patterns

**WRONG** -- filtering by project:

```text
// session-history shows ALL projects. Use recap for project-scoped view.
```

**RIGHT** -- showing all sessions without project filter:

```text
// Session list across all projects, clearly labeled by project name.
```

**WRONG** -- inventing sessions:

```text
// No sessions? Say so. Don't fabricate.
```

## MCP tool unavailable

If `memory_sessions` is not available:

1. Run `/plugin list` in Claude Code, confirm `mempalace` is enabled.
2. Restart Claude Code (`.mcp.json` is only read on startup).
3. Check `/mcp` to see if the `mempalace` MCP server is connected.

> See `_shared/TROUBLESHOOTING.md` and `EXAMPLES.md` for more.
