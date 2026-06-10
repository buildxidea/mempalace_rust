# Examples

## Forget specific observations by search

```
User: /forget test credentials for staging server
```

Step 1: Agent calls `memory_smart_search` with `query: "test credentials for staging server"`, `limit: 20`. Results look like:

```
Found 3 matching observations:
  id: obs_001 — "Staging DB credentials" (important: 5)
  id: obs_002 — "Test API key for staging" (important: 3)
  id: obs_003 — "Staging server hostname" (important: 2)
```

Step 2: Agent asks: "Found 3 observations. Delete all 3? [y/N]"

Step 3 (after user confirms): Agent calls `memory_governance_delete` with `memoryIds: ["obs_001", "obs_002", "obs_003"]`.

Expected output: "Deleted 3 observations."

## Forget an entire session's observations

```
User: /forget session abc123de
```

Agent searches, finds all observations in that session. Prompts user: "Found 12 observations in session abc123de. Delete all? [y/N]"

After confirmation, passes all 12 IDs to `memory_governance_delete`.

## Forget with no matches

```
User: /forget entirely made up thing that was never mentioned
```

Agent searches, returns no observations: "No matching observations found for 'entirely made up thing that was never mentioned'. Nothing to delete."

## Forget without confirmation -- blocked

```
User: /forget everything
```

Agent searches, finds observations, BUT will NOT delete until the user explicitly confirms. The skill always requires explicit confirmation before deletion, even for a single match.

## Gotchas

- **Destructive operation**: The skill always requires explicit user confirmation before deleting. Never auto-confirm.
- **Delete by ID only**: The standalone MCP tool does not accept a bare `sessionId`. You must collect all memory IDs in that session and pass them as an array.
- **No undo**: Once deleted, observations cannot be recovered. There is no trash or recycle bin.
- **Session deletion**: To drop a whole session's data, collect every memory ID from the search results. There is no bulk-delete-by-session endpoint.
- **Search limit**: The search uses `limit: 20`. If a session has more than 20 observations, the extra ones won't be shown or deleted in a single operation.
