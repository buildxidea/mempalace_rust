---
name: remember
description: Explicitly save an insight, decision, or learning to mempalace long-term storage
argument-hint: "[what to remember]"
user-invocable: true
---

The user wants to save this to long-term memory: $ARGUMENTS

## Quick start

Call `memory_save` with `content`, `concepts`, and `files`.

## Why

Preserve knowledge across sessions so it can be retrieved later via `recall`.

## Workflow

1. Analyze the user's input: extract the core insight, decision, or fact.
2. Extract 2-5 searchable `concepts` (lowercased keyword phrases, specific over generic).
3. Extract relevant `files` (absolute or repo-relative paths).
4. Call `memory_save` with `content`, `concepts`, `files`.
5. Confirm to the user with the concepts you tagged.

## Anti-patterns

**WRONG** -- generic concepts that won't match future searches:

```json
{"concepts": ["auth", "code", "bug"]}
```

**RIGHT** -- specific, searchable concepts:

```json
{"concepts": ["jwt-refresh-rotation", "token-expiry-handler"]}
```

**WRONG** -- editing the user's phrasing in `content`:

```text
// Paraphrasing changes the meaning. Preserve the user's exact words.
```

**RIGHT** -- preserving original phrasing:

```json
{"content": "We decided to use XDG_CONFIG_HOME as the primary config directory..."}
```

## MCP tool unavailable

If `memory_save` is not available:

1. Run `/plugin list` in Claude Code, confirm `mempalace` is enabled.
2. Restart Claude Code (`.mcp.json` is only read on startup).
3. Check `/mcp` to see if the `mempalace` MCP server is connected.

> See `_shared/TROUBLESHOOTING.md` and `EXAMPLES.md` for more.
