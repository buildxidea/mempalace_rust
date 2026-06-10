---
name: handoff
description: Resume the most recent agent session for the current working directory
argument-hint: "[optional cwd override]"
user-invocable: true
---

The user wants to resume work. Optional cwd override: $ARGUMENTS

## Quick start

Call `memory_sessions`, find the most recent session matching the project path, summarize it.

## Why

Pick up where the last session left off without re-reading history from scratch.

## Workflow

1. Determine project path: `$ARGUMENTS` override or current `cwd`.
2. Call `memory_sessions`, find the most recent session whose `cwd` matches (directory-boundary check). Prefer `completed` over `abandoned`.
3. If the session ended on an unanswered question (narrative ending in `?`), surface it first.
4. Summarize: title, key files, key decisions/errors. Call `memory_recall` with session concepts, limit 10.
5. End with a "next step" pointer.

## Anti-patterns

**WRONG** -- raw string prefix match on `cwd`:

```text
// session.cwd.startsWith("/repo") matches unrelated "/repo-staging"
```

**RIGHT** -- directory-boundary check:

```text
// session.cwd == projectPath OR startsWith(projectPath + path.sep) OR vice versa
```

**WRONG** -- ignoring the unanswered question:

```text
// Session ended with "Should we keep the fallback?" but you don't mention it
```

## REST fallback

If MCP tools are unavailable: `GET $MEMPALACE_URL/sessions` and `POST $MEMPALACE_URL/smart_search` with `Authorization: Bearer $MEMPALACE_SECRET`.

> See `_shared/TROUBLESHOOTING.md` and `EXAMPLES.md` for more.
