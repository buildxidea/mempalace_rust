---
name: entities
description: List or inspect people/project entities tracked in the mempalace graph
argument-hint: "[optional name or filter]"
user-invocable: true
---

The user wants entity information: $ARGUMENTS

## Quick start

Use MCP entity tools or `mpr entities` when available; otherwise search for the name and summarize person/project signals.

## Why

Entity context (who/what) improves recall and handoff quality across sessions.

## Workflow

1. If a name is given, look up that entity and related drawers.
2. If none, list top entities with type and mention counts when the API supports it.
3. Prefer structured fields (type, aliases, last seen) over freeform guesswork.

## Anti-patterns

**WRONG** -- inventing people or projects not present in the palace.

**RIGHT** -- report only tool-backed entities; offer to mine/search if empty.
