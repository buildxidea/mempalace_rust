# CLI Commands

All commands accept `--palace <path>` to override the default palace location (`~/.mempalace/palace`).

Run `mpr --help` for the full list. Commands below are grouped by purpose.

## Setup

### `mpr init`

Scan a project directory for people, projects, and rooms, and set up the palace. `<dir>` is **required**.

```bash
mpr init <dir>
mpr init ~/projects/myapp --yes
mpr init . --auto-mine
mpr init ~/projects/myapp --search-strategy fts5
mpr init ~/projects/myapp --no-llm
```

| Option | Default | Description |
|--------|---------|-------------|
| `<dir>` | — | **Required.** Project directory to scan. Pass `.` for the current directory. |
| `--yes` | — | Auto-accept all detected entities (non-interactive) |
| `--llm-provider` | `ollama` | LLM provider for entity refinement (`ollama`, `openai`, …) |
| `--llm-model` | `gemma4:e4b` | Model name for the chosen provider |
| `--llm-endpoint` | `http://localhost:11434` | Provider endpoint URL |
| `--llm-api-key` | — | API key for the provider |
| `--no-llm` | — | Disable LLM-assisted entity refinement (heuristics-only mode) |
| `--accept-external-llm` | — | Bypass interactive consent prompt for external LLM |
| `--auto-mine` | — | Automatically run `mpr mine` after init completes |
| `--lang` | auto | Spoken language hint for entity detection |
| `--search-strategy` | `contains` | One of `contains`, `naive`, `bm25`, `embedding` (see [Searching](/guide/searching)) |
| `--no-notes` | — | Skip creating `AGENT.md` / `USER.md` notes |

What it does:

1. Scans `<dir>` for people and projects in file content
2. Detects rooms from `<dir>`'s folder structure
3. Saves detected entities to `<dir>/entities.json`
4. Ensures the global `~/.mempalace/` config directory exists

## Mining & Splitting

### `mpr mine`

Mine files into the palace.

```bash
mpr mine ~/projects/myapp
mpr mine ~/chats/ --mode convos
mpr mine ~/chats/ --mode convos --extract general
mpr mine ~/data/ --wing myapp
```

| Option | Default | Description |
|--------|---------|-------------|
| `<dir>` | — | Directory to mine |
| `--mode` | `projects` | `projects`, `convos`, or `auto` |
| `--wing` | directory name | Wing name override |
| `--agent` | `mpr` | Agent name tag recorded on every drawer |
| `--limit` | `0` (all) | Max files to process |
| `--dry-run` | — | Preview without filing |
| `--extract` | `exchange` | `exchange` or `general` (for convos mode) |
| `--no-gitignore` | — | Don't respect `.gitignore` |
| `--include-ignored` | — | Always scan these paths even if ignored (repeat or comma-separated) |
| `--redetect-origin` | — | Re-run corpus-origin detection on existing drawers |
| `--max-chunks-per-file` | `50000` | Override per-file chunk cap (0 disables; lower to bound ONNX worst-case batches) |

Supports **8+ chat formats** — Claude Code JSONL, Claude.ai JSON, ChatGPT JSON, Slack JSON, Codex CLI JSONL, SoulForge JSONL, OpenCode SQLite, plain text.

### `mpr split`

Split concatenated transcript mega-files into per-session files.

```bash
mpr split <dir>
mpr split <dir> --dry-run
mpr split <dir> --min-sessions 3
mpr split <dir> --output-dir ~/split-output/
```

| Option | Default | Description |
|--------|---------|-------------|
| `<dir>` | — | Directory with transcript files |
| `--output-dir` | same dir | Write split files here |
| `--dry-run` | — | Preview without writing |
| `--min-sessions` | `2` | Only split files with N+ sessions |

### `mpr mine-device` *(hidden)*

Internal Rust-only helper to mine discovered device sessions. Exposed for tooling.

```bash
mpr mine-device --wing myapp --dry-run
```

### `mpr sweep`

Re-ingest a file or directory of mined drawers into the palace (idempotent). Use after editing drawers externally or to recover from partial runs.

