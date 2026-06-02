# MemPalace Hooks — Rust wrappers

These scripts expose the same shell-hook surface as the Python reference hooks, but delegate the logic to the Rust CLI:

- `hooks/mempal_save_hook.sh` — `stop` hook (auto-save)
- `hooks/mempal_precompact_hook.sh` — `precompact` hook
- `hooks/mempal_hook.sh <kind> [harness]` — generic dispatcher for **all** lifecycle kinds:
  `session-start`, `session-end`, `stop`, `precompact`, `post-tool-use`, `post-tool-failure`,
  `prompt-submit`, `notification`, `subagent-start`, `subagent-stop`, `task-completed`

All scripts read JSON from stdin and print JSON to stdout. Every kind is also callable
directly via `mpr hook run --hook <kind> --harness <claude-code|codex>` without a wrapper.

## Usage

Claude Code:

```json
{
  "hooks": {
    "Stop": [{
      "matcher": "*",
      "hooks": [{
        "type": "command",
        "command": "/absolute/path/to/hooks/mempal_save_hook.sh",
        "timeout": 30
      }]
    }],
    "PreCompact": [{
      "hooks": [{
        "type": "command",
        "command": "/absolute/path/to/hooks/mempal_precompact_hook.sh",
        "timeout": 30
      }]
    }]
  }
}
```

Codex:

```json
{
  "Stop": [{
    "type": "command",
    "command": "/absolute/path/to/hooks/mempal_save_hook.sh codex",
    "timeout": 30
  }],
  "PreCompact": [{
    "type": "command",
    "command": "/absolute/path/to/hooks/mempal_precompact_hook.sh codex",
    "timeout": 30
  }]
}
```

## Environment variables

- `MEMPALACE_BIN` — absolute path to the `mpr` binary to execute (defaults to `mpr` on `PATH`)
- `MEMPALACE_HOOK_HARNESS` — override harness (`claude-code` or `codex`) without passing a positional argument

The actual save/precompact behavior lives in `mpr hook run`, which mirrors the Python hook contract in Rust.
