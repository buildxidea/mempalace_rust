---
name: mine
description: Mine a project directory into the mempalace corpus (drawers, rooms, wings)
argument-hint: "[path] [--wing name]"
user-invocable: true
---

The user wants to mine project content into mempalace: $ARGUMENTS

## Quick start

Run `mpr mine <path>` (or call MCP `mempalace_mine` when available). Prefer CLI when path is large.

## Why

Populate the palace with searchable drawers so future `recall` / search has real corpus.

## Workflow

1. Parse path from `$ARGUMENTS` (default: current project root).
2. Optional wing name: `--wing <name>` or infer from directory basename.
3. Prefer CLI: `mpr mine <path> --wing <wing>` for progress and lock safety.
4. If MCP `mempalace_mine` is available and path is small, call it with `path`, optional `wing`.
5. Report files scanned / drawers filed from tool or CLI output.
6. Suggest a follow-up search query to verify content landed.

## Anti-patterns

**WRONG** -- mining `node_modules` / `.git` / build dirs without excludes:

```text
// mpr mine . with no skip of vendor trees
```

**RIGHT** -- rely on default skip dirs, or pass excludes if the CLI supports them:

```text
// mpr mine ./src --wing myapp
```

**WRONG** -- mining secrets (`.env`, credentials):

```text
// mining paths that contain private keys or tokens
```

**RIGHT** -- mine source/docs only; keep secrets out of the palace.

## MCP tool unavailable

If neither CLI nor MCP mine is available:

1. Confirm `mpr` is on PATH (`mpr --version`).
2. Check palace init: `mpr init <dir>` first if no palace exists.
3. Check `/mcp` for mempalace connection.

> See `_shared/TROUBLESHOOTING.md` for more.
