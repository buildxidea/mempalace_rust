# Examples

## Show recent sessions

```
User: /session-history
```

Agent calls `memory_sessions` MCP tool with `limit: 20`. Renders reverse-chronological timeline.

Expected output:
```
a1b2c3d4  mempalace_rust  2026-06-10 14:32  completed
  Config restructure (12 observations)
  > decision: "Adopted XDG_CONFIG_HOME"
  > decision: "Keep ~/.mempalace fallback"

e5f6g7h8  mempalace_rust  2026-06-10 10:00  completed
  Skills audit (8 observations)
  > observation: "Found 8 skills with flat SKILL.md"
  > observation: "Missing EXAMPLES.md"

i9j0k1l2  mempalace_rust/plugin  2026-06-09 16:45  abandoned
  Hook debugging (3 observations)
```

## Sessions with summaries

When sessions have summaries, the key decisions from the summary are surfaced:
```
a1b2c3d4  mempalace_rust  2026-06-10 14:32  completed
  Title: Config restructure
  Key decisions:
    - Moved config resolution to dedicated module
    - Added XDG support with backward compat
```

## No sessions yet

```
User: /session-history
```

If `memory_sessions` returns empty results:
```
No sessions found. Sessions are created by the auto-capture hooks when working in this project.
```

## MCP tool unavailable

If `memory_sessions` is not available, the skill tells the user to check plugin status, restart Claude Code, and verify MCP connectivity.

## Gotchas

- **Cross-project**: Unlike recap, session-history shows sessions from all projects, not filtered by `cwd`.
- **Limit default**: The tool uses `limit: 20`. For more history, the user can call the raw MCP tool directly.
- **Status indicators**: Sessions are shown as `completed`, `abandoned`, or `running`.
- **Summary availability**: Key decisions from summaries are only shown when the session has been summarized (auto-consolidation must be enabled).
- **No fabrication**: Only displays what `memory_sessions` returns. Never invent sessions.
