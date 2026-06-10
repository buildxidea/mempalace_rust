---
name: commit-history
description: List recent git commits linked to agent sessions, optionally filtered by branch or repo
argument-hint: "[branch=... repo=... limit=...]"
user-invocable: true
---

The user wants a list of agent-linked commits. Filter args: $ARGUMENTS

## Quick start

Call `memory_commits` MCP tool with parsed filters, render a reverse-chronological list.

## Why

See what an agent has shipped across branches and repos, with links back to the sessions.

## Workflow

1. Parse `$ARGUMENTS` for `branch=<name>`, `repo=<url-or-fragment>`, `limit=<n>` tokens. Bare number = limit.
2. Defaults: no branch filter, no repo filter, limit 100 (max 500).
3. Call `memory_commits` with the parsed filters.
4. Render as reverse-chronological list with SHA, branch, timestamp, message, linked session(s).

## Anti-patterns

**WRONG** -- inventing commits when empty:

```text
// No results is fine. Say so and suggest dropping filters.
```

**RIGHT** -- graceful empty state:

```text
// No commits matched. Try removing the branch or repo filter.
```

**WRONG** -- raw string interpolation in URL fallback:

```text
// Building URL with string concat: `${base}/mempalace/commits?repo=${repo}`
// Breaks when repo URL contains ? or &
```

**RIGHT** -- URL encoding:

```text
// Use URLSearchParams or encodeURIComponent on each parameter.
```

## REST fallback

If MCP is unavailable: build `GET $MEMPALACE_URL/commits` with URL-encoded query params.

> See `_shared/TROUBLESHOOTING.md` and `EXAMPLES.md` for more.
