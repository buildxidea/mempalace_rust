# Getting Started

## Installation

### One-Line Install (Recommended)

```bash
curl -fsSL "https://raw.githubusercontent.com/quangdang46/mempalace_rust/main/install.sh?$(date +%s)" | bash
```

This script automatically:
- Builds the `mpr` binary
- Detects your installed AI tools (Claude Code, Codex, Cursor, Windsurf, VS Code, Gemini, OpenCode, Amp, Droid)
- Configures MCP for each detected tool

### From Source

```bash
git clone https://github.com/quangdang46/mempalace_rust.git
cd mempalace_rust
cargo build --release
cargo install --path crates/cli
```

### Requirements

- **Rust 1.75+** (workspace edition 2021; the build pulls in `tract-onnx` and `rmcp` which require recent Rust)
- **No API key.** Everything runs locally.

## Quick Start

Three steps: **init**, **mine**, **search**.

### 1. Initialize Your Palace

```bash
mpr init ~/projects/myapp
```

This scans your project directory and:
- Detects people and projects from file content
- Creates rooms from your folder structure
- Ensures the `~/.mempalace/` config directory exists
- Optionally creates `AGENT.md` / `USER.md` notes (skip with `--no-notes`)

Useful flags:
```bash
mpr init ~/projects/myapp --yes           # non-interactive
mpr init ~/projects/myapp --auto-mine     # run `mpr mine` right after
mpr init ~/projects/myapp --no-llm        # heuristics-only entity detection
mpr init ~/projects/myapp --search-strategy fts5   # choose default search engine
```

### 2. Mine Your Data

```bash
# Mine project files (code, docs, notes)
mpr mine ~/projects/myapp

# Mine conversation exports (Claude, ChatGPT, Slack, Codex, OpenCode)
mpr mine ~/chats/ --mode convos

# Auto-classify conversation content into memory types
mpr mine ~/chats/ --mode convos --extract general
```

Three mining modes:
- **projects** (default) — code and docs, auto-detected rooms
- **convos** — conversation exports, chunked by exchange pair
- **auto** — pick the mode based on file content

Supports **8+ chat formats** — Claude Code JSONL, Claude.ai JSON, ChatGPT JSON, Slack JSON, Codex CLI JSONL, SoulForge JSONL, OpenCode SQLite, plain text.

For large transcript dumps that contain multiple concatenated sessions, split them first:

```bash
mpr split ~/chats/ --dry-run
mpr split ~/chats/ --min-sessions 3
```

### 3. Search

```bash
mpr search "why did we switch to GraphQL"
```

Four search strategies are available (`--strategy` flag, or set the default at init time):
- `contains` (default) — exact-word substring match, 0MB
- `naive` — Jaccard-style token overlap, 0MB
- `bm25` — BM25 ranking via the `bm25` crate, 0MB
- `embedding` — vector similarity via ONNX `MiniLM` (384 dim), ~90MB

That gives you a working local memory index.

## What Happens Next

After the one-time setup, you don't run MemPalace commands manually. Your AI uses it for you through [MCP integration](/guide/mcp-integration).

Ask your AI anything:

> *"What did we decide about auth last month?"*

It calls `mempalace_search` automatically, gets verbatim results, and answers you. You never type `mpr search` again.

## Next Steps

- [Mining Your Data](/guide/mining) — deep dive into mining modes
- [Searching Memories](/guide/searching) — search strategies and filters
- [MCP Integration](/guide/mcp-integration) — connect to Claude, ChatGPT, Cursor, Gemini
- [The Palace](/concepts/the-palace) — understand wings, rooms, halls, and tunnels
