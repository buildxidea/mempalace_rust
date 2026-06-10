---
name: forget
description: Delete specific observations or sessions from mempalace
argument-hint: "[what to forget - session ID, file path, or search term]"
user-invocable: true
---

The user wants to remove data from mempalace: $ARGUMENTS

**IMPORTANT**: This is a destructive operation. Always confirm with the user before deleting.

## Quick start

Search with `memory_smart_search`, show matches, confirm with user, then call `memory_governance_delete`.

## Why

Remove stale, incorrect, or private data from the palace.

## Workflow

1. Call `memory_smart_search` with the user's input as `query`, `limit: 20`.
2. Show the user what was found (IDs, titles) and ask for explicit confirmation.
3. On confirmation, call `memory_governance_delete` with `memoryIds: [<id>, ...]`.
4. To drop a whole session, collect all memory IDs from the search results.
5. Confirm the deletion count back to the user.

## Anti-patterns

**WRONG** -- deleting without confirmation:

```text
// Never auto-confirm. Always wait for explicit user approval.
```

**RIGHT** -- confirming first:

```text
// "Found 3 observations. Delete all 3? [y/N]"
```

**WRONG** -- using bare `sessionId`:

```text
// The MCP tool does not accept sessionId. Collect all memory IDs instead.
```

## MCP tool unavailable

If MCP tools are unavailable:

1. Run `/plugin list` in Claude Code, confirm `mempalace` is enabled.
2. Restart Claude Code (`.mcp.json` is only read on startup).
3. Check `/mcp` to see if the `mempalace` MCP server is connected.

> See `_shared/TROUBLESHOOTING.md` and `EXAMPLES.md` for more.
