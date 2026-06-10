# Examples

## Save an important decision

```
User: /remember We decided to use XDG_CONFIG_HOME as the primary config directory with ~/.mempalace as fallback
```

Agent extracts concepts: `["config-path-resolution", "xdg-config-home", "backward-compatibility"]`, files: `["crates/core/src/config.rs"]`. Calls `memory_save`.

Expected output:
```
Saved! Tagged with concepts: config-path-resolution, xdg-config-home, backward-compatibility
Files: crates/core/src/config.rs
```

## Save a bug investigation

```
User: /remember The repair scan bug was caused by a race condition in MinePidGuard. The fix was to use an atomic file lock instead of PID file.
```

Agent extracts concepts: `["repair-scan-bug", "race-condition", "mine-pid-guard", "atomic-file-lock"]`, files: `["crates/core/src/mine_pid_guard.rs"]`.

## Save a code pattern

```
User: /remember To add a new MCP tool, define the handler function in mcp_server.rs, add it to the dispatch table, then add a REST endpoint in rest_api.rs
```

Agent extracts concepts: `["adding-mcp-tools", "mcp-server-dispatch", "rest-api-endpoints"]`, files: `["crates/core/src/mcp_server.rs", "crates/core/src/rest_api.rs"]`.

## Save with no file references

```
User: /remember The embedding model "naive" uses word overlap similarity and doesn't require an external model file
```

Agent extracts concepts: `["embedding-model", "naive-similarity", "word-overlap"]`, files: `[]`.

## Save when MCP is unavailable

If `memory_save` is not available, the skill directs the user to check plugin status, restart Claude Code, and verify MCP connectivity.

## Gotchas

- **Concepts are critical**: The saved memory is only as findable as its concepts. Prefer specific terms over generic ones: `"jwt-refresh-rotation"` beats `"auth"`.
- **Concept formatting**: Concepts are lowercased keyword phrases, not sentences.
- **Content preservation**: The user's original phrasing is preserved as `content`. The agent adds structure via the `concepts` and `files` fields, not by editing the content.
- **No duplicate detection**: The tool doesn't check for duplicates. The same memory can be saved multiple times.
- **File paths**: Use absolute or repo-relative paths. The agent determines what's relevant -- paths that are tangentially related should be omitted.
