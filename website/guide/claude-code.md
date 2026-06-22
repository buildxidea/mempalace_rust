# Claude Code Plugin

The recommended way to use MemPalace with Claude Code.

## Installation

### One-line install (recommended)

```bash
curl -fsSL "https://raw.githubusercontent.com/quangdang46/mempalace_rust/main/install.sh?$(date +%s)" | bash
```

The installer auto-detects Claude Code, builds `mpr`, and registers the MCP server. No manual config needed.

### Manual MCP

If you prefer manual setup:

```bash
claude mcp add mpr -- mpr serve
```

Both approaches give identical functionality. Restart Claude Code, then ask:

> *"What did we decide about auth last month?"*

Claude calls `mempalace_search` automatically, gets verbatim results, and answers you. The full tool set is documented in [MCP Tools Reference](/reference/mcp-tools).

## How It Works

With `mpr` on the PATH and `claude mcp add mpr -- mpr serve` registered, Claude Code automatically:

- Spawns the MemPalace MCP server on launch
- Has access to all 84 tools (prefixed `mempalace_`)
- Learns the AAAK dialect and memory protocol from the `mempalace_status` response
- Searches the palace before answering questions about past work

## Marketplace Plugin (legacy)

If your team uses the marketplace plugin format:

```bash
claude plugin marketplace add MemPalace/mempalace
claude plugin install --scope user mempalace
```

Restart Claude Code, then type `/skills` to verify "mempalace" appears.

## Auto-Save Hooks

Set up [auto-save hooks](/guide/hooks) to ensure memories are saved automatically during long conversations. The hooks live at `plugin/hooks/` in the repository and call into `mpr hook` under the hood.
