---
name: status
description: Show mempalace palace status (drawers, wings, rooms, health)
argument-hint: "[optional palace path]"
user-invocable: true
---

The user wants palace status: $ARGUMENTS

## Quick start

Call `mempalace_status` / `memory_status` or run `mpr status`.

## Why

Confirm the palace is initialized, healthy, and roughly how much content it holds before mining or searching.

## Workflow

1. Resolve palace path from `$ARGUMENTS`, `MEMPALACE_PALACE_PATH`, or default config.
2. Call status tool/CLI.
3. Report: path, drawer count, wing/room counts, embedding backend if shown, any warnings.
4. If missing palace, suggest `mpr init` then `mpr mine`.

## Anti-patterns

**WRONG** -- treating a missing palace as empty success.

**RIGHT** -- surface "palace not found" and the init command.
