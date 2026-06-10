---
name: mempalace-agents
description: List of supported AI coding agents and their mpr connect commands
---

Mempalace can integrate with 17 AI coding agents via `mpr connect <adapter>`. Run `mpr connect` with no arguments to list installed agents.

## Adapter reference

| Agent | Connect command | Config path |
|-------|----------------|-------------|
| kiro | `mpr connect kiro` | `~/.kiro/settings/mcp.json` |
| warp | `mpr connect warp` | `~/.warp/.mcp.json` |
| cline | `mpr connect cline` | `~/.cline/mcp.json` |
| continue | `mpr connect continue` | `~/.continue/config.json` |
| zed | `mpr connect zed` | `~/.config/zed/settings.json` |
| openhuman | `mpr connect openhuman` | `~/.openhuman/mcp.json` |
| qwen | `mpr connect qwen` | `~/.qwen/settings.json` |
| antigravity | `mpr connect antigravity` | `~/.config/Antigravity/User/mcp_config.json` |
| claude-code | `mpr connect claude-code` | `.claude/settings.json` (per-project) |
| copilot-cli | `mpr connect copilot-cli` | `~/.config/github-copilot` |
| codex | `mpr connect codex` | `.codex/settings.json` (per-project) |
| cursor | `mpr connect cursor` | `.cursor/mcp.json` (per-project) |
| gemini-cli | `mpr connect gemini-cli` | `~/.gemini/settings.json` |
| windsurf | `mpr connect windsurf` | `.windsurf/settings.json` (per-project) |
| vscode | `mpr connect vscode` | `.vscode/mcp.json` (per-project) |
| amp | `mpr connect amp` | Platform-specific |
| droid | `mpr connect droid` | Platform-specific |

## Notes

- Agents with per-project config paths write to the current working directory.
- Run `mpr connect <agent>` with `--dry-run` to preview without writing files.
- The `mpr connect` subcommand detects whether each agent is installed ([detected] vs [not detected]).
