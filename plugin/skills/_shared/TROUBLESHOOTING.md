# Troubleshooting

Common failure modes when running mempalace skills and their fixes.

## "Tool not found" / MCP tool unavailable

**Symptom:** Agent reports the MCP tool (e.g., `memory_smart_search`, `memory_sessions`) is not available.

**Fixes:**

1. Run `/plugin list` in Claude Code and confirm `mempalace` shows as enabled.
2. Restart Claude Code -- the plugin's `.mcp.json` is only read on startup.
3. Run `/mcp` to check whether the `mempalace` MCP server is connected.
4. Verify the mempalace server is running: `mpr serve` (or `mpr --palace <path> serve`).
5. If the issue persists, check that the MCP server process is alive and not crashing on startup.

## Empty results from search

**Symptom:** `memory_smart_search` or `memory_recall` returns zero observations.

**Fixes:**

1. Try a broader query -- use fewer or more generic terms.
2. Check that the palace has data: run `mpr status` in your terminal.
3. If the palace is empty, run `mpr init <dir>` then `mpr mine <dir>` to populate it.
4. For new projects, ensure `mpr init` completed successfully and created the palace.
5. If data exists but search returns nothing, try `mpr repair rebuild` to rebuild the index.

## No session results in recap / session-history / handoff

**Symptom:** Session tools return empty lists.

**Fixes:**

1. Verify sessions have been captured -- the auto-capture hooks must be installed in `.claude/settings.local.json`.
2. Check `~/.mempalace/hook_state/hook.log` for capture activity.
3. Manual sessions can be captured with `mpr mine --mode convos <transcript-dir>`.
4. In recap, the `cwd` filter requires the session's `cwd` to match the project directory. If you moved the repo, sessions won't match.

## REST fallback not working

**Symptom:** MCP tools unavailable and REST fallback also fails.

**Fixes:**

1. Ensure the HTTP server is running (started with `mpr serve --http`).
2. Set `MEMPALACE_URL` to the HTTP server address (default `http://localhost:6969`).
3. Set `MEMPALACE_SECRET` if the server requires authentication.
4. Verify the correct route -- REST routes are bare paths like `/sessions`, `/commits`, `/search`. Do NOT use the old `/mempalace/` prefix.
5. Check which REST endpoints exist by visiting `$MEMPALACE_URL/tools`.

## Forgetting doesn't work

**Symptom:** `memory_governance_delete` reports success but memories reappear.

**Fixes:**

1. The tool deletes by memory ID, not by content. Ensure you're passing the correct `memoryIds` array.
2. After deletion, use `memory_smart_search` with the same query to verify the memories are gone.
3. If the palace uses a snapshot system, snapshots created before the deletion may still contain the data. Create a new snapshot after deletion.

## Handoff selects wrong session

**Symptom:** The skill picks a session from a different project.

**Fixes:**

1. The session is selected by matching `cwd` against the current working directory. Ensure the session's `cwd` is correct.
2. The match uses directory-boundary checks: `session.cwd.startsWith(projectPath + path.sep)` or vice versa. Raw prefix matches are not used.
3. If working in a subdirectory of the project, the parent project directory will match.
4. If no session matches, the skill falls back to the most recent session overall. Pass an explicit `cwd` override as `$ARGUMENTS` to narrow the search.

## Memory was saved but can't be found

**Symptom:** `memory_save` succeeded but search doesn't return it.

**Fixes:**

1. Try searching with the exact concepts you tagged during save.
2. The hybrid search (BM25 + vector + graph) may rank other results higher. Try with `limit: 20` and scan further.
3. Check that the memory wasn't accidentally saved under a different wing.
4. The vector index may need rebuilding after many saves: run `mpr repair rebuild`.

## Commit lookup returns no session

**Symptom:** `memory_commit_lookup` returns an empty `commit: null` body.

**Fixes:**

1. Commits that predate session linking are not associated with any session.
2. Ensure the agent hooks (session-start, post-tool-use) are active to capture commit-session links going forward.
3. Use `git log` and `git show` for unlinked commits -- the skill presents the git metadata directly.
