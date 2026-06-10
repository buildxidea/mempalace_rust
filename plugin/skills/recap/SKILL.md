---
name: recap
description: Summarize the last N agent sessions for the current project, grouped by date
argument-hint: "[last N | today | this week]"
user-invocable: true
---

The user wants a recap. Time window args: $ARGUMENTS

## Quick start

Call `memory_sessions`, filter by project `cwd`, apply time window, group by date.

## Why

Get a bird's-eye view of recent work without reading every session log.

## Workflow

1. Parse `$ARGUMENTS`: `today`, `this week`, `last N`, bare number, empty (default `last 10`).
2. Call `memory_sessions` MCP tool, filter by current project `cwd`.
3. Apply time window, sort by `startedAt` descending.
4. Group by calendar date (YYYY-MM-DD).
5. For each date, list sessions with id, title/first prompt, observation count, status.
6. Indent 2-3 highlight observations per session (importance >= 7).

## Anti-patterns

**WRONG** -- inventing sessions when results are empty:

```text
// Just say "No sessions found for that window" instead of fabricating
```

**RIGHT** -- honest response:

```text
// "No sessions found for the requested time window (last 10)."
```

**WRONG** -- loose `cwd` prefix match across unrelated repos:

```text
// Using raw prefix match: session.cwd.startsWith("/repo") matches "/repo-staging"
```

## REST fallback

If MCP tools are unavailable: `GET $MEMPALACE_URL/sessions` and `POST $MEMPALACE_URL/smart_search` with `Authorization: Bearer $MEMPALACE_SECRET`.

> See `_shared/TROUBLESHOOTING.md` and `EXAMPLES.md` for more.
