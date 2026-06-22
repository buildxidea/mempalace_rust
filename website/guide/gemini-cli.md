# Gemini CLI

MemPalace works natively with [Gemini CLI](https://github.com/google/gemini-cli), which handles the MCP server and save hooks automatically.

## Prerequisites

- Rust 1.75+
- Gemini CLI installed and configured

## Installation

### One-Line Install (Recommended)

```bash
curl -fsSL "https://raw.githubusercontent.com/quangdang46/mempalace_rust/main/install.sh?$(date +%s)" | bash
```

This automatically builds `mpr`, detects Gemini CLI, and configures MCP for it.

### Manual Installation

```bash
git clone https://github.com/quangdang46/mempalace_rust.git
cd mempalace_rust
cargo build --release
cargo install --path crates/cli
```

## Initialize the Palace

```bash
mpr init .
```

Useful flags:

```bash
mpr init . --yes           # non-interactive
mpr init . --auto-mine     # run mpr mine right after
mpr init . --no-llm        # heuristics-only
```

### Identity and Notes (Optional)

After init, you can optionally edit:

- **`~/.mempalace/identity.txt`** — plain text describing your role and focus (becomes Layer 0)
- **`AGENT.md`** / **`USER.md`** — created by init in the palace notes directory; managed via `mpr remember` and `mpr user set`

## Connect to Gemini CLI

Register MemPalace as an MCP server:

```bash
gemini mcp add mpr -- mpr serve
```

## Enable Auto-Saving

Add hooks to `~/.gemini/settings.json` (the exact event names depend on your Gemini CLI version):

```json
{
  "hooks": {
    "PreCompress": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "/absolute/path/to/mempalace_rust/plugin/hooks/mempal_precompact_hook.sh"
          }
        ]
      }
    ]
  }
}
```

Make sure the hook scripts are executable:

```bash
chmod +x plugin/hooks/*.sh
```

## Usage

Once connected, Gemini CLI will automatically:
- Start the MemPalace server on launch
- Use `mempalace_search` to find relevant past discussions
- Use the `PreCompress` hook to save memories before context compression

### Manual Mining

Mine existing code or docs:

```bash
mpr mine /path/to/your/project
mpr mine /path/to/chats --mode convos
```

### Verification

In a Gemini CLI session:
- `/mcp list` — verify `mpr` is `CONNECTED`
- `/hooks panel` — verify the `PreCompress` hook is active
