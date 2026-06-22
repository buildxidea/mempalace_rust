# Contributing

PRs welcome. MemPalace is open source and we welcome contributions of all sizes — from typo fixes to new features.

## Getting Started

```bash
git clone https://github.com/quangdang46/mempalace_rust.git
cd mempalace_rust

# Build the CLI
cargo build --release

# Install the binary onto your PATH
cargo install --path crates/cli

# (Optional) Build the HTTP REST API into the CLI
cargo build --release --features http-server
```

### Toolchain

- **Rust 1.75+** (workspace edition 2021)
- **No API key.** Everything runs locally.

## Running Tests

The workspace currently contains **1,400+ tests** spanning `crates/core` (unit + integration) and the CLI binary. Run them all:

```bash
# Fast — unit tests in the core crate
cargo test --workspace

# Full suite (adds CLI integration tests + conformance)
cargo test --all

# A specific test
cargo test -p mempalace-core config_roundtrip
```

All tests must pass before submitting a PR. The test suite is fully offline — no API keys or network access required (the embedder test fixtures use deterministic hashing instead of downloading ONNX weights).

## Running Benchmarks

```bash
# Build the benchmark harness
cargo build --release -p mempalace-bench

# LongMemEval — quick smoke test
./target/release/mempalace-bench longmemeval --limit 20

# LongMemEval — full 500 questions
./target/release/mempalace-bench longmemeval
```

See [Benchmarks](/reference/benchmarks) for the full methodology and result tables.

## Project Layout

```
crates/
  core/      ← the library (120+ modules, 1,400+ tests)
  cli/       ← thin binary wrapper around `mempalace_core::cli::run()`
  bench/     ← benchmark harness
```

CLI logic lives in `crates/core/src/cli.rs` (a single clap `Parser` with ~50 subcommands). Adding a new subcommand means adding a `Commands::Foo { … }` variant, a `cmd_foo()` function, and wiring it in `cli::run()`.

MCP tool logic lives in `crates/core/src/mcp_server.rs` (`make_tools()` builds the 84-tool set). Adding a tool means a `tool_foo()` handler plus a `tool("mempalace_foo", …)` entry in `make_tools()`.

## PR Guidelines

1. Fork the repo and create a feature branch: `git checkout -b feat/my-thing`
2. Write your code
3. Add or update tests — every behaviour change should come with at least one regression test
4. Run the full pre-commit checklist:

   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

5. Commit with clear [conventional commits](https://www.conventionalcommits.org/):
   - `feat: add Notion export format`
   - `fix(search): handle UTF-8 truncation in result text`
   - `chore: bump clap to 4.5`
6. Open a PR against `main`. CI runs the full build, test, clippy, and `cargo fmt --check` matrix.

## Issue Tracking

Beads (`br`) is the source of truth for project status, priority, and dependencies. Agent Mail is used for inter-agent coordination and file reservations.

```bash
# Pick up ready work
br ready

# Update status as you work
br update br-123 --status in_progress

# Close when done
br close br-123 --reason "Completed"
br sync --flush-only    # export to .beads/beads.jsonl
```

See `.beads/` for the live issue list.

## Documentation

- **User-facing docs** (this site): `website/`
- **API / architecture notes**: `docs/`
- **Specs and RFCs**: `specs/`
- **In-repo AI instructions**: `AGENTS.md` at the repo root

When you change CLI behaviour, update `website/reference/cli.md`. When you add an MCP tool, update `website/reference/mcp-tools.md`. The docs are version-controlled alongside the code, so they should never drift.
