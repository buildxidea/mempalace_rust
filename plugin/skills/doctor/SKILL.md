---
name: doctor
description: Diagnose mempalace setup (config, palace path, MCP, FTS/index health)
argument-hint: "[optional focus: mcp|config|palace]"
user-invocable: true
---

The user wants a mempalace health check: $ARGUMENTS

## Quick start

Run `mpr doctor` if available; otherwise check config path, palace path, and MCP connectivity.

## Why

Unblock "search returns nothing" / "MCP not connected" / "palace missing" failures quickly.

## Workflow

1. Check config: `~/.mempalace/config.json` or XDG path; print resolved palace path.
2. Check palace exists and is readable; note drawer count if status works.
3. Check MCP: `/mcp` or env `MEMPALACE_URL` / secret when HTTP transport is used.
4. If FTS/index errors appear, suggest `mpr repair` paths documented in project.
5. Summarize green/yellow/red with one next action each.

## Anti-patterns

**WRONG** -- only restating the user error without checking paths.

**RIGHT** -- verify config → palace → MCP → search smoke in order.
