# Examples

## List recent agent-linked commits

```
User: /commit-history
```

Agent calls `memory_commits` MCP tool with default params (limit 100). Renders reverse-chronological list.

Expected output:
```
abc123d  main  2026-06-10  feat: add XDG config directory resolution
  Session: a1b2c3d4 (12 obs)
def456g  main  2026-06-09  fix: correct MinePidGuard race condition
  Session: e5f6g7h8 (8 obs)
ghi789h  main  2026-06-08  chore: update dependencies
  Session: i9j0k1l2 (5 obs), Files: 3
...
Total: 15 commits across 3 branches.
```

## Filter by branch

```
User: /commit-history branch=feature/skills-restructure
```

Parses `branch` token, passes to `memory_commits`. Output shows only commits on that branch.

## Custom limit

```
User: /commit-history limit=5
```

Returns only 5 commits (max 500). Bare numeric:
```
User: /commit-history 5
```

Same result as `limit=5`.

## Filter by branch and repo

```
User: /commit-history branch=main repo=github.com/quangdang46/mempalace
```

Both filters passed to `memory_commits`. The URL-encoded HTTP fallback handles special characters in repo URLs.

## Empty results

```
User: /commit-history branch=nonexistent-branch
```

"No commits matched your filters. Try removing the branch or repo filter for broader results."

## Gotchas

- **No invention**: Only display what the MCP tool returns. Never fabricate commits.
- **URL encoding on fallback**: The HTTP fallback uses `URLSearchParams` or `encodeURIComponent` to handle `?`, `&`, `#` in repo URLs.
- **Default limit 100**: The maximum is 500. For very large branch histories, set an explicit limit.
- **Link-dep link**: Only commits made while agent hooks were active will have linked sessions. Old commits won't appear here.
- **File count**: Only shown when the `files` field is present in the response.
