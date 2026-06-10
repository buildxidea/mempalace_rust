# Examples

## Quick search with `recall`

```
User: /recall how does the config system work
```

Agent calls `memory_smart_search` with `query: "how does the config system work"`, `limit: 10`.

Expected output:
```
Session: config-setup (abc123de)
  Type: decision — "Adopted XDG_CONFIG_HOME with platform fallback"
  Type: code — "Config struct with palace_path, embedding_model, weight knobs"
  Type: conversation — "Discussed XDG vs ~/.mempalace paths"
  Importance 8: "The config file must be parsed before the palace is opened"
```

## Debug empty results

```
User: /recall memory corruption handling
```

If `memory_smart_search` returns zero results:

```
No results found for "memory corruption handling". Try:
  - "corrupt drawer" (scoped to palace repair)
  - "palace corruption" (mentions repair behavior)
  - "repair scan" (tool logic for corrupt IDs)
```

## Recall with specific project context

Agent calls `memory_smart_search` with `query: "deployment configuration"`, `limit: 10`.

The results are grouped by session. Each observation shows type, title, and narrative. Observations with `importance >= 7` are highlighted.

## Recalling with no palace data

If the palace is empty, the skill returns: "No observations found. This palace has no data yet -- run `mpr init` and `mpr mine` to populate it."

## MCP tool unavailable

If `memory_smart_search` is not available, the skill tells the user to check plugin status, restart Claude Code, and verify MCP connectivity.

## Gotchas

- **Hallucination trap**: Only present tool results. Never make up observations.
- **Limit default**: The tool uses `limit: 10`. For broader searches, the user should call `/recall` with more specific terms or use the raw MCP tool directly.
- **Session grouping**: Results are grouped by session ID. If a session has no observations, it won't appear.
- **Importance threshold**: Only observations with `importance >= 7` are highlighted. Lower values are still listed without the highlight.
