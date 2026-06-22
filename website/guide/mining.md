# Mining Your Data

MemPalace ingests your data by **mining** — scanning files and filing their content as verbatim drawers in the palace.

## Mining Modes

### Projects Mode (default)

Scans code, docs, and notes. Respects `.gitignore` by default.

```bash
mpr mine ~/projects/myapp
```

Each file becomes a drawer, tagged with a wing (project name) and room (topic). Rooms are auto-detected from your folder structure during `mpr init`.

Options:
```bash
# Override wing name
mpr mine ~/projects/myapp --wing myapp

# Ignore .gitignore rules
mpr mine ~/projects/myapp --no-gitignore

# Include specific ignored paths (repeat flag or comma-separated)
mpr mine ~/projects/myapp --include-ignored dist --include-ignored build
mpr mine ~/projects/myapp --include-ignored dist,build

# Limit number of files
mpr mine ~/projects/myapp --limit 100

# Preview without filing
mpr mine ~/projects/myapp --dry-run

# Override per-file chunk cap (lower to bound ONNX worst-case batches on Windows)
mpr mine ~/projects/myapp --max-chunks-per-file 5000
```

### Conversations Mode

Indexes conversation exports from Claude, ChatGPT, Slack, Codex, OpenCode, and other tools. Chunks by exchange pair (human + assistant turns).

```bash
mpr mine ~/chats/ --mode convos
```

Supports **8+ chat formats** automatically:
- Claude Code JSONL
- Claude.ai JSON
- ChatGPT JSON
- Slack JSON
- Codex CLI JSONL
- SoulForge JSONL
- OpenCode SQLite
- Plain text / Markdown

### Auto Mode

Let `mpr` pick `projects` or `convos` based on the file content. Useful when a directory contains a mix of code and chat dumps.

```bash
mpr mine ~/data/ --mode auto
```

### General Extraction

Auto-classifies conversation content into five memory types:

```bash
mpr mine ~/chats/ --mode convos --extract general
```

Memory types:
- **Decisions** — choices made, options rejected
- **Preferences** — habits, likes, opinions
- **Milestones** — sessions completed, goals reached
- **Problems** — bugs, blockers, issues encountered
- **Emotional context** — reactions, concerns, excitement

## Splitting Mega-Files

Some transcript exports concatenate multiple sessions into one huge file. Split them first:

```bash
# Preview what would be split
mpr split ~/chats/ --dry-run

# Split files with 2+ sessions (default)
mpr split ~/chats/

# Only split files with 3+ sessions
mpr split ~/chats/ --min-sessions 3

# Output to a different directory
mpr split ~/chats/ --output-dir ~/chats-split/
```

::: tip
Always run `mpr split` before mining conversation files. It's a no-op if files don't need splitting.
:::

## Multi-Project Setup

Mine each project into its own wing:

```bash
mpr mine ~/chats/orion/  --mode convos --wing orion
mpr mine ~/chats/nova/   --mode convos --wing nova
mpr mine ~/chats/helios/ --mode convos --wing helios
```

Six months later:
```bash
# Project-specific search
mpr search "database decision" --wing orion

# Cross-project search
mpr search "rate limiting approach"
# → finds your approach in Orion AND Nova, shows the differences
```

## Team Usage

Mine Slack exports and AI conversations for team history:

```bash
mpr mine ~/exports/slack/ --mode convos --wing driftwood
mpr mine ~/.claude/projects/ --mode convos
```

Then search across people and projects:
```bash
mpr search "Soren sprint" --wing driftwood
# → 14 closets: OAuth refactor, dark mode, component library migration
```

## Agent Tag

Every drawer is tagged with the agent that filed it. The default is `mpr` (the CLI binary), so plan-style drawers can be partitioned by source. Override per-call:

```bash
# Default agent name
mpr mine ~/data/

# Custom agent name (e.g. a specific bot or reviewer)
mpr mine ~/data/ --agent reviewer
mpr mine ~/data/ --agent codex
```

This is used by [Specialist Agents](/concepts/agents) to partition memories.
