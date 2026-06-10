# Examples

## Recap the last 10 sessions (default)

```
User: /recap
```

Agent parses empty `$ARGUMENTS`, defaults to `last 10`. Calls `memory_sessions` MCP tool, filters by current project `cwd`, sorts by `startedAt` descending, groups by date.

Expected output:
```
2026-06-10
  a1b2c3d4 — "Config restructure" — 12 observations — completed
    > Decided to move XDG path resolution to Config::config_dir()
    > Noted that ~/.mempalace fallback must remain for backward compat
  e5f6g7h8 — "Skills audit" — 8 observations — completed
    > Found 8 existing skills with flat SKILL.md only
    > Missing EXAMPLES.md and _shared/TROUBLESHOOTING.md

2026-06-09
  i9j0k1l2 — "REST route cleanup" — 15 observations — completed
    > Changed fallback routes from /mempalace/* to /sessions and /smart_search

Total: 3 sessions across 2 days, 35 observations.
```

## Recap for today

```
User: /recap today
```

Agent parses `today`, filters sessions started on the current local date, groups by date, shows highlights.

## Recap for a specific number

```
User: /recap 5
```

Agent treats bare number as `last 5`, fetches the 5 most recent sessions, applies the same grouping and formatting.

## Recap with no matching sessions

If no sessions match the window, the skill says so directly:
```
No sessions found for the requested time window (last 10).
```

## Recap with MCP unavailable

Fallback to HTTP: `GET $MEMPALACE_URL/sessions` then `POST $MEMPALACE_URL/smart_search` with `Authorization: Bearer $MEMPALACE_SECRET`.

## Gotchas

- **`/recap` vs `/session-history`**: Recap filters by the current project's `cwd`; session-history shows all projects.
- **Window parsing**: `last N` expects a number after "last". `today` and `this week` are case-insensitive.
- **CWD match**: Sessions from a different directory path won't match even if they're about the same project.
- **MCP unavailable path**: The HTTP fallback uses `/sessions` and `/smart_search`. Make sure the REST server is running with `mpr serve --http`.
- **Importance 7 highlighted**: Only observations with `importance >= 7` from the per-session `memory_recall` calls are indented. Sessions with no high-importance observations show no highlights.