```bash
mpr sweep <target>
mpr sweep ./palace --palace /custom/palace
```

## Search & Context

### `mpr search`

Find anything by keyword or vector search.

```bash
mpr search "why did we switch to GraphQL"
mpr search "database decision" --wing myapp --room auth
mpr search "deploy process" --results 10
mpr search "auth" --strategy bm25 --bm25
mpr search "auth" --strategy embedding --fusion-mode hybrid
mpr search "auth" --json
```

| Option | Default | Description |
|--------|---------|-------------|
| `"query"` | — | What to search for |
| `--wing` | all | Filter by wing |
| `--room` | all | Filter by room |
| `--results` | `5` | Number of results |
| `--bm25` | — | Enable BM25 reranking for better relevance |
| `--fusion-mode` | `vector` | `vector`, `ppr`, or `hybrid` |
| `--strategy` | from config | `contains`, `naive`, `bm25`, or `embedding` (per-call override) |
| `--json` | — | Output results as JSON for piping / external consumers |

### `mpr wake-up`

Show L0 + L1 wake-up context (~600–900 tokens).

```bash
mpr wake-up
mpr wake-up --wing myapp
```

| Option | Description |
|--------|-------------|
| `--wing` | Project-specific wake-up |

### `mpr context`

Show context/breadcrumbs for the current session.

```bash
mpr context
mpr context --levels 5
```

| Option | Default | Description |
|--------|---------|-------------|
| `--levels` | `3` | Number of context levels to show |

## Compression & Memory Lifecycle

### `mpr compress`

Compress drawers using [AAAK Dialect](/concepts/aaak-dialect) (~5–30× reduction, lossy).

```bash
mpr compress --wing myapp
mpr compress --wing myapp --dry-run
mpr compress --config entities.json
```

| Option | Description |
|--------|-------------|
| `--wing` | Wing to compress (default: all wings) |
| `--dry-run` | Preview compression without storing |
| `--config` | Entity config JSON (e.g. `entities.json`) |

### `mpr consolidate`

Run the consolidation pipeline to merge and refine memories.

```bash
mpr consolidate --dry-run
mpr consolidate --max-memories 1000
```

| Option | Description |
|--------|-------------|
| `--dry-run` | Run without persisting changes |
| `--max-memories` | Maximum memories to consolidate |

### `mpr evolve`

Refine memories using an LLM.

```bash
mpr evolve --wing myapp --count 25
```

| Option | Default | Description |
|--------|---------|-------------|
| `--wing` | all | Wing to evolve |
| `--count` | `10` | Number to evolve |

### `mpr forget`

Forget/evict memories by age or type.

```bash
mpr forget --older-than-days 90 --dry-run
mpr forget --memory-type preference
```

| Option | Description |
|--------|-------------|
| `--older-than-days` | Forget by age (days) |
| `--memory-type` | Forget by memory type |
| `--dry-run` | Preview without deleting |

## Export, Import, Snapshot

### `mpr export`

Export the palace to a directory of Markdown files (Obsidian-compatible).

```bash
mpr export ./vault
mpr export ./vault --format markdown
```

| Option | Default | Description |
|--------|---------|-------------|
| `<output_dir>` | — | Output directory for the exported vault |
| `--format` | `basic-memory` | `basic-memory` (Markdown/Obsidian) or `markdown` |

### `mpr import`

Import data from external sources.

```bash
mpr import json ./dump.json
mpr import markdown ./notes/
```

| Option | Description |
|--------|-------------|
| `<format>` | `json`, `csv`, or `markdown` |
| `<input>` | Input file or directory |

### `mpr snapshot`

Create or inspect a memory snapshot.

```bash
mpr snapshot --name before-refactor
mpr snapshot --name v1 --with-embeddings
```

| Option | Description |
|--------|-------------|
| `--name` | Snapshot name |
| `--with-embeddings` | Include embeddings in the snapshot |

## Diagnostics & Maintenance

### `mpr status`

