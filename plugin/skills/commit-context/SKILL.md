---
name: commit-context
description: Trace a file, function, or line back to the agent session that produced its current commit
argument-hint: "[file, function, or line]"
user-invocable: true
---

The user wants commit context for: $ARGUMENTS

## Quick start

Run `git blame`/`git log`, get the SHA, call `memory_commit_lookup`.

## Why

Understand why a line of code exists by linking it to the agent session that created it.

## Workflow

1. Run git command based on input:
   - Line range: `git blame -L <start>,<end> <file>`
   - Function name: `git log -L :<function>:<file>`
   - File path only: `git log -n 1 -- <file>`
2. Extract the most recent commit SHA.
3. Call `memory_commit_lookup` with `sha: "<full-sha>"`.
4. Present: commit SHA/branch/author/message, linked session, and key observations.

## Anti-patterns

**WRONG** -- inventing a session for unlinked commits:

```text
// "memory_commit_lookup returned null so I made up a session"
```

**RIGHT** -- stating the fact:

```text
// No linked session found. This commit predates session linking.
```

**WRONG** -- using short SHA:

```text
// memory_commit_lookup expects a full SHA, not the 7-char abbreviation
```

## REST fallback

If MCP is unavailable: `GET $MEMPALACE_URL/commits/{sha}` with `Authorization: Bearer $MEMPALACE_SECRET`.

> See `_shared/TROUBLESHOOTING.md` and `EXAMPLES.md` for more.
