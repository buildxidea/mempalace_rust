# Auto-Save Hooks

MemPalace ships lifecycle hooks for Claude Code and Codex. Hooks run on the host machine, call into the Rust `mpr` binary, and let your AI auto-save during work — no manual "save" commands needed.

The hooks are thin bash wrappers around `mpr hook`; you don't have to write any logic, just register the path in your AI client's settings.

## What They Do

| Hook | When It Fires | What Happens |
|------|--------------|-------------|
| **Save Hook** (`mpr hook --hook stop`) | Every ~15 human messages | Records the session observation, increments counter, lets the AI stop |
| **PreCompact Hook** (`mpr hook --hook pre_compact`) | Right before context compaction | Forces an observation save before the context window collapses |

The hook counter and auto-save behaviour are tracked by MemPalace itself (in the `sessions` SQLite store inside the palace). The shell wrappers just relay the lifecycle event into `mpr hook`. Any additional memory filing happens via the `mempalace_*` MCP tools.

## Install — One-Line

The recommended path is the installer, which detects Claude Code / Codex and registers both the wrappers and the `~/.claude/settings.json` hooks automatically:

```bash
curl -fsSL "https://raw.githubusercontent.com/quangdang46/mempalace_rust/main/install.sh?$(date +%s)" | bash
```

This creates the wrapper scripts under:

- Linux:   `~/.local/share/mempalace/hooks/`
- macOS:   `~/Library/Application Support/mempalace/hooks/`
- Windows: `%LOCALAPPDATA%\mempalace\hooks\`

…and patches `~/.claude/settings.json` to wire them up.

## Install — Manual (Claude Code)

If you prefer manual setup, place the wrapper scripts from `plugin/hooks/` anywhere on disk and make them executable:

```bash
chmod +x plugin/hooks/mempal_save_hook.sh plugin/hooks/mempal_precompact_hook.sh
```

Then add to `~/.claude/settings.json` (or `.claude/settings.local.json`):

```json
{
  "hooks": {
    "Stop": [{
      "matcher": "*",
      "hooks": [{
        "type": "command",
        "command": "/absolute/path/to/plugin/hooks/mempal_save_hook.sh",
        "timeout": 30
      }]
    }],
    "PreCompact": [{
      "hooks": [{
        "type": "command",
        "command": "/absolute/path/to/plugin/hooks/mempal_precompact_hook.sh",
        "timeout": 30
      }]
    }]
  }
}
```

The `mempal_save_hook.sh` and `mempal_precompact_hook.sh` scripts in this repository are the canonical wrappers; the installer copies them to the platform-specific locations above and registers them.

## Install — Codex CLI

Codex reads `~/.codex/hooks.json`:

```json
{
  "Stop": [{
    "type": "command",
    "command": "/absolute/path/to/mempal_save_hook.sh",
    "timeout": 30
  }],
  "PreCompact": [{
    "type": "command",
    "command": "/absolute/path/to/mempal_precompact_hook.sh",
    "timeout": 30
  }]
}
```

## How the Save Hook Works

```
User sends message → AI responds → Stop hook fires
                                          ↓
                            mpr hook --hook stop
                                          ↓
                  SessionStore.add_observation(&obs)
                                          ↓
              counter increments → if >= SAVE_INTERVAL (15)
                                          ↓
                            AI is prompted to save
                                          ↓
                                    AI stops
```

The Rust side increments a counter per session; the AI itself is responsible for the actual memory filing via `mempalace_search` + `mempalace_add_drawer`.

## PreCompact Hook

```
Context window full → PreCompact fires → mpr hook --hook pre_compact
                                                ↓
                                  SessionStore observation saved
                                                ↓
                                          Compaction proceeds
```

No counting needed — compaction always warrants a save.

## Direct `mpr hook` Usage

You can invoke the same plumbing from your own scripts:

```bash
# Default — record a notification observation
mpr hook

# Specific hook type
mpr hook --hook stop --session-id sess-1 --project myapp

# With a JSON payload
mpr hook --hook post_tool_use --session-id sess-1 --project myapp \
  --data '{"tool":"Edit","path":"src/lib.rs"}'
```

| Option | Default | Description |
|--------|---------|-------------|
| `--hook` | `notification` | Hook type: `session_end`, `post_tool_use`, `stop`, `notification`, `pre_compact`, … |
| `--session-id` | `cli-session` | Session ID this observation belongs to |
| `--project` | `default` | Project name |
| `--cwd` | `.` | Working directory |
| `--data` | — | JSON payload for the observation |

The auto-save cadence lives in the Rust `cli.rs` constant `SAVE_INTERVAL: usize = 15`; tune by rebuilding from source if you need a different cadence.

## Debugging

Hook activity is recorded in the palace `sessions` SQLite store. Inspect recent sessions:

```bash
mpr sessions --limit 20
```

Or query the underlying store directly:

```bash
sqlite3 ~/.mempalace/palace/sessions.sqlite \
  "SELECT id, started_at, ended_at FROM sessions ORDER BY started_at DESC LIMIT 10;"
```

## Cost

**Zero extra tokens.** The hooks run locally and only invoke Rust on your machine. The only "cost" is a few hundred milliseconds of local processing at each checkpoint, plus the AI's own follow-up memory-filing turns via MCP.