Show what's been filed — drawer count, wing/room breakdown, configuration summary.

```bash
mpr status
```

### `mpr diagnose`

Diagnose palace health issues.

```bash
mpr diagnose
mpr diagnose --deep
```

| Option | Description |
|--------|-------------|
| `--deep` | Run deep diagnostics |

### `mpr profile`

Show project/profile insights.

```bash
mpr profile --wing myapp
mpr profile --wing myapp --refresh
```

| Option | Description |
|--------|-------------|
| `--wing` | Project/wing name (default: `default`) |
| `--refresh` | Refresh profile data |

### `mpr sessions`

List recent mining sessions.

```bash
mpr sessions --wing myapp --limit 20
```

| Option | Default | Description |
|--------|---------|-------------|
| `--wing` | all | Filter by wing |
| `--limit` | `20` | Limit results |

### `mpr actions`

List active actions tracked by the ActionStore.

```bash
mpr actions --status pending --limit 50
```

| Option | Default | Description |
|--------|---------|-------------|
| `--status` | all | Filter by status: `pending`, `running`, `completed`, `failed` |
| `--limit` | `50` | Limit results |

### `mpr frontier`

Show frontier tasks (pending work items).

```bash
mpr frontier --agent reviewer
mpr frontier --include-completed
```

| Option | Description |
|--------|-------------|
| `--agent` | Filter by agent |
| `--include-completed` | Show completed items too |

### `mpr signals`

Read, send, or list inter-agent signals.

```bash
mpr signals read
mpr signals send --to reviewer --payload "PR#42 ready"
mpr signals list
```

| Operation | Description |
|-----------|-------------|
| `read` | Read pending signals for this agent |
| `send` | Send a signal (use `--to` and `--payload`) |
| `list` | List recent signals |

### `mpr mesh`

Sync or inspect the mesh between agents.

```bash
mpr mesh --operation sync
mpr mesh --operation status
mpr mesh --operation peers
```

| Operation | Description |
|-----------|-------------|
| `sync` | Sync mesh with peers |
| `status` | Show sync status |
| `peers` | List known peers |

### `mpr vision`

Vision search for images attached to memories.

```bash
mpr vision "dashboard mockup" --limit 10
```

| Option | Default | Description |
|--------|---------|-------------|
| `"query"` | — | Search query |
| `--limit` | `10` | Max results |

## Repair

### `mpr repair`

Subcommands to repair the palace index. Run these to recover from corruption or schema drift.

```bash
mpr repair scan
mpr repair scan --wing myapp
mpr repair prune --confirm
mpr repair rebuild
mpr repair cleanup-pid
mpr repair migrate-vector-index
```

| Subcommand | Description |
|------------|-------------|
| `scan` | Scan for corrupt/unfetchable drawer IDs |
| `prune` | Delete corrupt IDs (requires `--confirm`) |
| `rebuild` | Rebuild the palace index |
| `cleanup-pid` | Clean up stale PID file from interrupted mine operations |
| `migrate-vector-index` | Re-index with the current embedder after schema changes |

## Notes & User Profile

`.md` notes — human-readable, version-controllable. See [Memory Stack](/concepts/memory-stack) for the layer model.

### `mpr remember`

