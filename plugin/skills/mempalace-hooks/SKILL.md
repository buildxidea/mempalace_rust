---
name: mempalace-hooks
description: Hook scripts for auto-capturing agent sessions in mempalace
---

Mempalace uses Claude Code hooks (via `.mcp.json` and `hooks.json`) to automatically capture observations during agent sessions. The hooks are JavaScript files in `plugin/scripts/`.

## Hook events

| Event | Script | When it fires |
|-------|--------|-------------|
| `SessionStart` | `session-start.mjs` | Agent session begins |
| `UserPromptSubmit` | `prompt-submit.mjs` | User sends a message |
| `PreToolUse` | `pre-tool-use.mjs` | Before tool execution |
| `PostToolUse` | `post-tool-use.mjs` | After tool execution |
| `PostToolUseFailure` | `post-tool-failure.mjs` | Tool error |
| `PreCompact` | `pre-compact.mjs` | Before context compaction |
| `SubagentStart` | `subagent-start.mjs` | Sub-agent spawns |
| `SubagentStop` | `subagent-stop.mjs` | Sub-agent finishes |
| `Notification` | `notification.mjs` | Agent notification |
| `TaskCompleted` | `task-completed.mjs` | Task marked done |
| `Stop` | `stop.mjs` | Agent stops |
| `SessionEnd` | `session-end.mjs` | Agent session ends |

## Hook config (Claude Code)

In `.claude/settings.local.json` or `.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [{
      "hooks": [{"type": "command", "command": "node \"${CLAUDE_PLUGIN_ROOT}/scripts/session-start.mjs\""}]
    }],
    "PostToolUse": [{
      "hooks": [{"type": "command", "command": "node \"${CLAUDE_PLUGIN_ROOT}/scripts/post-tool-use.mjs\""}]
    }],
    "Stop": [{
      "hooks": [{"type": "command", "command": "node \"${CLAUDE_PLUGIN_ROOT}/scripts/stop.mjs\""}]
    }]
  }
}
```

Also supports Codex (`.codex/hooks.json`) and Copilot (`.github/copilot/hooks.json`).