Append a note to `AGENT.md` (the agent's personal notes).

```bash
mpr remember "Switched auth provider to Clerk on 2026-01-15"
mpr remember "Decided to use" "GraphQL" "for the new dashboard"
```

### `mpr recall`

Print all notes (`AGENT.md` + `USER.md`).

```bash
mpr recall
```

### `mpr user`

Show or modify the user profile (`USER.md`).

```bash
mpr user set role "Founding engineer"
mpr user get role
mpr user list
```

| Subcommand | Description |
|------------|-------------|
| `set <key> <value>` | Set a `key=value` entry |
| `get <key>` | Get a value by key |
| `list` | List all `key=value` entries |

## Configuration

### `mpr config`

Inspect MemPalace configuration.

```bash
mpr config show
mpr config path
```

| Subcommand | Description |
|------------|-------------|
| `show` | Print the current configuration |
| `path` | Print the configuration file path |

For runtime configuration fields, see [Configuration](/guide/configuration).

## Server & Lifecycle

### `mpr serve`

Run the MemPalace MCP server (stdio) or HTTP REST API.

```bash
mpr serve                          # stdio MCP server (default)
mpr serve --http                   # HTTP REST API on port 3111
mpr serve --http --port 4111       # custom REST port
mpr serve --http --instance 1      # ports 3211 / 3212 / 49234
mpr serve --read-only              # block mutations
mpr serve --no-background          # disable auto-forget / consolidation tasks
```

| Option | Description |
|--------|-------------|
| `--http` | Start HTTP REST API server instead of stdio MCP |
| `--port` | HTTP REST port override (default: `3111`, env: `MEMPALACE_HTTP_PORT`) |
| `--instance N` | Multi-instance: assigns REST `3111+N*100`, stream `3112+N*100`, engine `49134+N*100`. Max 50. Config env: `MEMPALACE_INSTANCE` (default: 0). |
| `--read-only` | Block all mutations |
| `--no-background` | Disable background maintenance tasks (auto-forget, consolidation) |

### `mpr mcp`

Helper command that prints the exact MCP setup syntax for your AI client. Useful for scripted wiring.

```bash
mpr mcp
mpr mcp --palace ~/.custom-palace
```

### `mpr connect`

Wire MemPalace as an MCP server to a third-party agent. Supports `claude-code`, `codex`, `cursor`, `kiro`, `warp`, `cline`, `continue_dev`, `zed`, `openhuman`, `qwen`, `antigravity`. Pass no adapter to list supported adapters.

```bash
mpr connect claude-code
mpr connect cursor --dry-run
```

### `mpr hook`

Capture a lifecycle hook observation (used by Claude Code / Codex integration).

```bash
mpr hook --hook notification --session-id sess-1 --project myapp --data '{"k":"v"}'
```

| Option | Default | Description |
|--------|---------|-------------|
| `--hook` | `notification` | Hook type: `session_end`, `post_tool_use`, `stop`, `notification`, … |
| `--session-id` | `cli-session` | Session ID this observation belongs to |
| `--project` | `default` | Project name |
| `--cwd` | `.` | Working directory |
| `--data` | — | JSON payload for the observation |

### `mpr instructions`

Output skill instructions to stdout — for AI agents that consume the instruction text directly.

```bash
mpr instructions init
mpr instructions search
mpr instructions mine
mpr instructions help
mpr instructions status
```

### `mpr stop`

Stop a running MemPalace engine by PID file.

```bash
mpr stop
mpr stop --pid-file /tmp/mpr.pid
mpr stop --kill
```

| Option | Default | Description |
|--------|---------|-------------|
| `--pid-file` | `~/.mempalace/run/mpr.pid` | PID file path |
| `--kill` | — | Send SIGKILL instead of SIGTERM |

### `mpr upgrade`

Print upgrade instructions for MemPalace and its dependencies. Pass `--apply` to upgrade in-place.

```bash
mpr upgrade
mpr upgrade --apply
```

### `mpr demo`

Seed a demo palace with example memories for first-run exploration.

```bash
mpr demo
mpr demo --dir ~/demo --force
```

| Option | Default | Description |
|--------|---------|-------------|
| `--dir` | `./mempalace-demo` | Directory to create the demo palace in |
| `--force` | — | Overwrite an existing demo palace at the target directory |

### `mpr remove`

Remove MemPalace data and config from this machine.

```bash
mpr remove --force
mpr remove --palace-only       # only remove active palace data dir, keep global config
mpr remove --all               # remove palace data AND global config directory
```

| Option | Description |
|--------|-------------|
| `--force` | Skip the confirmation prompt |
| `--palace-only` | Only remove the active palace data dir, keep global config |
| `--all` | Remove palace data AND global config directory entirely |

### `mpr deinit`

De-initialize the palace: remove palace data AND global config.

```bash
mpr deinit --force
```
